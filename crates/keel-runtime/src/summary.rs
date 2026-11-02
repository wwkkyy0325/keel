//! Tool summaries for history.
//!
//! The current turn keeps full tool output. History keeps a compact record:
//! the call, the status and the head and tail of the output by character
//! count. A long output spills once to `.keel/outputs/` and the marker names
//! the file, so the model can read it again when the summary is not enough.

use std::path::Path;

use keel_core::{redact_secrets, ArgValue, ToolArgs};

/// Characters kept from the start of a long tool output.
pub const TOOL_HEAD_CHARS: usize = 2000;
/// Characters kept from the end of a long tool output.
pub const TOOL_TAIL_CHARS: usize = 1000;
/// Maximum characters used to render tool arguments.
pub const ARG_CLIP_CHARS: usize = 200;

/// Renders tool arguments as one bounded line.
pub fn compact_args(args: &ToolArgs) -> String {
    let mut parts = Vec::new();
    for key in args.keys() {
        if let Some(value) = args.get(key) {
            parts.push(format!("{key}={}", render_value(value)));
        }
    }
    clip(&parts.join(" "), ARG_CLIP_CHARS)
}

/// Summarizes one tool result for history, spilling long output once.
///
/// The marker names the spill file by absolute path, so the model can read it
/// back even when the host keeps outputs outside the workspace.
pub async fn summarize_output(outputs: &Path, turn: u64, call: usize, output: &str) -> String {
    let redacted = redact_secrets(output);
    let chars = redacted.chars().count();
    if chars <= TOOL_HEAD_CHARS + TOOL_TAIL_CHARS {
        return redacted;
    }

    let head: String = redacted.chars().take(TOOL_HEAD_CHARS).collect();
    let tail_skip = chars.saturating_sub(TOOL_TAIL_CHARS);
    let tail: String = redacted.chars().skip(tail_skip).collect();

    let path = outputs.join(format!("turn-{turn}-call-{call}.txt"));
    let marker = match write_spill(&path, &redacted).await {
        Ok(()) => format!("[truncated {chars} chars; full output: {}]", path.display()),
        Err(_) => format!("[truncated {chars} chars; full output unavailable]"),
    };
    format!("{head}\n{marker}\n{tail}")
}

fn render_value(value: &ArgValue) -> String {
    match value {
        ArgValue::String(text) => text.clone(),
        ArgValue::Integer(number) => number.to_string(),
        ArgValue::Boolean(flag) => flag.to_string(),
        ArgValue::StringList(items) => items.join(","),
    }
}

async fn write_spill(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, content).await
}

fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_rendered_and_bounded() {
        let mut args = ToolArgs::new();
        args.insert("path", ArgValue::String("src/lib.rs".to_owned()));
        args.insert("limit", ArgValue::Integer(10));
        args.insert("deep", ArgValue::Boolean(true));

        let rendered = compact_args(&args);
        assert_eq!(rendered, "deep=true limit=10 path=src/lib.rs");

        let mut long = ToolArgs::new();
        long.insert("text", ArgValue::String("x".repeat(500)));
        assert_eq!(compact_args(&long).chars().count(), ARG_CLIP_CHARS + 3);
    }

    #[tokio::test]
    async fn short_output_is_kept_as_is() {
        let dir = tempfile::tempdir().unwrap();
        let outputs = dir.path().join("outputs");
        let output = summarize_output(&outputs, 1, 0, "short").await;

        assert_eq!(output, "short");
        assert!(!outputs.exists());
    }

    #[tokio::test]
    async fn long_output_keeps_head_and_tail_and_spills_once() {
        let dir = tempfile::tempdir().unwrap();
        let outputs = dir.path().join(".keel/outputs");
        let full: String = (0..5000)
            .map(|index| (b'a' + (index % 26) as u8) as char)
            .collect();
        let summary = summarize_output(&outputs, 7, 2, &full).await;

        let head: String = full.chars().take(TOOL_HEAD_CHARS).collect();
        let tail: String = full
            .chars()
            .skip(full.chars().count() - TOOL_TAIL_CHARS)
            .collect();
        assert!(summary.starts_with(&head));
        assert!(summary.ends_with(&tail));
        let spill = outputs.join("turn-7-call-2.txt");
        assert!(summary.contains(&format!("full output: {}", spill.display())));

        let spilled = tokio::fs::read_to_string(&spill).await.unwrap();
        assert_eq!(spilled, full);
    }

    #[tokio::test]
    async fn spilled_output_is_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let outputs = dir.path().join(".keel/outputs");
        let secret = "sk-abcdefghijklmnopqrstuvwxyz";
        let full = format!("{secret}\n{}\n{}", "y".repeat(4000), "z".repeat(1000));
        let summary = summarize_output(&outputs, 1, 0, &full).await;

        assert!(!summary.contains(secret));
        let spilled = tokio::fs::read_to_string(outputs.join("turn-1-call-0.txt"))
            .await
            .unwrap();
        assert!(!spilled.contains(secret));
    }
}
