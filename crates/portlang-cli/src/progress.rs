use std::sync::{LazyLock, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Global spinner status updated by the tracing layer and polled by the progress bar.
pub static STATUS: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));

pub fn set_status(msg: impl Into<String>) {
    if let Ok(mut g) = STATUS.lock() {
        *g = msg.into();
    }
}

pub fn get_status() -> String {
    STATUS.lock().map(|g| g.clone()).unwrap_or_default()
}

/// Tracing subscriber layer that translates INFO log messages into spinner status updates.
pub struct ProgressTracingLayer;

impl<S: Subscriber> Layer<S> for ProgressTracingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() > tracing::Level::INFO {
            return;
        }

        let mut v = MsgVisitor::default();
        event.record(&mut v);
        let msg = v.0;

        let status = if msg.contains("Building image") {
            "Building container...".to_string()
        } else if msg.contains("Started Apple container") {
            "Container ready, starting agent...".to_string()
        } else if let Some(rest) = msg.strip_prefix("Starting step ") {
            format!("Step {}", rest.trim())
        } else if msg.starts_with("\u{2190} tool_result") {
            // "← tool_result ToolName preview..." — show as result of current step
            let after = msg
                .trim_start_matches('\u{2190}')
                .trim_start_matches(" tool_result")
                .trim();
            let tool = after.split_whitespace().next().unwrap_or(after);
            let short = tool
                .strip_prefix("mcp__")
                .and_then(|s| s.split_once("__").map(|(_, name)| name))
                .unwrap_or(tool);
            format!("\u{2190} {}", short)
        } else if msg.starts_with("\u{2192} tool_use") {
            // "→ tool_use  ToolName {args...}" — extract just the tool name
            let after = msg
                .trim_start_matches('\u{2192}')
                .trim_start_matches(" tool_use")
                .trim();
            let tool = after.split_whitespace().next().unwrap_or(after);
            // shorten mcp__ prefix for readability
            let short = tool
                .strip_prefix("mcp__")
                .and_then(|s| s.split_once("__").map(|(_, name)| name))
                .unwrap_or(tool);
            format!("\u{2192} {}", short)
        } else {
            return;
        };

        set_status(status);
    }
}

#[derive(Default)]
struct MsgVisitor(String);

impl Visit for MsgVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{:?}", value);
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        }
    }
}
