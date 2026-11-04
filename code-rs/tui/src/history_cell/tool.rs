//! Tool call history cells driven by structured argument/result state.

use super::*;
use crate::history::state::{
    ArgumentValue,
    HistoryId,
    RunningToolState,
    ToolArgument,
    ToolCallState,
    ToolResultPreview,
    ToolStatus as HistoryToolStatus,
};
use crate::sanitize::{sanitize_for_tui, Mode as SanitizeMode, Options as SanitizeOptions};
use crate::text_formatting::format_json_compact;
use regex_lite::Regex;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

const MAX_COMPACT_ARGUMENTS: usize = 1;
const MAX_COMPACT_RESULT_LINES: usize = 3;
const MAX_COMPACT_INPUT_CHARS: usize = 512;
const MAX_COMPACT_TEXT_CHARS: usize = 160;

pub(crate) struct ToolCallCell {
    state: ToolCallState,
}

impl ToolCallCell {
    pub(crate) fn new(state: ToolCallState) -> Self {
        let mut state = state;
        state.id = HistoryId::ZERO;
        Self { state }
    }

    #[allow(dead_code)]
    pub(crate) fn from_state(state: ToolCallState) -> Self {
        Self { state }
    }

    pub(crate) fn state(&self) -> &ToolCallState {
        &self.state
    }

    pub(crate) fn state_mut(&mut self) -> &mut ToolCallState {
        &mut self.state
    }

    pub(crate) fn retint(&mut self, _old: &crate::theme::Theme, _new: &crate::theme::Theme) {}

    fn header_line(&self) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut style = Style::default().add_modifier(Modifier::BOLD);
        style = match self.state.status {
            HistoryToolStatus::Running => style.fg(crate::colors::info()),
            HistoryToolStatus::Success => style.fg(crate::colors::success()),
            HistoryToolStatus::Failed => style.fg(crate::colors::error()),
        };
        spans.push(Span::styled(self.state.title.clone(), style));
        if let Some(duration) = self.state.duration {
            spans.push(Span::styled(
                format!(", duration: {}", format_duration(duration)),
                Style::default().fg(crate::colors::text_dim()),
            ));
        }
        Line::from(spans)
    }
}

impl HistoryCell for ToolCallCell {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn kind(&self) -> HistoryCellType {
        HistoryCellType::Tool {
            status: super::ToolCellStatus::from(self.state.status),
        }
    }

    fn display_lines(&self) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(self.header_line());
        lines.extend(render_compact_arguments(&self.state.arguments));

        if let Some(result) = &self.state.result_preview
            && !result.lines.is_empty() {
                lines.extend(render_compact_result_preview(result));
                if result.truncated {
                    lines.push(Line::styled(
                        "… truncated ",
                        Style::default().fg(crate::colors::text_dim()),
                    ));
                }
            }

        if let Some(error) = &self.state.error_message
            && !error.is_empty() {
                lines.push(Line::styled(
                    compact_text(error),
                    Style::default().fg(crate::colors::error()),
                ));
            }

        lines
    }
}

pub(crate) struct RunningToolCallCell {
    state: RunningToolState,
    start_clock: Instant,
}

impl RunningToolCallCell {
    pub(crate) fn new(state: RunningToolState) -> Self {
        let mut state = state;
        state.id = HistoryId::ZERO;
        Self {
            state,
            start_clock: Instant::now(),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_state(state: RunningToolState) -> Self {
        Self {
            state,
            start_clock: Instant::now(),
        }
    }

    pub(crate) fn state(&self) -> &RunningToolState {
        &self.state
    }

    pub(crate) fn state_mut(&mut self) -> &mut RunningToolState {
        &mut self.state
    }

    #[cfg(any(test, feature = "test-helpers"))]
    pub(crate) fn override_elapsed_for_testing(&mut self, duration: Duration) {
        if let Some(adjusted) = SystemTime::now().checked_sub(duration) {
            self.state.started_at = adjusted;
        } else {
            self.state.started_at = SystemTime::UNIX_EPOCH;
        }
        self.start_clock = Instant::now();
    }

    fn strip_zero_seconds_suffix(mut duration: String) -> String {
        if duration.ends_with(" 00s") {
            duration.truncate(duration.len().saturating_sub(4));
        }
        duration
    }

    fn compact_duration(duration: Duration) -> String {
        Self::strip_zero_seconds_suffix(format_duration(duration)).replace(' ', "")
    }

    fn spinner_frame(&self) -> &'static str {
        const FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
        let idx = ((self.start_clock.elapsed().as_millis() / 100) as usize) % FRAMES.len();
        FRAMES[idx]
    }

    fn is_gh_run_wait(&self) -> bool {
        self.state.title == "Gh Run Wait..."
    }

    fn tool_argument_text(&self, name: &str) -> Option<String> {
        self.state
            .arguments
            .iter()
            .find(|arg| arg.name == name)
            .and_then(|arg| match &arg.value {
                ArgumentValue::Text(text) => Some(text.clone()),
                ArgumentValue::Json(json) => {
                    let raw = json.to_string();
                    Some(format_json_compact(&raw).unwrap_or(raw))
                }
                ArgumentValue::Secret => None,
            })
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }

    fn tool_argument_json(&self, name: &str) -> Option<Value> {
        self.state
            .arguments
            .iter()
            .find(|arg| arg.name == name)
            .and_then(|arg| match &arg.value {
                ArgumentValue::Json(json) => Some(json.clone()),
                _ => None,
            })
    }

    fn progress_bar(completed: usize, total: usize, width: usize) -> String {
        if total == 0 {
            return "[----------------]".to_string();
        }
        let clamped_width = width.max(1);
        let filled = (completed.saturating_mul(clamped_width)).saturating_add(total - 1) / total;
        let mut bar = String::with_capacity(clamped_width + 2);
        bar.push('[');
        for idx in 0..clamped_width {
            if idx < filled {
                bar.push('=');
            } else {
                bar.push('-');
            }
        }
        bar.push(']');
        bar
    }

    fn format_job_list(names: &[String], max_items: usize) -> String {
        if names.is_empty() {
            return String::new();
        }
        let shown = names.iter().take(max_items).cloned().collect::<Vec<_>>();
        let mut text = shown.join(", ");
        if names.len() > max_items {
            let remaining = names.len() - max_items;
            text.push_str(&format!(" +{remaining} more"));
        }
        text
    }

    fn render_gh_run_wait(&self, elapsed: Duration) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::styled(
            "Monitoring GitHub Workflow",
            Style::default()
                .fg(crate::colors::info())
                .add_modifier(Modifier::BOLD),
        ));

        let dim = Style::default().fg(crate::colors::text_dim());
        let text = Style::default().fg(crate::colors::text());
        if let Some(url) = self.tool_argument_text("url") {
            lines.push(Line::from(vec![
                Span::styled("│ ", dim),
                Span::styled("url ", dim),
                Span::styled(url, text),
            ]));
        }
        if let Some(branch) = self.tool_argument_text("branch") {
            lines.push(Line::from(vec![
                Span::styled("│ ", dim),
                Span::styled("branch ", dim),
                Span::styled(branch, text),
            ]));
        }
        if let Some(run_id) = self.tool_argument_text("run_id") {
            lines.push(Line::from(vec![
                Span::styled("│ ", dim),
                Span::styled("run ", dim),
                Span::styled(run_id, text),
            ]));
        }
        if let Some(workflow) = self.tool_argument_text("workflow") {
            lines.push(Line::from(vec![
                Span::styled("│ ", dim),
                Span::styled("workflow ", dim),
                Span::styled(workflow, text),
            ]));
        }

        if let Some(jobs) = self.tool_argument_json("jobs") {
            let total = jobs.get("total").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
            let completed = jobs
                .get("completed")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let in_progress = jobs
                .get("in_progress")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let queued = jobs.get("queued").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
            let steps_total = jobs
                .get("steps_total")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let steps_completed = jobs
                .get("steps_completed")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let (progress_completed, progress_total, progress_label) = if steps_total > 0 {
                (steps_completed, steps_total, "progress (steps)")
            } else {
                (completed, total, "progress (jobs)")
            };
            let progress = Self::progress_bar(progress_completed, progress_total, 16);
            if progress_total > 0 {
                let percent = (progress_completed.saturating_mul(100)) / progress_total.max(1);
                lines.push(Line::from(vec![
                    Span::styled("│ ", dim),
                    Span::styled(format!("{progress_label} "), dim),
                    Span::styled(
                        format!(
                            "{progress} {progress_completed}/{progress_total} ({percent}%)"
                        ),
                        text,
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("│ ", dim),
                    Span::styled("jobs ", dim),
                    Span::styled(
                        format!("{completed} completed • {in_progress} running • {queued} queued"),
                        text,
                    ),
                ]));
            }
            let running_names = jobs
                .get("running")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(std::string::ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let queued_names = jobs
                .get("queued_names")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(std::string::ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !running_names.is_empty() {
                let list = Self::format_job_list(&running_names, 4);
                lines.push(Line::from(vec![
                    Span::styled("│ ", dim),
                    Span::styled("running ", dim),
                    Span::styled(list, text),
                ]));
            }
            if !queued_names.is_empty() {
                let list = Self::format_job_list(&queued_names, 4);
                lines.push(Line::from(vec![
                    Span::styled("│ ", dim),
                    Span::styled("queued ", dim),
                    Span::styled(list, text),
                ]));
            }
        }

        let elapsed_str = Self::compact_duration(elapsed);
        lines.push(Line::from(vec![
            Span::styled("└ ", dim),
            Span::styled("Waiting for ", dim),
            Span::styled(elapsed_str, text),
        ]));
        lines.push(Line::from(""));
        lines
    }

    pub(crate) fn has_title(&self, title: &str) -> bool {
        self.state.title == title
    }

    fn elapsed_duration(&self) -> Duration {
        SystemTime::now()
            .duration_since(self.state.started_at)
            .unwrap_or_else(|_| self.start_clock.elapsed())
    }
}

impl HistoryCell for RunningToolCallCell {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn kind(&self) -> HistoryCellType {
        HistoryCellType::Tool {
            status: super::ToolCellStatus::Running,
        }
    }

    fn gutter_symbol(&self) -> Option<&'static str> {
        if self.state.title == "Waiting" {
            if self.state.wait_has_call_id {
                None
            } else {
                Some(self.spinner_frame())
            }
        } else {
            Some("⚙")
        }
    }

    fn is_animating(&self) -> bool {
        true
    }

    fn display_lines(&self) -> Vec<Line<'static>> {
        let elapsed = self.elapsed_duration();
        let mut lines: Vec<Line<'static>> = Vec::new();
        if self.state.title == "Waiting" {
            let show_elapsed = !self.state.wait_has_target;
            let mut spans = Vec::new();
            spans.push(
                Span::styled(
                    "Waiting...",
                    Style::default()
                        .fg(crate::colors::text())
                        .add_modifier(Modifier::BOLD),
                ),
            );
            let cap_ms = self.state.wait_cap_ms.unwrap_or(600_000);
            let cap_str = Self::strip_zero_seconds_suffix(
                format_duration(Duration::from_millis(cap_ms)),
            );
            let suffix = if show_elapsed {
                let elapsed_str = Self::strip_zero_seconds_suffix(format_duration(elapsed));
                format!(" ({elapsed_str} / up to {cap_str})")
            } else {
                format!(" (up to {cap_str})")
            };
            spans.push(Span::styled(
                suffix,
                Style::default().fg(crate::colors::text_dim()),
            ));
            lines.push(Line::from(spans));
        } else if self.is_gh_run_wait() {
            return self.render_gh_run_wait(elapsed);
        } else {
            lines.push(Line::styled(
                format!("{} ({})", self.state.title, format_duration(elapsed)),
                Style::default()
                    .fg(crate::colors::info())
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.extend(render_compact_arguments(&self.state.arguments));
        lines
    }
}

fn render_compact_arguments(arguments: &[ToolArgument]) -> Vec<Line<'static>> {
    let visible: Vec<&ToolArgument> = arguments
        .iter()
        .filter(|argument| !matches!(argument.value, ArgumentValue::Secret))
        .take(MAX_COMPACT_ARGUMENTS)
        .collect();
    let mut lines = visible
        .iter()
        .map(|argument| render_compact_argument(argument))
        .collect::<Vec<_>>();
    let hidden_count = arguments.len().saturating_sub(visible.len());
    if hidden_count > 0 {
        let noun = if hidden_count == 1 { "argument" } else { "arguments" };
        lines.push(Line::styled(
            format!("└ +{hidden_count} more {noun}"),
            Style::default().fg(crate::colors::text_dim()),
        ));
    }
    lines
}

fn render_compact_argument(arg: &ToolArgument) -> Line<'static> {
    let dim_style = Style::default().fg(crate::colors::text_dim());
    let mut spans = vec![Span::styled("└ ", dim_style)];
    spans.push(Span::styled(
        format!("{}: ", arg.name),
        dim_style,
    ));
    let value_span = match &arg.value {
        ArgumentValue::Text(text) => Span::styled(compact_text(text), Style::default().fg(crate::colors::text())),
        ArgumentValue::Json(json) => Span::styled(
            compact_json_summary(json),
            Style::default().fg(crate::colors::text()),
        ),
        ArgumentValue::Secret => Span::styled("(secret)".to_string(), Style::default().fg(crate::colors::text_dim())),
    };
    spans.push(value_span);
    Line::from(spans)
}

fn render_compact_result_preview(preview: &ToolResultPreview) -> Vec<Line<'static>> {
    let lines = preview
        .lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .take(MAX_COMPACT_RESULT_LINES.saturating_add(1))
        .map(|line| compact_text(line))
        .collect::<Vec<_>>();
    if lines.len() <= MAX_COMPACT_RESULT_LINES {
        return lines
            .into_iter()
            .map(|line| Line::styled(line, Style::default().fg(crate::colors::text_dim())))
            .collect();
    }

    let mut compact = lines
        .iter()
        .take(2)
        .cloned()
        .map(|line| Line::styled(line, Style::default().fg(crate::colors::text_dim())))
        .collect::<Vec<_>>();
    compact.push(Line::styled(
        "⋮ more output".to_string(),
        Style::default().fg(crate::colors::text_dim()),
    ));
    compact
}

fn compact_json_summary(value: &Value) -> String {
    match value {
        Value::Object(fields) => format!("object ({} fields)", fields.len()),
        Value::Array(items) => format!("array ({} items)", items.len()),
        Value::String(text) => compact_text(text),
        primitive => compact_text(&primitive.to_string()),
    }
}

fn compact_text(text: &str) -> String {
    let (bounded, input_truncated) = take_prefix_chars(text, MAX_COMPACT_INPUT_CHARS);
    let sanitized = sanitize_for_tui(
        bounded,
        SanitizeMode::Plain,
        SanitizeOptions {
            expand_tabs: true,
            tabstop: 4,
            debug_markers: false,
        },
    );
    let redacted = redact_sensitive_values(&sanitized);
    let single_line = redacted.split_whitespace().collect::<Vec<_>>().join(" ");
    let (compact, display_truncated) = take_prefix_chars(&single_line, MAX_COMPACT_TEXT_CHARS);
    let mut compact = compact.to_string();
    if input_truncated || display_truncated {
        compact.push('…');
    }
    compact
}

pub(crate) fn is_sensitive_tool_argument_name(name: &str) -> bool {
    let normalized = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "apikey"
            | "accesstoken"
            | "refreshtoken"
            | "bearertoken"
            | "token"
            | "password"
            | "secret"
            | "clientsecret"
            | "authorization"
            | "credential"
            | "credentials"
    )
}

fn redact_sensitive_values(text: &str) -> String {
    static SENSITIVE_VALUE: OnceLock<Regex> = OnceLock::new();
    let pattern = SENSITIVE_VALUE.get_or_init(|| {
        Regex::new(
            r#"(?i)([\"']?(?:api[_-]?key|access[_-]?token|refresh[_-]?token|bearer[_-]?token|client[_-]?secret|token|password|secret|authorization|credentials?)[\"']?\s*[:=]\s*)(?:\"[^\"]*\"|'[^']*'|(?:bearer\s+)?[^\s,}\]]+)"#,
        )
        .expect("valid sensitive value regex")
    });
    pattern.replace_all(text, "${1}<redacted>").into_owned()
}

fn take_prefix_chars(text: &str, limit: usize) -> (&str, bool) {
    if let Some((index, _)) = text.char_indices().nth(limit) {
        (&text[..index], true)
    } else {
        (text, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::compat::ToolResultPreview;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn completed_tool_call_compacts_arguments_and_preview() {
        let cell = ToolCallCell::new(ToolCallState {
            id: HistoryId::ZERO,
            call_id: None,
            status: HistoryToolStatus::Success,
            title: "Complete".to_string(),
            duration: Some(Duration::from_secs(1)),
            arguments: vec![
                ToolArgument {
                    name: "invocation".to_string(),
                    value: ArgumentValue::Text("search({\"query\":\"ratatui\"})".to_string()),
                },
                ToolArgument {
                    name: "directory".to_string(),
                    value: ArgumentValue::Text("/very/long/path".to_string()),
                },
                ToolArgument {
                    name: "token".to_string(),
                    value: ArgumentValue::Secret,
                },
            ],
            result_preview: Some(ToolResultPreview {
                lines: vec![
                    "first result".to_string(),
                    "second result".to_string(),
                    "third result".to_string(),
                    "fourth result".to_string(),
                ],
                truncated: false,
            }),
            error_message: None,
        });

        let rendered = cell
            .display_lines_trimmed()
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();

        assert!(rendered.iter().any(|line| line.contains("invocation:")));
        assert!(rendered.iter().any(|line| line.contains("+2 more arguments")));
        assert!(rendered.iter().any(|line| line.contains("more output")));
        assert!(!rendered.iter().any(|line| line.contains("token:")));
        assert!(rendered.len() <= 7, "compact tool output was too tall: {rendered:?}");
    }

    #[test]
    fn completed_tool_call_sanitizes_compact_content() {
        let cell = ToolCallCell::new(ToolCallState {
            id: HistoryId::ZERO,
            call_id: None,
            status: HistoryToolStatus::Failed,
            title: "Error".to_string(),
            duration: None,
            arguments: vec![ToolArgument {
                name: "query".to_string(),
                value: ArgumentValue::Text("safe\u{1b}]8;;https://example.test\u{1b}\\label".to_string()),
            }],
            result_preview: Some(ToolResultPreview {
                lines: vec!["result\u{1b}[31m red".to_string()],
                truncated: false,
            }),
            error_message: Some("failed\u{1b}[2J message".to_string()),
        });

        let rendered = cell
            .display_lines_trimmed()
            .iter()
            .map(line_text)
            .collect::<String>();

        assert!(!rendered.contains('\u{1b}'));
        assert!(rendered.contains("label"));
        assert!(rendered.contains("failed message"));
    }

    #[test]
    fn completed_tool_call_redacts_credential_shaped_text() {
        let secret = "super-secret-token";
        let cell = ToolCallCell::new(ToolCallState {
            id: HistoryId::ZERO,
            call_id: None,
            status: HistoryToolStatus::Success,
            title: "Complete".to_string(),
            duration: None,
            arguments: vec![ToolArgument {
                name: "invocation".to_string(),
                value: ArgumentValue::Text(format!(
                    "tool({{\"credentials\":{{\"api_key\":\"{secret}\"}}}})"
                )),
            }],
            result_preview: Some(ToolResultPreview {
                lines: vec![format!("{{\"access_token\":\"{secret}\"}}")],
                truncated: false,
            }),
            error_message: Some(format!("authorization: Bearer {secret}")),
        });

        let rendered = cell
            .display_lines_trimmed()
            .iter()
            .map(line_text)
            .collect::<String>();

        assert!(!rendered.contains(secret), "credential leaked: {rendered}");
        assert!(rendered.matches("<redacted>").count() >= 3, "{rendered}");
    }
}
