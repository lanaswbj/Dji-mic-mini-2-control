//! Coarse, best-effort tracking of what Claude Code is currently doing,
//! derived from the same hook events `hook_bridge` already receives on its
//! loopback listener — drives the system tray icon's status badge (see
//! `main.rs`), not a popup. Interactive prompts (permission requests,
//! `AskUserQuestion`) are handled separately by `hook_bridge`/`pie_menu`
//! themselves; this module only tracks the passive idle/thinking/working/
//! error/attention state shown ambiently in the tray.
//!
//! Deliberately not a precise per-session state machine: Claude Code's hooks
//! are fire-and-forget, one-way, with no request/response correlation, so
//! two concurrent sessions (or a subagent racing the main turn) are
//! indistinguishable from each other here — last event wins, the same
//! trade-off the tray's own 1000ms poll loop already makes for device state.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ClaudeStatus {
    Idle = 0,
    Thinking = 1,
    Working = 2,
    NeedsAttention = 3,
    Error = 4,
}

impl ClaudeStatus {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Thinking,
            2 => Self::Working,
            3 => Self::NeedsAttention,
            4 => Self::Error,
            _ => Self::Idle,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(ClaudeStatus::Idle as u8);

pub fn set(status: ClaudeStatus) {
    CURRENT.store(status as u8, Ordering::Relaxed);
}

pub fn get() -> ClaudeStatus {
    ClaudeStatus::from_u8(CURRENT.load(Ordering::Relaxed))
}

/// Maps a lifecycle hook's `hook_event_name` to a coarse status — see the
/// module doc comment for why this is last-write-wins rather than a real
/// state machine. `PreToolUse` defaults to `Working`; `hook_bridge` upgrades
/// it to `NeedsAttention` itself when the tool turns out to be
/// `AskUserQuestion` (a real interactive prompt, not just background work).
pub fn apply_event(event_name: &str) {
    use ClaudeStatus::*;
    match event_name {
        "SessionStart" | "SessionEnd" => set(Idle),
        "UserPromptSubmit" => set(Thinking),
        "PreToolUse" => set(Working),
        "PostToolUse" => set(Thinking),
        "PostToolUseFailure" | "StopFailure" => set(Error),
        "Stop" | "PermissionRequest" | "Notification" => set(NeedsAttention),
        "PreCompact" => set(Working),
        "PostCompact" => set(Thinking),
        // SubagentStart/SubagentStop deliberately ignored — a subagent's own
        // lifecycle isn't the foreground assistant's turn.
        _ => {}
    }
}
