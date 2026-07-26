//! Bridges Claude Code's *non-permission* hook events into the pie menu and
//! tray status. Permission requests — including `AskUserQuestion`, which
//! turns out to fire the exact same `PermissionRequest` hook as a Bash/Edit
//! approval — are handled separately and far more cleanly by
//! `permission_server` (a real `"http"`-hook decision channel; see that
//! module's doc comment for the whole story, including why `AskUserQuestion`
//! used to be handled here via keystroke simulation and no longer is).
//!
//! This module is the `"command"`-hook side: `~/.claude/settings.json`
//! registers, for a set of lifecycle events, a `"command"` hook that reads
//! the event JSON off stdin and forwards it, unmodified, to this
//! loopback-only TCP listener via a plain PowerShell one-liner (that reads
//! stdin as explicit UTF-8, so non-ASCII payloads survive intact). It fans
//! out to (see `dispatch`):
//! - `AskUserQuestion`'s `PostToolUse` → auto-dismiss a stale pie-menu
//!   question overlay if it was answered some other way than a pie-menu pick
//!   (`pie_menu::question_answered_externally`) — a safety net that still
//!   applies regardless of how the question got answered (pie menu, typed
//!   directly in the terminal, or `permission_server`'s own "answer in
//!   terminal instead" escape hatch).
//! - every event → a coarse idle/thinking/working/error/attention status
//!   (`claude_status::apply_event`) that drives the tray icon's badge, not a
//!   popup.

use std::io::Read;
use std::net::TcpListener;

use serde::Deserialize;
use tauri::AppHandle;

use crate::claude_status;
use crate::pie_menu;

/// Loopback-only, arbitrary-but-fixed port. Not configurable: this is a
/// single-machine, single-user bridge between this app and a hook entry it
/// generates itself, not a general integration point. (`permission_server`
/// uses the next port up, 47216, for its own separate http endpoint.)
pub const PORT: u16 = 47215;

#[derive(Deserialize)]
struct HookPayload {
    hook_event_name: Option<String>,
    tool_name: Option<String>,
}

fn debug_enabled() -> bool {
    std::env::var_os("DJIMIC_DEBUG").is_some()
}

/// Routes one parsed hook payload: `AskUserQuestion`'s `PostToolUse`
/// dismisses a stale pie-menu overlay; everything else with a
/// `hook_event_name` just updates the coarse tray status. (Permission
/// requests, including `AskUserQuestion` itself, never reach here — they go
/// to `permission_server`'s http endpoint instead.)
fn dispatch(app: &AppHandle, payload: HookPayload) {
    // `AskUserQuestion` resolving — however it actually got answered, not
    // necessarily through the pie menu at all (typed/clicked directly in
    // the terminal, or `permission_server`'s "answer in terminal instead"
    // escape hatch, are just as real ways to answer it). Requested after
    // real use: answering it some other way left the overlay still open,
    // showing a now-stale question with no way to tell it had already been
    // handled. `pie_menu::question_answered_externally` no-ops if the
    // answer *did* come through the pie menu (which already clears its own
    // pending-answer state the moment the user picks something there, well
    // before this event can arrive) — this only cleans up the *other* case.
    if payload.hook_event_name.as_deref() == Some("PostToolUse")
        && payload.tool_name.as_deref() == Some("AskUserQuestion")
    {
        pie_menu::question_answered_externally(app);
    }
    if let Some(event_name) = payload.hook_event_name.as_deref() {
        claude_status::apply_event(event_name);
    }
}

/// Starts the listener on its own thread. Never fails loudly — if the port
/// is already taken (e.g. a previous instance of this app didn't shut down
/// cleanly), this just logs and gives up rather than crashing app startup;
/// the PowerShell hook command's own `try {} catch {}` already makes a
/// refused connection harmless to Claude Code's own permission flow.
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("[hook-bridge] failed to bind 127.0.0.1:{PORT}: {e}");
                return;
            }
        };
        if debug_enabled() {
            eprintln!("[hook-bridge] listening on 127.0.0.1:{PORT}");
        }
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut body = String::new();
            if stream.read_to_string(&mut body).is_err() {
                continue;
            }
            let payload: HookPayload = match serde_json::from_str(&body) {
                Ok(payload) => payload,
                Err(e) => {
                    if debug_enabled() {
                        // `{body:?}` Rust-escapes control characters (\n, \r, \u{...})
                        // instead of printing them raw, so a corrupted/truncated
                        // payload's actual bytes are visible here instead of just
                        // serde_json's line/column, which alone hasn't been enough
                        // to root-cause a couple of parse failures seen in practice.
                        const MAX: usize = 4000;
                        let bytes = body.as_bytes();
                        if bytes.len() > MAX {
                            // Byte-offset slicing (not `body[..]`) since a raw
                            // offset can land mid-UTF-8-character; `from_utf8_lossy`
                            // repairs that with a replacement char instead of
                            // panicking.
                            eprintln!(
                                "[hook-bridge] failed to parse payload: {e} ({} bytes, truncated) — head: {:?} — tail: {:?}",
                                bytes.len(),
                                String::from_utf8_lossy(&bytes[..MAX / 2]),
                                String::from_utf8_lossy(&bytes[bytes.len() - MAX / 2..])
                            );
                        } else {
                            eprintln!(
                                "[hook-bridge] failed to parse payload: {e} ({} bytes) — raw: {body:?}",
                                body.len()
                            );
                        }
                    }
                    continue;
                }
            };
            dispatch(&app, payload);
        }
    });
}
