//! Boundary decisions for protocol events before they become TUI projections.

use code_core::protocol::{Event, EventMsg};

use super::ChatWidget;

/// Returns whether an event may enter the turn-presentation pipeline.
///
/// SessionConfigured is the bootstrap event; all other events are stale until
/// the widget has a session, except in isolated test mode.
pub(crate) fn should_process_event(
    has_session: bool,
    test_mode: bool,
    is_session_configured: bool,
) -> bool {
    has_session || test_mode || is_session_configured
}

/// Applies the common event gate and records the provider ordering anchor.
/// The event-specific projection remains in `ChatWidget` for now, but every
/// protocol event crosses this boundary before it can mutate history/stream
/// state.
pub(crate) fn accept_event(chat: &mut ChatWidget<'_>, event: Event) -> Option<Event> {
    if !should_process_event(
        chat.session_id.is_some(),
        chat.test_mode,
        matches!(&event.msg, EventMsg::SessionConfigured(_)),
    ) {
        tracing::debug!(
            "Ignoring stale event {:?} (seq={}) while waiting for SessionConfigured",
            &event.msg,
            event.event_seq
        );
        return None;
    }

    chat.note_order(event.order.as_ref());
    Some(event)
}

#[cfg(test)]
mod tests {
    use super::should_process_event;

    #[test]
    fn gates_events_until_session_is_configured() {
        assert!(!should_process_event(false, false, false));
        assert!(should_process_event(false, false, true));
        assert!(should_process_event(false, true, false));
        assert!(should_process_event(true, false, false));
    }
}
