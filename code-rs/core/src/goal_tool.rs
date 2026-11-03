use crate::codex::{Session, ToolCallCtx};
use crate::openai_tools::{JsonSchema, OpenAiTool, ResponsesApiTool};
use code_protocol::models::{FunctionCallOutputPayload, ResponseInputItem};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GoalRecord { objective: String, status: String, #[serde(default)] token_budget: Option<i64>, #[serde(default)] blocker: Option<String>, #[serde(default)] blocker_count: u8 }

fn save(path: &Path, record: &GoalRecord) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().expect("goal path parent"))?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec(record).map_err(std::io::Error::other)?)?;
    std::fs::rename(temporary, path)
}

fn tool(name: &str, description: &str, properties: BTreeMap<String, JsonSchema>, required: Vec<String>) -> OpenAiTool {
    OpenAiTool::Function(ResponsesApiTool { name: name.to_string(), description: description.to_string(), strict: false, parameters: JsonSchema::Object { properties, required: Some(required), additional_properties: Some(false.into()) } })
}

pub(crate) fn tools() -> [OpenAiTool; 3] {
    let objective = JsonSchema::String { description: Some("Concrete objective for this saved session.".to_string()), allowed_values: None };
    let budget = JsonSchema::Number { description: Some("Optional token budget; set only when the user explicitly requested one.".to_string()) };
    let mut create = BTreeMap::new(); create.insert("objective".to_string(), objective); create.insert("token_budget".to_string(), budget);
    let mut update = BTreeMap::new(); update.insert("status".to_string(), JsonSchema::String { description: Some("Only complete or blocked. Complete only after all requirements are verified; blocked only after the same blocker recurs for three turns.".to_string()), allowed_values: Some(vec!["complete".to_string(), "blocked".to_string()]) });
    [tool("create_goal", "Create one durable goal for this session. Fails when an unfinished goal exists.", create, vec!["objective".to_string()]), tool("get_goal", "Read the current durable goal for this session.", BTreeMap::new(), vec![]), tool("update_goal", "Mark the current goal complete or blocked when its lifecycle rules are satisfied.", update, vec!["status".to_string()])]
}

pub(crate) async fn handle(session: &Session, ctx: &ToolCallCtx, name: &str, arguments: String) -> ResponseInputItem {
    let path = session.client().code_home().join("goals").join(format!("{}.json", session.id()));
    let result: Result<String, std::io::Error> = (|| match name {
        "get_goal" => std::fs::read(&path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok("null".to_string()) } else { Err(e) }),
        "create_goal" => {
            let input: GoalRecord = serde_json::from_str(&arguments).map_err(std::io::Error::other)?;
            if input.objective.trim().is_empty() { Err(std::io::Error::other("objective must not be empty")) } else if std::fs::read(&path).is_ok() { Err(std::io::Error::other("an unfinished goal already exists")) } else { let record = GoalRecord { objective: input.objective.trim().to_string(), status: "active".to_string(), token_budget: input.token_budget, blocker: None, blocker_count: 0 }; save(&path, &record)?; Ok("goal created".to_string()) }
        }
        "update_goal" => {
            let update: serde_json::Value = serde_json::from_str(&arguments).map_err(std::io::Error::other)?;
            let status = update.get("status").and_then(serde_json::Value::as_str).ok_or_else(|| std::io::Error::other("status is required"))?;
            if !matches!(status, "complete" | "blocked") { return Err(std::io::Error::other("status must be complete or blocked")); }
            let bytes = std::fs::read(&path)?; let mut record: GoalRecord = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
            if record.status != "active" { Err(std::io::Error::other("only an active goal may be updated")) } else if status == "complete" && update.get("evidence").and_then(serde_json::Value::as_str).is_none_or(str::is_empty) { Err(std::io::Error::other("completion requires verification evidence")) } else if status == "blocked" { let blocker = update.get("blocker").and_then(serde_json::Value::as_str).filter(|value| !value.is_empty()).ok_or_else(|| std::io::Error::other("blocked requires a blocker description"))?; if record.blocker.as_deref() == Some(blocker) { record.blocker_count = record.blocker_count.saturating_add(1); } else { record.blocker = Some(blocker.to_string()); record.blocker_count = 1; } save(&path, &record)?; if record.blocker_count < 3 { Err(std::io::Error::other("the same blocker must recur for three turns before marking blocked")) } else { record.status = "blocked".to_string(); save(&path, &record)?; Ok("goal marked blocked".to_string()) } } else { record.status = status.to_string(); save(&path, &record)?; Ok(format!("goal marked {status}")) }
        }
        _ => Err(std::io::Error::other("unsupported goal tool")),
    })();
    let output = match result { Ok(text) => FunctionCallOutputPayload { body: code_protocol::models::FunctionCallOutputBody::Text(text), success: Some(true) }, Err(error) => FunctionCallOutputPayload { body: code_protocol::models::FunctionCallOutputBody::Text(format!("goal error: {error}")), success: Some(false) } };
    ResponseInputItem::FunctionCallOutput { call_id: ctx.call_id.clone(), output }
}
