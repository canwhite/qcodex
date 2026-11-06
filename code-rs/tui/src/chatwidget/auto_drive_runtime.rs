//! Pure Auto Drive lifecycle decisions used by the widget boundary.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EscapeDecision {
    PauseForEdit,
    CancelTask,
    StopDuringApproval,
    StopActive,
}

/// Chooses the Auto Drive action for Esc after modal precedence has been
/// resolved by the host widget.
pub(crate) fn decide_escape(
    prompt_visible: bool,
    task_activity_active: bool,
    awaiting_submission: bool,
) -> EscapeDecision {
    if prompt_visible {
        EscapeDecision::PauseForEdit
    } else if task_activity_active {
        EscapeDecision::CancelTask
    } else if awaiting_submission {
        EscapeDecision::StopDuringApproval
    } else {
        EscapeDecision::StopActive
    }
}

#[cfg(test)]
mod tests {
    use super::{EscapeDecision, decide_escape};

    #[test]
    fn escape_prioritizes_edit_then_task_then_approval() {
        assert_eq!(
            decide_escape(true, true, true),
            EscapeDecision::PauseForEdit
        );
        assert_eq!(
            decide_escape(false, true, true),
            EscapeDecision::CancelTask
        );
        assert_eq!(
            decide_escape(false, false, true),
            EscapeDecision::StopDuringApproval
        );
        assert_eq!(
            decide_escape(false, false, false),
            EscapeDecision::StopActive
        );
    }
}
