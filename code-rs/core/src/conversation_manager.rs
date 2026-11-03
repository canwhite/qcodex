use crate::AuthManager;
use crate::CodexAuth;
use crate::codex::Codex;
use crate::codex::CodexSpawnOk;
use crate::codex::INITIAL_SUBMIT_ID;
use crate::code_conversation::CodexConversation;
use crate::config::Config;
use crate::error::CodexErr;
use crate::error::Result as CodexResult;
use crate::protocol::Event;
use crate::protocol::EventMsg;
use crate::protocol::SessionConfiguredEvent;
use crate::rollout::RolloutRecorder;
use code_protocol::ConversationId;
use code_protocol::protocol::SessionSource;
use code_protocol::models::DeveloperInstructions;
use code_protocol::models::ResponseItem;
use code_protocol::protocol::InitialHistory;
use code_protocol::protocol::RolloutItem;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const EPHEMERAL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(4);

/// Represents a newly created Codex conversation, including the first event
/// (which is [`EventMsg::SessionConfigured`]).
pub struct NewConversation {
    pub conversation_id: ConversationId,
    pub conversation: Arc<CodexConversation>,
    pub session_configured: SessionConfiguredEvent,
}

/// Opaque ownership proof for cleaning up a temporary side conversation.
pub struct EphemeralConversationCleanup {
    conversation_id: ConversationId,
    rollout_path: Option<PathBuf>,
}

/// A fork created for a temporary side conversation. Its cleanup token must be
/// returned to [`ConversationManager::dispose_ephemeral_conversation`].
pub struct EphemeralConversation {
    pub conversation: NewConversation,
    pub cleanup: EphemeralConversationCleanup,
    pub captured_sequence: u64,
    pub boundary_version: u32,
}

const SIDE_BOUNDARY_VERSION: u32 = 1;

struct EphemeralRolloutGuard {
    path: Option<PathBuf>,
}

impl EphemeralRolloutGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for EphemeralRolloutGuard {
    fn drop(&mut self) {
        let Some(path) = self.path.take() else {
            return;
        };
        remove_ephemeral_rollout_sync(&path);
    }
}

/// [`ConversationManager`] is responsible for creating conversations and
/// maintaining them in memory.
pub struct ConversationManager {
    conversations: Arc<RwLock<HashMap<ConversationId, Arc<CodexConversation>>>>,
    auth_manager: Arc<AuthManager>,
    session_source: SessionSource,
}

impl ConversationManager {
    pub fn new(auth_manager: Arc<AuthManager>, session_source: SessionSource) -> Self {
        Self {
            conversations: Arc::new(RwLock::new(HashMap::new())),
            auth_manager,
            session_source,
        }
    }

    pub fn auth_manager(&self) -> Arc<AuthManager> {
        Arc::clone(&self.auth_manager)
    }

    /// Construct with a dummy AuthManager containing the provided CodexAuth.
    /// Used for integration tests: should not be used by ordinary business logic.
    pub fn with_auth(auth: CodexAuth) -> Self {
        Self::new(
            crate::AuthManager::from_auth_for_testing(auth),
            SessionSource::Exec,
        )
    }

    pub async fn new_conversation(&self, config: Config) -> CodexResult<NewConversation> {
        self.spawn_conversation(config, self.auth_manager.clone())
            .await
    }

    async fn spawn_conversation(
        &self,
        mut config: Config,
        auth_manager: Arc<AuthManager>,
    ) -> CodexResult<NewConversation> {
        // Our core `Codex::spawn_with_auth_manager` handles resume via
        // `config.experimental_resume`. For a fresh conversation we leave it as `None`.
        config.experimental_resume = None;
        let CodexSpawnOk {
            codex,
            init_id: _,
            session_id,
        } = Codex::spawn_with_auth_manager(config, Some(auth_manager.clone())).await?;
        let conversation_id: code_protocol::ConversationId = session_id.into();
        self.finalize_spawn(codex, conversation_id).await
    }

    async fn finalize_spawn(
        &self,
        codex: Codex,
        conversation_id: ConversationId,
    ) -> CodexResult<NewConversation> {
        // The first event must be `SessionInitialized`. Validate and forward it
        // to the caller so that they can display it in the conversation
        // history.
        let event = codex.next_event().await?;
        let session_configured = match event {
            Event {
                id,
                msg: EventMsg::SessionConfigured(session_configured),
                ..
            } if id == INITIAL_SUBMIT_ID => session_configured,
            _ => {
                return Err(CodexErr::SessionConfiguredNotFirstEvent);
            }
        };

        let conversation = Arc::new(CodexConversation::new(codex));
        self.conversations
            .write()
            .await
            .insert(conversation_id, conversation.clone());

        Ok(NewConversation {
            conversation_id,
            conversation,
            session_configured,
        })
    }

    pub async fn get_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> CodexResult<Arc<CodexConversation>> {
        let conversations = self.conversations.read().await;
        conversations
            .get(&conversation_id)
            .cloned()
            .ok_or_else(|| CodexErr::ConversationNotFound(conversation_id.into()))
    }

    pub async fn resume_conversation_from_rollout(
        &self,
        mut config: Config,
        rollout_path: PathBuf,
        auth_manager: Arc<AuthManager>,
    ) -> CodexResult<NewConversation> {
        // Point the config at the rollout we want to resume.
        config.experimental_resume = Some(rollout_path);
        let CodexSpawnOk {
            codex,
            init_id: _,
            session_id,
        } = Codex::spawn_with_auth_manager(config, Some(auth_manager.clone())).await?;
        let conversation_id: code_protocol::ConversationId = session_id.into();
        self.finalize_spawn(codex, conversation_id).await
    }

    /// Removes the conversation from the manager's internal map, though the
    /// conversation is stored as `Arc<CodexConversation>`, it is possible that
    /// other references to it exist elsewhere. Returns the conversation if the
    /// conversation was found and removed.
    pub async fn remove_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Option<Arc<CodexConversation>> {
        self.conversations.write().await.remove(conversation_id)
    }

    /// Fork an existing conversation by dropping the last `drop_last_messages`
    /// user/assistant messages from its transcript and starting a new
    /// conversation with identical configuration (unless overridden by the
    /// caller's `config`). The new conversation will have a fresh id.
    pub async fn fork_conversation(
        &self,
        num_messages_to_drop: usize,
        config: Config,
        path: PathBuf,
        trailing_developer_instructions: Option<&str>,
    ) -> CodexResult<EphemeralConversation> {
        // Compute the prefix up to the cut point.
        let history = RolloutRecorder::get_rollout_history(&path).await?;
        let history = truncate_history_for_fork(history, num_messages_to_drop);
        self.fork_from_history(
            config,
            history,
            trailing_developer_instructions,
            0,
        )
        .await
    }

    /// Fork a currently registered conversation from an atomic in-memory
    /// history snapshot. This avoids racing the rollout recorder on disk.
    pub async fn fork_registered_conversation(
        &self,
        parent_id: ConversationId,
        config: Config,
        trailing_developer_instructions: Option<&str>,
    ) -> CodexResult<EphemeralConversation> {
        let parent = self.get_conversation(parent_id).await?;
        let (items, captured_sequence) = parent
            .fork_history_snapshot()
            .ok_or(CodexErr::ConversationNotFound(parent_id.into()))?;
        let history = live_history_for_fork(items);
        self.fork_from_history(
            config,
            history,
            trailing_developer_instructions,
            captured_sequence,
        )
        .await
    }

    async fn fork_from_history(
        &self,
        mut config: Config,
        history: InitialHistory,
        trailing_developer_instructions: Option<&str>,
        captured_sequence: u64,
    ) -> CodexResult<EphemeralConversation> {

        // If there is no prior history to seed, just start a fresh conversation.
        if matches!(history, InitialHistory::New) {
            let conversation = self.spawn_conversation(config, self.auth_manager.clone()).await?;
            return Ok(EphemeralConversation {
                cleanup: EphemeralConversationCleanup {
                    conversation_id: conversation.conversation_id,
                    rollout_path: None,
                },
                conversation,
                captured_sequence,
                boundary_version: SIDE_BOUNDARY_VERSION,
            });
        }

        // Otherwise, create a temporary rollout with the truncated items and resume from it.
        let convo_id = code_protocol::ConversationId::new();
        let instructions = config.user_instructions.clone();
        let mut rollout_guard: EphemeralRolloutGuard;
        let recorder = RolloutRecorder::new(
            &config,
            crate::rollout::recorder::RolloutRecorderParams::new(
                convo_id,
                instructions,
                self.session_source.clone(),
            ),
        )
        .await
        .map_err(CodexErr::Io)?;
        let rollout_path = recorder.rollout_path.clone();
        rollout_guard = EphemeralRolloutGuard::new(rollout_path.clone());

        // Persist rollout items to seed the resumed conversation.
        let rollout_items =
            rollout_items_for_fork(history, trailing_developer_instructions);
        if !rollout_items.is_empty() {
            recorder
                .record_items(&rollout_items)
                .await
                .map_err(CodexErr::Io)?;
        }
        // Ensure data is flushed to disk before resuming.
        recorder.shutdown().await.map_err(CodexErr::Io)?;

        // Now spawn a conversation resuming from the newly created rollout.
        config.experimental_resume = Some(rollout_path.clone());
        let CodexSpawnOk {
            codex,
            init_id: _,
            session_id,
        } = match Codex::spawn_with_auth_manager(config, Some(self.auth_manager.clone())).await {
            Ok(spawned) => spawned,
            Err(error) => {
                rollout_guard.disarm();
                remove_ephemeral_rollout(&rollout_path).await;
                return Err(error);
            }
        };
        let conversation_id: code_protocol::ConversationId = session_id.into();
        let conversation = match self.finalize_spawn(codex, conversation_id).await {
            Ok(conversation) => conversation,
            Err(error) => {
                rollout_guard.disarm();
                remove_ephemeral_rollout(&rollout_path).await;
                return Err(error);
            }
        };
        rollout_guard.disarm();
        Ok(EphemeralConversation {
            cleanup: EphemeralConversationCleanup {
                conversation_id,
                rollout_path: Some(rollout_path),
            },
            conversation,
            captured_sequence,
            boundary_version: SIDE_BOUNDARY_VERSION,
        })
    }

    /// Stop and unregister a temporary fork, then remove its rollout files.
    /// This operation is intentionally idempotent so every terminal side path
    /// can call the same cleanup routine.
    pub async fn dispose_ephemeral_conversation(
        &self,
        cleanup: EphemeralConversationCleanup,
    ) {
        if let Some(conversation) = self.remove_conversation(&cleanup.conversation_id).await {
            let _ = conversation.submit(crate::protocol::Op::Interrupt).await;
            let _ = conversation.submit(crate::protocol::Op::Shutdown).await;
            let shutdown_complete = tokio::time::timeout(EPHEMERAL_SHUTDOWN_TIMEOUT, async {
                loop {
                    match conversation.next_event().await {
                        Ok(event) if matches!(event.msg, EventMsg::ShutdownComplete) => break true,
                        Ok(_) => {}
                        Err(_) => break false,
                    }
                }
            })
            .await
            .unwrap_or(false);
            if !shutdown_complete {
                tracing::warn!(
                    conversation_id = %cleanup.conversation_id,
                    "timed out waiting for ephemeral side conversation shutdown"
                );
            }
        }
        if let Some(path) = cleanup.rollout_path {
            remove_ephemeral_rollout(&path).await;
        }
    }
}

async fn remove_ephemeral_rollout(path: &PathBuf) {
    let snapshot = path.with_extension("snapshot.json");
    for path in [path, &snapshot] {
        if let Err(error) = tokio::fs::remove_file(path).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(?path, ?error, "failed to remove ephemeral side rollout");
        }
    }
}

fn remove_ephemeral_rollout_sync(path: &PathBuf) {
    let snapshot = path.with_extension("snapshot.json");
    for path in [path, &snapshot] {
        if let Err(error) = std::fs::remove_file(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(?path, ?error, "failed to remove cancelled side rollout");
        }
    }
}

fn rollout_items_for_fork(
    history: InitialHistory,
    trailing_developer_instructions: Option<&str>,
) -> Vec<RolloutItem> {
    let mut items = history.get_rollout_items();
    if let Some(instructions) = trailing_developer_instructions
        .map(str::trim)
        .filter(|instructions| !instructions.is_empty())
    {
        items.push(RolloutItem::ResponseItem(
            DeveloperInstructions::new(instructions).into(),
        ));
    }
    items
}

fn truncate_history_for_fork(
    history: InitialHistory,
    requested_messages_to_drop: usize,
) -> InitialHistory {
    let unfinished_turn = history.get_rollout_items().iter().rev().find_map(|item| {
        let event = match item {
            RolloutItem::Event(event) => Some(&event.msg),
            RolloutItem::EventMsg(event) => Some(event),
            _ => None,
        };
        match event {
            Some(code_protocol::protocol::EventMsg::TurnStarted(_)) => Some(true),
            Some(code_protocol::protocol::EventMsg::TurnComplete(_)) => Some(false),
            _ => None,
        }
    });
    let messages_to_drop = requested_messages_to_drop.max(usize::from(
        unfinished_turn.unwrap_or(false),
    ));
    truncate_after_dropping_last_messages(history, messages_to_drop)
}

fn live_history_for_fork(items: Vec<ResponseItem>) -> InitialHistory {
    InitialHistory::Forked(
        items
            .into_iter()
            .map(RolloutItem::ResponseItem)
            .collect(),
    )
}

/// Return a prefix of `items` obtained by dropping the last `n` user messages
/// and all items that follow them.
fn truncate_after_dropping_last_messages(history: InitialHistory, n: usize) -> InitialHistory {
    if n == 0 {
        return InitialHistory::Forked(history.get_rollout_items());
    }

    // Work directly on rollout items, and cut the vector at the nth-from-last user message input.
    let items: Vec<RolloutItem> = history.get_rollout_items();

    // Find indices of user message inputs in rollout order.
    let mut user_positions: Vec<usize> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        if let RolloutItem::ResponseItem(ResponseItem::Message { role, .. }) = item
            && role == "user"
        {
            user_positions.push(idx);
        }
    }

    // If fewer than n user messages exist, treat as empty.
    if user_positions.len() < n {
        return InitialHistory::New;
    }

    // Cut strictly before the nth-from-last user message (do not keep the nth itself).
    let cut_idx = user_positions[user_positions.len() - n];
    let rolled: Vec<RolloutItem> = items.into_iter().take(cut_idx).collect();

    if rolled.is_empty() {
        InitialHistory::New
    } else {
        InitialHistory::Forked(rolled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use code_protocol::models::ContentItem;
    use code_protocol::models::ReasoningItemReasoningSummary;
    use code_protocol::models::ResponseItem;
    use code_protocol::protocol::EventMsg;
    use code_protocol::config_types::ModeKind;
    use code_protocol::protocol::RecordedEvent;
    use code_protocol::protocol::TurnCompleteEvent;
    use code_protocol::protocol::TurnStartedEvent;

    fn user_msg(text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::OutputText {
                text: text.to_string(),
            }], end_turn: None, phase: None}
    }
    fn assistant_msg(text: &str) -> ResponseItem {
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: text.to_string(),
            }], end_turn: None, phase: None}
    }

    #[test]
    fn drops_from_last_user_only() {
        let items = vec![
            user_msg("u1"),
            assistant_msg("a1"),
            assistant_msg("a2"),
            user_msg("u2"),
            assistant_msg("a3"),
            ResponseItem::Reasoning {
                id: Some("r1".to_string()),
                summary: vec![ReasoningItemReasoningSummary::SummaryText {
                    text: "s".to_string(),
                }],
                content: None,
                encrypted_content: None,
            },
            ResponseItem::FunctionCall {
                id: None,
                name: "tool".to_string(),
                namespace: None,
                arguments: "{}".to_string(),
                call_id: "c1".to_string(),
            },
            assistant_msg("a4"),
        ];

        // Wrap as InitialHistory::Forked with response items only.
        let initial: Vec<RolloutItem> = items
            .iter()
            .cloned()
            .map(RolloutItem::ResponseItem)
            .collect();
        let truncated = truncate_after_dropping_last_messages(InitialHistory::Forked(initial), 1);
        let got_items = truncated.get_rollout_items();
        let expected_items = vec![
            RolloutItem::ResponseItem(items[0].clone()),
            RolloutItem::ResponseItem(items[1].clone()),
            RolloutItem::ResponseItem(items[2].clone()),
        ];
        assert_eq!(
            serde_json::to_value(&got_items).unwrap(),
            serde_json::to_value(&expected_items).unwrap()
        );

        let initial2: Vec<RolloutItem> = items
            .iter()
            .cloned()
            .map(RolloutItem::ResponseItem)
            .collect();
        let truncated2 = truncate_after_dropping_last_messages(InitialHistory::Forked(initial2), 2);
        assert!(matches!(truncated2, InitialHistory::New));
    }

    #[test]
    fn live_snapshot_preserves_the_exact_atomic_prefix() {
        let history = live_history_for_fork(vec![
                user_msg("completed request"),
                assistant_msg("completed answer"),
            ]);

        let actual = serde_json::to_value(history.get_rollout_items()).expect("serialize history");
        let expected = serde_json::to_value(vec![
            RolloutItem::ResponseItem(user_msg("completed request")),
            RolloutItem::ResponseItem(assistant_msg("completed answer")),
        ])
        .expect("serialize expected history");
        assert_eq!(actual, expected);
    }

    #[test]
    fn side_boundary_follows_inherited_parent_history() {
        let inherited = InitialHistory::Forked(vec![RolloutItem::ResponseItem(user_msg(
            "continue the parent goal",
        ))]);

        let items = rollout_items_for_fork(inherited, Some("parent work is inactive"));

        assert!(matches!(
            items.last(),
            Some(RolloutItem::ResponseItem(ResponseItem::Message { role, content, .. }))
                if role == "developer"
                    && content.iter().any(|item| matches!(
                        item,
                        ContentItem::InputText { text } if text == "parent work is inactive"
                    ))
        ));
    }

    #[test]
    fn active_parent_turn_is_not_inherited_by_side_fork() {
        let history = InitialHistory::Forked(vec![
            RolloutItem::ResponseItem(user_msg("completed parent question")),
            RolloutItem::ResponseItem(assistant_msg("completed parent answer")),
            RolloutItem::ResponseItem(user_msg("unfinished parent goal")),
        ]);

        let history = truncate_after_dropping_last_messages(history, 1);
        let items = rollout_items_for_fork(history, Some("answer only the side request"));

        assert!(!items.iter().any(|item| matches!(
            item,
            RolloutItem::ResponseItem(ResponseItem::Message { content, .. })
                if content.iter().any(|part| matches!(
                    part,
                    ContentItem::InputText { text } | ContentItem::OutputText { text }
                        if text == "unfinished parent goal"
                ))
        )));
        assert!(items.iter().any(|item| matches!(
            item,
            RolloutItem::ResponseItem(ResponseItem::Message { content, .. })
                if content.iter().any(|part| matches!(
                    part,
                    ContentItem::OutputText { text } if text == "completed parent answer"
                ))
        )));
    }

    #[test]
    fn persisted_unfinished_turn_is_not_inherited_when_tui_state_lags() {
        let history = InitialHistory::Forked(vec![
            RolloutItem::ResponseItem(user_msg("completed parent question")),
            RolloutItem::ResponseItem(assistant_msg("completed parent answer")),
            RolloutItem::ResponseItem(user_msg("unfinished parent goal")),
            RolloutItem::Event(RecordedEvent {
                id: "parent-turn".to_string(),
                event_seq: 1,
                order: None,
                msg: EventMsg::TurnStarted(TurnStartedEvent {
                    model_context_window: None,
                    collaboration_mode_kind: ModeKind::Default,
                }),
            }),
            RolloutItem::ResponseItem(assistant_msg("partial parent analysis")),
        ]);

        let history = truncate_history_for_fork(history, 0);
        let items = rollout_items_for_fork(history, Some("answer only the side request"));

        assert!(!items.iter().any(|item| matches!(
            item,
            RolloutItem::ResponseItem(ResponseItem::Message { content, .. })
                if content.iter().any(|part| matches!(
                    part,
                    ContentItem::InputText { text } | ContentItem::OutputText { text }
                        if text == "unfinished parent goal" || text == "partial parent analysis"
                ))
        )));
        assert!(items.iter().any(|item| matches!(
            item,
            RolloutItem::ResponseItem(ResponseItem::Message { content, .. })
                if content.iter().any(|part| matches!(
                    part,
                    ContentItem::OutputText { text } if text == "completed parent answer"
                ))
        )));
    }

    #[test]
    fn completed_persisted_turn_remains_available_to_side_fork() {
        let history = InitialHistory::Forked(vec![
            RolloutItem::ResponseItem(user_msg("completed parent question")),
            RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
                model_context_window: None,
                collaboration_mode_kind: ModeKind::Default,
            })),
            RolloutItem::ResponseItem(assistant_msg("completed parent answer")),
            RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
                last_agent_message: Some("completed parent answer".to_string()),
            })),
        ]);

        let history = truncate_history_for_fork(history, 0);

        assert!(history.get_rollout_items().iter().any(|item| matches!(
            item,
            RolloutItem::ResponseItem(ResponseItem::Message { content, .. })
                if content.iter().any(|part| matches!(
                    part,
                    ContentItem::OutputText { text } if text == "completed parent answer"
                ))
        )));
    }

    #[tokio::test]
    async fn cancelled_side_fork_guard_removes_temporary_rollout() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let rollout_path = directory.path().join("side-rollout.jsonl");
        let snapshot_path = rollout_path.with_extension("snapshot.json");
        tokio::fs::write(&rollout_path, b"rollout")
            .await
            .expect("write rollout fixture");
        tokio::fs::write(&snapshot_path, b"snapshot")
            .await
            .expect("write snapshot fixture");

        drop(EphemeralRolloutGuard::new(rollout_path.clone()));

        tokio::time::timeout(Duration::from_secs(1), async {
            while rollout_path.exists() || snapshot_path.exists() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelled fork artifacts should be removed");
    }
}
