use std::sync::mpsc::Sender;

use crate::app_event::{AppEvent, BackgroundPlacement};
use crate::chatwidget::BackgroundOrderTicket;
use crate::session_log;

#[derive(Clone, Debug)]
pub(crate) struct AppEventSender {
    // High‑priority events (input, resize, redraw scheduling) are routed here.
    high_tx: Sender<AppEvent>,
    // Bulk/streaming events (history inserts, commit ticks, file search, etc.).
    bulk_tx: Sender<AppEvent>,
}

impl AppEventSender {
    /// Create a sender that splits events by priority across two channels.
    pub(crate) fn new_dual(high_tx: Sender<AppEvent>, bulk_tx: Sender<AppEvent>) -> Self {
        Self { high_tx, bulk_tx }
    }
    /// Backward‑compatible constructor for tests/fixtures that expect a single
    /// channel. Routes both high‑priority and bulk events to the same sender.
    #[allow(dead_code)]
    pub(crate) fn new(app_event_tx: Sender<AppEvent>) -> Self {
        Self { high_tx: app_event_tx.clone(), bulk_tx: app_event_tx }
    }

    /// Send an event to the app event channel. If it fails, we swallow the
    /// error and log it.
    pub(crate) fn send(&self, event: AppEvent) {
        let _ = self.send_with_result(event);
    }

    /// Send an event while surfacing whether the channel was still connected.
    /// Returns `true` if the event was delivered, `false` if the channel was
    /// disconnected (already logged).
    pub(crate) fn send_with_result(&self, event: AppEvent) -> bool {
        // Record inbound events for high-fidelity session replay.
        // Avoid double-logging Ops; those are logged at the point of submission.
        if !matches!(event, AppEvent::CodexOp(_)) {
            session_log::log_inbound_app_event(&event);
        }
        let is_high = matches!(
            event,
            AppEvent::KeyEvent(..)
                | AppEvent::MouseEvent(_)
                | AppEvent::Paste(_)
                | AppEvent::RequestRedraw
                | AppEvent::Redraw
                | AppEvent::ExitRequest
                | AppEvent::ClearUi
                | AppEvent::SetTerminalTitle { .. }
                | AppEvent::EmitTuiNotification { .. }
                | AppEvent::AutoCoordinatorCountdown { .. }
        );

        let tx = if is_high { &self.high_tx } else { &self.bulk_tx };
        match tx.send(event) {
            Ok(()) => true,
            Err(e) => {
                tracing::error!("failed to send event: {e}");
                false
            }
        }
    }

    pub(crate) fn send_background_event_with_ticket(
        &self,
        ticket: &BackgroundOrderTicket,
        message: impl Into<String>,
    ) {
        self.send(AppEvent::InsertBackgroundEvent {
            session_id: Some(ticket.session_id()),
            message: message.into(),
            placement: BackgroundPlacement::Tail,
            order: Some(ticket.next_order()),
        });
    }

    pub(crate) fn send_background_before_next_output_with_ticket(
        &self,
        ticket: &BackgroundOrderTicket,
        message: impl Into<String>,
    ) {
        self.send(AppEvent::InsertBackgroundEvent {
            session_id: Some(ticket.session_id()),
            message: message.into(),
            placement: BackgroundPlacement::BeforeNextOutput,
            order: Some(ticket.next_order()),
        });
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    #[test]
    fn auto_coordinator_countdown_bypasses_bulk_backlog() {
        let (high_tx, high_rx) = channel();
        let (bulk_tx, bulk_rx) = channel();
        let sender = AppEventSender::new_dual(high_tx, bulk_tx);

        for idx in 0..64 {
            sender.send(AppEvent::AutoCoordinatorAction {
                message: format!("bulk-{idx}"),
            });
        }

        sender.send(AppEvent::AutoCoordinatorCountdown {
            countdown_id: 99,
            seconds_left: 9,
        });

        let prioritized = high_rx
            .recv_timeout(Duration::from_millis(50))
            .expect("countdown event should bypass bulk backlog");
        assert!(matches!(
            prioritized,
            AppEvent::AutoCoordinatorCountdown {
                countdown_id: 99,
                seconds_left: 9
            }
        ));

        assert!(matches!(
            bulk_rx.try_recv(),
            Ok(AppEvent::AutoCoordinatorAction { .. })
        ));
    }
}
