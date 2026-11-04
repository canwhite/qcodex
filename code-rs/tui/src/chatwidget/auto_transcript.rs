//! Conversation-compaction projection for Auto Drive history.
//!
//! The widget owns rendering and history-cell conversion. This module owns the
//! stable correspondence between a compacted coordinator transcript and the
//! on-screen history tail that can still be projected after compaction.

use code_protocol::models::ResponseItem;

#[derive(Clone)]
pub(crate) struct CompactionOverlay {
    pub(crate) prefix_items: Vec<ResponseItem>,
    pub(crate) tail_start_cell: usize,
}

pub(crate) fn derive_compaction_overlay(
    current_overlay: Option<&CompactionOverlay>,
    history_len: usize,
    previous_items: &[ResponseItem],
    previous_indices: &[Option<usize>],
    new_items: &[ResponseItem],
) -> Option<CompactionOverlay> {
    if previous_items == new_items {
        return current_overlay.cloned();
    }

    if new_items.is_empty() {
        return Some(CompactionOverlay {
            prefix_items: Vec::new(),
            tail_start_cell: history_len,
        });
    }

    let max_prefix = previous_items.len().min(new_items.len());
    let mut prefix_len = 0;
    while prefix_len < max_prefix && previous_items[prefix_len] == new_items[prefix_len] {
        prefix_len += 1;
    }

    let remaining_prev = previous_items.len().saturating_sub(prefix_len);
    let remaining_new = new_items.len().saturating_sub(prefix_len);
    let mut suffix_len = 0;
    while suffix_len < remaining_prev && suffix_len < remaining_new {
        let prev_idx = previous_items.len() - 1 - suffix_len;
        let new_idx = new_items.len() - 1 - suffix_len;
        if previous_items[prev_idx] != new_items[new_idx] {
            break;
        }
        suffix_len += 1;
    }

    let mut prefix_items_end = new_items.len().saturating_sub(suffix_len);
    let tail_start_cell = if suffix_len == 0 {
        history_len
    } else {
        let start = previous_items.len() - suffix_len;
        previous_indices[start..]
            .iter()
            .find_map(|idx| *idx)
            .unwrap_or(history_len)
    };

    if suffix_len > 0 && tail_start_cell == history_len {
        // Suffix items no longer map to visible cells after an earlier
        // compaction, so retain the complete compacted conversation as prefix.
        prefix_items_end = new_items.len();
    }

    Some(CompactionOverlay {
        prefix_items: new_items[..prefix_items_end].to_vec(),
        tail_start_cell,
    })
}

#[cfg(test)]
mod tests {
    use super::derive_compaction_overlay;
    use code_protocol::models::{ContentItem, ResponseItem};

    fn item(text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: text.to_string(),
            }],
            end_turn: None,
            phase: None,
        }
    }

    #[test]
    fn retains_the_visible_suffix_after_compaction() {
        let previous = vec![item("before"), item("visible one"), item("visible two")];
        let compacted = vec![item("summary"), item("visible one"), item("visible two")];

        let overlay = derive_compaction_overlay(
            None,
            3,
            &previous,
            &[Some(0), Some(1), Some(2)],
            &compacted,
        )
        .expect("compaction changes the overlay");

        assert_eq!(overlay.prefix_items, vec![item("summary")]);
        assert_eq!(overlay.tail_start_cell, 1);
    }

    #[test]
    fn preserves_an_existing_overlay_for_an_unchanged_conversation() {
        let previous = vec![item("summary")];
        let existing = derive_compaction_overlay(None, 0, &[], &[], &previous)
            .expect("initial compaction creates an overlay");

        let unchanged = derive_compaction_overlay(
            Some(&existing),
            0,
            &previous,
            &[None],
            &previous,
        )
        .expect("unchanged compaction retains the overlay");

        assert_eq!(unchanged.prefix_items, existing.prefix_items);
        assert_eq!(unchanged.tail_start_cell, existing.tail_start_cell);
    }
}
