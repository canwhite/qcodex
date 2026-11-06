use code_core::CodexConversation;
use code_core::protocol::{InputItem as CoreInputItem, Op};
use code_protocol::mcp_protocol::InputItem as WireInputItem;
use std::collections::HashMap;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::outgoing_message::ConnectionId;

pub(crate) struct ListenerRegistration {
    pub(crate) owner_connection_id: ConnectionId,
    pub(crate) cancel_tx: oneshot::Sender<()>,
}

pub(crate) struct ListenerRegistry {
    registrations: HashMap<Uuid, ListenerRegistration>,
}

impl ListenerRegistry {
    pub(crate) fn new() -> Self {
        Self {
            registrations: HashMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, id: Uuid, registration: ListenerRegistration) {
        self.registrations.insert(id, registration);
    }

    pub(crate) fn remove(&mut self, id: &Uuid) -> Option<ListenerRegistration> {
        self.registrations.remove(id)
    }

    #[cfg(test)]
    pub(crate) fn contains_key(&self, id: &Uuid) -> bool {
        self.registrations.contains_key(id)
    }

    pub(crate) fn take_owned(
        &mut self,
        connection_id: ConnectionId,
    ) -> Vec<ListenerRegistration> {
        let registrations = std::mem::take(&mut self.registrations);
        let mut owned = Vec::new();
        for (id, registration) in registrations {
            if registration.owner_connection_id == connection_id {
                owned.push(registration);
            } else {
                self.registrations.insert(id, registration);
            }
        }
        owned
    }
}

/// Maps the wire representation once at the conversation boundary.
pub(crate) fn map_input_items(items: Vec<WireInputItem>) -> Vec<CoreInputItem> {
    items
        .into_iter()
        .map(|item| match item {
            WireInputItem::Text { text } => CoreInputItem::Text { text },
            WireInputItem::Image { image_url } => CoreInputItem::Image { image_url },
            WireInputItem::LocalImage { path } => CoreInputItem::LocalImage { path },
        })
        .collect()
}

/// Submits a user turn through the single conversation bridge path.
pub(crate) async fn submit_user_input(
    conversation: &CodexConversation,
    items: Vec<WireInputItem>,
) -> Result<(), String> {
    conversation
        .submit(Op::UserInput {
            items: map_input_items(items),
            final_output_json_schema: None,
        })
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{ListenerRegistration, ListenerRegistry, map_input_items};
    use code_protocol::mcp_protocol::InputItem;
    use crate::outgoing_message::ConnectionId;
    use tokio::sync::oneshot;
    use uuid::Uuid;

    #[test]
    fn maps_all_supported_wire_input_variants() {
        let mapped = map_input_items(vec![
            InputItem::Text {
                text: "hello".to_string(),
            },
            InputItem::Image {
                image_url: "data:image/png;base64,abc".to_string(),
            },
            InputItem::LocalImage {
                path: "/tmp/image.png".into(),
            },
        ]);

        assert_eq!(mapped.len(), 3);
        assert!(matches!(mapped[0], code_core::protocol::InputItem::Text { .. }));
        assert!(matches!(mapped[1], code_core::protocol::InputItem::Image { .. }));
        assert!(matches!(mapped[2], code_core::protocol::InputItem::LocalImage { .. }));
    }

    #[test]
    fn takes_only_listeners_owned_by_a_connection() {
        let mut registry = ListenerRegistry::new();
        let (cancel_a, _receiver_a) = oneshot::channel();
        let (cancel_b, _receiver_b) = oneshot::channel();
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        registry.insert(
            id_a,
            ListenerRegistration {
                owner_connection_id: ConnectionId(1),
                cancel_tx: cancel_a,
            },
        );
        registry.insert(
            id_b,
            ListenerRegistration {
                owner_connection_id: ConnectionId(2),
                cancel_tx: cancel_b,
            },
        );

        let removed = registry.take_owned(ConnectionId(1));
        assert_eq!(removed.len(), 1);
        assert!(!registry.contains_key(&id_a));
        assert!(registry.contains_key(&id_b));
    }
}
