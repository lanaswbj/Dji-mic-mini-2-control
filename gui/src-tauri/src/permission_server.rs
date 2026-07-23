//! HTTP decision server for Claude Code's `PermissionRequest` `"http"` hook.
//!
//! This is the real, working answer to what `hook_bridge`'s module doc
//! comment used to describe as an unsolved mystery. That comment claimed the
//! `"http"` hook type "never once" received real `PermissionRequest` events
//! and so the whole feature had to fall back to simulating keystrokes into
//! the terminal. That turned out to be **wrong** (or fixed by a later Claude
//! Code version): a from-scratch probe — a throwaway Node HTTP server
//! registered as a third `PermissionRequest` `"http"` hook alongside Clawd on
//! Desk's own — received a full, real permission request the very first time
//! a tool needed approval, and a decision response was honored (the command
//! ran with no terminal prompt at all). So the `"http"` hook *does* work as a
//! genuine allow/deny decision channel; the earlier conclusion was stale.
//!
//! The real request body's shape is also different from what the old
//! `hook_bridge` code assumed. There is no `permission_request.choices`
//! field (that was invented for synthetic tests and never matched reality).
//! A real body is, e.g.:
//! ```json
//! {
//!   "session_id": "...", "cwd": "E:\\path", "permission_mode": "default",
//!   "hook_event_name": "PermissionRequest", "tool_name": "Bash",
//!   "tool_input": { "command": "…", "description": "…" },
//!   "permission_suggestions": [
//!     { "type": "addRules", "behavior": "allow", "destination": "localSettings",
//!       "rules": [ { "toolName": "Bash", "ruleContent": "sudo -n true" } ] }
//!   ]
//! }
//! ```
//!
//! ## The response envelope
//!
//! The response body must be `{"hookSpecificOutput":{"hookEventName":
//! "PermissionRequest","decision":{"behavior":"allow"|"deny",...}}}` — a
//! bare `{"decision":"allow"}` (what an earlier version of this file sent)
//! happened to be honored for the plain allow/deny case, but the *documented*
//! shape (confirmed against Clawd on Desk's mature, widely-used
//! implementation of this exact hook — see `build_response`) is the
//! `hookSpecificOutput` envelope, and only that shape supports the
//! `updatedInput` field the `AskUserQuestion` handling below depends on.
//!
//! ## AskUserQuestion answers via `updatedInput`, not keystrokes
//!
//! `AskUserQuestion` — Claude Code's own interactive multi-choice-plus-
//! freeform-text tool — turns out to fire this *exact same* `PermissionRequest`
//! hook (confirmed empirically: added a diagnostic that logs every request's
//! `tool_name` to a file, then triggered a real `AskUserQuestion` tool call
//! and watched `tool_name="AskUserQuestion"` arrive here with
//! `tool_input: {questions: [...]}`, under `permission_mode: "acceptEdits"`
//! for this dev session specifically — i.e. it goes through the *identical*
//! gate as a Bash command needing approval, not some separate mechanism).
//! Before this was known, this project answered `AskUserQuestion` by
//! simulating real arrow-key/Enter presses into whatever window was showing
//! Claude Code's own terminal picker (see `pie_menu`'s now-removed
//! `close_gap_locked`/`navigate_remaining_gap`/`ASKUSERQUESTION_REAL_OFFSET`)
//! — fragile by nature (depends on the host UI's own keyboard semantics,
//! which vary: a raw CLI's arrow-key list navigation is not guaranteed to
//! match an Electron/browser-hosted chat UI's rendering of the same tool
//! call) and, worse, broke entirely once a real permission request could
//! also be mid-flight on the very same overlay (see `QUEUE`'s doc comment).
//!
//! The fix, once the mechanism above was confirmed: when `tool_name ==
//! "AskUserQuestion"`, don't treat it like a plain Bash/Edit permission.
//! Extract the real question/options from `tool_input.questions[0]`, show
//! them in the pie menu (`pie_menu::show_pending_ask_user_question`), and on
//! a pick, respond `allow` with an `updatedInput` that echoes the original
//! `tool_input` plus an `answers: {question_text: chosen_label}` map — Claude
//! Code then treats the tool call as if the user had answered exactly that,
//! with **no terminal interaction, no keystroke injection, no focus stealing
//! at all**. This is the exact mechanism Clawd on Desk (a mature, widely-used
//! reference implementation of this same idea — its own source calls this
//! tool an "elicitation") uses for the identical problem; its
//! `buildElicitationUpdatedInput`/`sendPermissionResponse` functions are what
//! confirmed both the `updatedInput` field and the `hookSpecificOutput`
//! envelope shape here. If the user instead picks "answer in terminal
//! instead" (the trailing escape-hatch slot — see
//! `show_pending_ask_user_question`), this responds `deny` with a message,
//! which — per the same reference implementation's own comment — makes
//! Claude Code fall back to showing its native interactive picker, where the
//! user can then answer directly (typing, or however). Multi-select
//! questions (an arc pick can't represent "confirm N checked boxes") and any
//! question missing text/options fall through to a plain instant `allow`
//! with no `updatedInput`, letting Claude Code show its own native picker
//! immediately — same conservative behavior as the old `hook_bridge` code had
//! for multi-select.
//!
//! ## No mode-based gating
//!
//! An earlier version only popped the pie menu and waited when
//! `permission_mode == "default"`, auto-allowing instantly in every other
//! mode (`acceptEdits`, `bypassPermissions`, `plan`, absent, ...) on the
//! theory that Claude Code only *genuinely* needs an interactive decision in
//! `default` mode, and every other mode already leans toward auto-approving
//! — specifically so this exact server, also hit by every `PermissionRequest`
//! in *this very* Claude Code session (the one doing development), wouldn't
//! deadlock the assistant's own work on every command it ran.
//!
//! That theory was wrong, caught by real use: a Bash command in this dev
//! session's own `acceptEdits` mode (which by definition only auto-accepts
//! file edits, not command execution) still fired this hook expecting a real
//! decision — auto-allowing there wasn't Claude Code choosing not to ask, it
//! was this code silently deciding *for* the user instead. Claude Code
//! firing this hook at all already means it wants an answer; if it didn't,
//! it simply wouldn't send the request. So every request now pops the pie
//! menu and waits, full stop, regardless of `permission_mode` — matching
//! Claude Code's own decision to ask rather than second-guessing it. The
//! practical effect for this dev session: every one of its own tool calls
//! now genuinely waits on a pick too, same as any other client of this
//! server — no longer specially exempted.
//!
//! ## Coexistence with Clawd on Desk
//!
//! Clawd registers its own `"http"` `PermissionRequest` hook. Claude Code
//! calls *all* registered http hooks (the probe above was honored even with
//! Clawd's present), so both fire. If both return a decision they could in
//! principle disagree; in practice the probe's decision was honored fine
//! alongside Clawd. If a conflict ever surfaces, the fix is to disable
//! Clawd's competing hook — not something this side can arbitrate.
//!
//! ## Requests can overlap — this session included
//!
//! Since every request now genuinely waits (see above), overlap is the norm,
//! not an edge case: this very development session (the one whose commands
//! built this file) is itself a constant client of this exact server, and a
//! human tester's own separate session can easily have a request in flight
//! at the same moment. `QUEUE` (below) exists because of that — an earlier
//! version held a single `Option<Sender>` slot and a second arrival just
//! overwrote it, silently dropping whatever was still showing. See `QUEUE`'s
//! own doc comment for the two real symptoms that produced.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::AppHandle;

use crate::pie_menu;

/// Loopback-only, fixed port — the URL `~/.claude/settings.json` registers
/// the `"http"` hook at (`http://127.0.0.1:47216/permission`). One higher
/// than `hook_bridge`'s raw-TCP command-hook port (47215) so the two never
/// collide.
const PORT: u16 = 47216;

/// How long the pop-and-wait path waits for a pie-menu pick before giving up
/// and denying. Comfortably under Claude Code's own 600s http-hook timeout,
/// so we always answer first. Denying (rather than allowing) on timeout is
/// the safe default for an unattended prompt — nothing happens. Runs from
/// each request's own arrival (not from when it's actually shown — see
/// `QUEUE`), so a request stuck behind several others can time out before a
/// human ever sees it; acceptable, since anything backed up that deep is
/// already stale by the time it would be shown.
const MAX_WAIT: Duration = Duration::from_secs(120);

/// The decision a pie-menu slot maps to for a *plain* Permission request
/// (Bash/Edit/etc — see `pie_menu`'s `Permission` answer branch).
/// `AllowAlways` additionally persists a "don't ask again" rule (see
/// `apply_always_rules`). `AskUserQuestion` answers go through
/// `resolve_question` instead — see `Resolution`.
#[derive(Clone, Copy, Debug)]
pub enum Decision {
    Allow,
    AllowAlways,
    Deny,
}

struct AlwaysRules {
    cwd: String,
    rules: Vec<String>,
}

/// What kind of `PermissionRequest` this is — a plain tool needing an
/// Allow/Deny call (`Simple`), or an `AskUserQuestion` whose answer can be
/// supplied directly via the response's `updatedInput` instead (see module
/// doc comment).
enum RequestKind {
    Simple,
    Question {
        /// The specific question's own text — only needed for display here
        /// (`show_front`); the connection thread that owns this request
        /// (`handle_ask_user_question`) builds the actual `updatedInput`
        /// response from its own local copy of `tool_input` once `resolve_
        /// question` wakes it, rather than reading it back out through the
        /// queue.
        question_text: String,
        /// Listed option labels, in order — `options[i]` is the answer text
        /// for pie-menu slot `i`.
        options: Vec<String>,
    },
}

/// What a pie-menu pick resolves a request with. `Simple` carries a plain
/// Permission's three-way decision. `Question` carries `Some(i)` (picked
/// listed option `i`) or `None` ("answer in terminal instead" — resolves as
/// a deny so Claude Code falls back to its own interactive picker, mirroring
/// Clawd on Desk's own "Go to Terminal" escape hatch for the same case).
enum Resolution {
    Simple(Decision),
    Question(Option<u32>),
}

/// One in-flight `PermissionRequest`, from arrival until it's answered
/// (`resolve`/`resolve_question`) or times out. `id` lets a connection
/// thread find and remove exactly itself from `QUEUE` without relying on
/// `Sender` identity.
struct PendingRequest {
    id: u64,
    tx: Sender<Resolution>,
    tool_name: String,
    detail: String,
    always: Option<AlwaysRules>,
    kind: RequestKind,
}

/// FIFO of every `PermissionRequest` that has arrived and not yet been
/// answered or timed out — this is the fix for a real bug: an earlier
/// design held only a single `Option<Sender>` slot, so a second request
/// arriving while the first was still showing (unanswered) silently
/// *overwrote* it, dropping the first request's sender. That first
/// connection's `recv` then errored out and denied, but — critically —
/// nothing told the frontend the card it was still showing (and that the
/// user might be mid-click on) no longer corresponded to anything waiting.
/// Real use surfaced this two ways at once: "Allow" appearing to do nothing
/// (the user's click resolved a request that had already been silently
/// denied out from under the visible card) and the overlay "flashing" (each
/// new request re-popped/re-focused the window regardless of one already
/// being mid-display). Only the item at the *front* is ever shown
/// (`show_front`) — later arrivals wait their turn and are shown only once
/// everything ahead of them has resolved or timed out.
static QUEUE: Mutex<VecDeque<PendingRequest>> = Mutex::new(VecDeque::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Stashed by `spawn` so `resolve`/`resolve_question`/timeout handling
/// (called from a Tauri command handler or this module's own connection
/// threads, neither of which carry an `AppHandle` otherwise) can show the
/// next queued request — same pattern as `pie_menu::APP_HANDLE`.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

fn debug_enabled() -> bool {
    std::env::var_os("DJIMIC_DEBUG").is_some()
}

/// Pops the pie-menu card for whatever is now at the front of `queue` — a
/// no-op if the queue is empty. Called with `QUEUE` already locked (the
/// caller holds the guard), so the two can never disagree about which
/// request is "current."
fn show_front(queue: &VecDeque<PendingRequest>) {
    let Some(front) = queue.front() else { return };
    let Some(app) = APP_HANDLE.get() else { return };
    if debug_enabled() {
        eprintln!(
            "[permission] showing request #{} ({} more queued behind it)",
            front.id,
            queue.len() - 1
        );
    }
    match &front.kind {
        RequestKind::Simple => {
            pie_menu::show_pending_permission(app, front.tool_name.clone(), front.detail.clone());
        }
        RequestKind::Question {
            question_text,
            options,
            ..
        } => {
            pie_menu::show_pending_ask_user_question(
                app,
                question_text.clone(),
                options.clone(),
            );
        }
    }
}

/// Pops the front-of-queue request, hands it `resolution`, and immediately
/// shows whatever is now at the new front (if anything) — so the queue
/// actively drains instead of requiring each request to be separately
/// triggered. Returns false if the queue was already empty (a stale/double
/// pick — harmless).
fn resolve_with(resolution: Resolution) -> bool {
    let mut queue = QUEUE.lock().unwrap();
    let Some(front) = queue.pop_front() else {
        return false;
    };
    if matches!(resolution, Resolution::Simple(Decision::AllowAlways)) {
        if let Some(always) = &front.always {
            apply_always_rules(always);
        }
    }
    let _ = front.tx.send(resolution);
    show_front(&queue);
    true
}

/// Delivers the user's pie-menu pick to a plain Permission request's waiting
/// HTTP connection. Called from `pie_menu::pie_menu_answer_question`'s
/// `Permission` branch.
pub fn resolve(decision: Decision) -> bool {
    resolve_with(Resolution::Simple(decision))
}

/// Delivers the user's pie-menu pick to an `AskUserQuestion` request's
/// waiting HTTP connection. `Some(i)` answers with listed option `i`;
/// `None` is the "answer in terminal instead" escape hatch (resolves as
/// deny). Called from `pie_menu::pie_menu_answer_question`'s
/// `AskUserQuestion` branch.
pub fn resolve_question(pick: Option<u32>) -> bool {
    resolve_with(Resolution::Question(pick))
}

/// True if at least one `PermissionRequest` is currently queued (shown or
/// waiting its turn). Not called anywhere yet — kept as a small, obviously
/// correct building block for whatever needs it next (e.g. a tray-icon
/// badge) rather than re-deriving queue-emptiness ad hoc elsewhere.
#[allow(dead_code)]
pub fn is_pending() -> bool {
    !QUEUE.lock().unwrap().is_empty()
}

/// Starts the HTTP listener on its own thread. Never fails loudly — a taken
/// port (a stale previous instance) just logs and gives up, exactly like
/// `hook_bridge::spawn`; Claude Code's own http-hook client already treats a
/// refused/again-unavailable endpoint as a soft failure and falls back to its
/// normal prompt.
pub fn spawn(app: AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("[permission] failed to bind 127.0.0.1:{PORT}: {e}");
                return;
            }
        };
        if debug_enabled() {
            eprintln!("[permission] listening on 127.0.0.1:{PORT}");
        }
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            // One thread per connection: the pop-and-wait path blocks for up
            // to MAX_WAIT, which must not stall the accept loop.
            std::thread::spawn(move || handle_connection(stream));
        }
    });
}

fn handle_connection(mut stream: TcpStream) {
    let Some(body) = read_request_body(&mut stream) else {
        respond(&mut stream, &build_response("allow", None, None));
        return;
    };
    let payload: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            if debug_enabled() {
                eprintln!("[permission] failed to parse body: {e}");
            }
            respond(&mut stream, &build_response("allow", None, None));
            return;
        }
    };

    let mode = payload
        .get("permission_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tool_name = payload
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("A tool")
        .to_string();

    if debug_enabled() {
        eprintln!("[permission] request received tool={tool_name:?} mode={mode:?}");
    }

    // No mode-based gating: an earlier version auto-allowed instantly unless
    // `permission_mode == "default"`, on the theory that Claude Code only
    // *genuinely* needs a decision in that mode and every other mode already
    // leans toward auto-approving. That theory was wrong — real testing
    // showed Claude Code fires this exact hook, awaiting a real decision,
    // for a Bash command even in `acceptEdits` mode (which only auto-accepts
    // file edits, not command execution) and presumably others; auto-
    // allowing there was this code silently deciding *for* the user, not
    // Claude Code doing so. Claude Code firing this hook at all already means
    // it wants an answer — if it didn't, it simply wouldn't send the
    // request — so every request now pops the pie menu and waits, full
    // stop, matching Claude Code's own decision to ask rather than
    // second-guessing it by mode.
    if tool_name == "AskUserQuestion" {
        handle_ask_user_question(payload, stream);
        return;
    }

    let detail = request_detail(&payload);
    let cwd = payload
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let always_rules = collect_always_rules(&payload);

    if debug_enabled() {
        eprintln!("[permission] request tool={tool_name:?} detail={detail:?}");
    }

    let (tx, rx) = channel::<Resolution>();
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let request = PendingRequest {
        id,
        tx,
        tool_name,
        detail,
        always: if always_rules.is_empty() {
            None
        } else {
            Some(AlwaysRules {
                cwd,
                rules: always_rules,
            })
        },
        kind: RequestKind::Simple,
    };

    {
        let mut queue = QUEUE.lock().unwrap();
        queue.push_back(request);
        // Only pop the card for this one if it's now at the front — i.e.
        // nothing else was already showing. If something else is already
        // pending, this one just waits in line; `resolve`/the timeout path
        // below will show it once its turn comes.
        if queue.len() == 1 {
            show_front(&queue);
        } else if debug_enabled() {
            eprintln!(
                "[permission] request #{id} queued behind {} pending request(s)",
                queue.len() - 1
            );
        }
    }

    let resolution = match rx.recv_timeout(MAX_WAIT) {
        Ok(r) => r,
        Err(_) => {
            // Timed out with no pick. Remove exactly this request from
            // wherever it is in the queue — it may still be waiting its turn
            // (never shown, nothing to clean up on screen) or sitting at the
            // front (currently shown and now stale). Only the latter needs
            // any UI reaction: dismiss the overlay if nothing is left queued,
            // or advance straight to showing whatever's next.
            let mut queue = QUEUE.lock().unwrap();
            let was_front = queue.front().map(|r| r.id) == Some(id);
            queue.retain(|r| r.id != id);
            if was_front {
                if debug_enabled() {
                    eprintln!("[permission] request #{id} timed out after {MAX_WAIT:?} — denying");
                }
                if queue.is_empty() {
                    if let Some(app) = APP_HANDLE.get() {
                        pie_menu::force_close_permission(app);
                    }
                } else {
                    show_front(&queue);
                }
            }
            Resolution::Simple(Decision::Deny)
        }
    };

    let decision = match resolution {
        Resolution::Simple(d) => d,
        // Shouldn't happen (this request was pushed as `Simple`) — deny
        // defensively rather than send a response shape that doesn't match.
        Resolution::Question(_) => Decision::Deny,
    };
    let response = match decision {
        Decision::Allow | Decision::AllowAlways => build_response("allow", None, None),
        Decision::Deny => build_response("deny", None, None),
    };
    if debug_enabled() {
        eprintln!("[permission] responding {response}");
    }
    respond(&mut stream, &response);
}

/// Handles a `PermissionRequest` whose `tool_name` is `AskUserQuestion` — see
/// the module doc comment for the whole mechanism. Only the *first* question
/// is surfaced (a batch of several isn't representable in one pie menu, same
/// limitation the old `hook_bridge`-based handling had) and multi-select
/// questions fall through to a plain instant allow (an arc pick can't
/// represent "confirm N checked boxes").
fn handle_ask_user_question(payload: serde_json::Value, mut stream: TcpStream) {
    let tool_input = payload
        .get("tool_input")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let question = tool_input
        .get("questions")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first());

    let Some(question) = question else {
        if debug_enabled() {
            eprintln!("[permission] AskUserQuestion had no questions — auto-allowing");
        }
        respond(&mut stream, &build_response("allow", None, None));
        return;
    };

    let multi_select = question
        .get("multiSelect")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let question_text = question
        .get("question")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let options: Vec<String> = question
        .get("options")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .enumerate()
                .map(|(i, opt)| {
                    opt.get("label")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("Option {}", i + 1))
                })
                .collect()
        })
        .unwrap_or_default();

    if multi_select || options.is_empty() || question_text.is_empty() {
        if debug_enabled() {
            eprintln!(
                "[permission] AskUserQuestion multi_select={multi_select} options={} — auto-allowing without popping the menu",
                options.len()
            );
        }
        respond(&mut stream, &build_response("allow", None, None));
        return;
    }

    if debug_enabled() {
        eprintln!(
            "[permission] AskUserQuestion: {question_text:?}, {} option(s)",
            options.len()
        );
    }

    let (tx, rx) = channel::<Resolution>();
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let request = PendingRequest {
        id,
        tx,
        tool_name: "AskUserQuestion".to_string(),
        detail: String::new(),
        always: None,
        kind: RequestKind::Question {
            question_text: question_text.clone(),
            options: options.clone(),
        },
    };

    {
        let mut queue = QUEUE.lock().unwrap();
        queue.push_back(request);
        if queue.len() == 1 {
            show_front(&queue);
        } else if debug_enabled() {
            eprintln!(
                "[permission] request #{id} (AskUserQuestion) queued behind {} pending request(s)",
                queue.len() - 1
            );
        }
    }

    let resolution = match rx.recv_timeout(MAX_WAIT) {
        Ok(r) => r,
        Err(_) => {
            let mut queue = QUEUE.lock().unwrap();
            let was_front = queue.front().map(|r| r.id) == Some(id);
            queue.retain(|r| r.id != id);
            if was_front {
                if debug_enabled() {
                    eprintln!(
                        "[permission] request #{id} (AskUserQuestion) timed out after {MAX_WAIT:?} — denying"
                    );
                }
                if queue.is_empty() {
                    if let Some(app) = APP_HANDLE.get() {
                        pie_menu::force_close_permission(app);
                    }
                } else {
                    show_front(&queue);
                }
            }
            Resolution::Question(None)
        }
    };

    let pick = match resolution {
        Resolution::Question(p) => p,
        // Shouldn't happen (this request was pushed as `Question`).
        Resolution::Simple(_) => None,
    };

    match pick {
        Some(i) if (i as usize) < options.len() => {
            let mut updated_input = tool_input;
            if let Some(obj) = updated_input.as_object_mut() {
                let mut answers = serde_json::Map::new();
                answers.insert(
                    question_text,
                    serde_json::Value::String(options[i as usize].clone()),
                );
                obj.insert("answers".to_string(), serde_json::Value::Object(answers));
            }
            let response = build_response("allow", Some(&updated_input), None);
            if debug_enabled() {
                eprintln!("[permission] responding {response}");
            }
            respond(&mut stream, &response);
        }
        _ => {
            // "Answer in terminal instead", or a timeout — deny so Claude
            // Code falls back to its own native interactive picker (see
            // module doc comment).
            let response = build_response("deny", None, Some("Answering in the terminal instead"));
            if debug_enabled() {
                eprintln!("[permission] responding {response}");
            }
            respond(&mut stream, &response);
        }
    }
}

/// A short, human-readable description of what's being requested, for the pie
/// menu's title. Prefers a Bash command, then any tool's `description`, then
/// a compact single-line dump of `tool_input`.
fn request_detail(payload: &serde_json::Value) -> String {
    let input = payload.get("tool_input");
    if let Some(cmd) = input
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
    {
        return cmd.to_string();
    }
    if let Some(path) = input
        .and_then(|v| v.get("file_path"))
        .and_then(|v| v.as_str())
    {
        return path.to_string();
    }
    if let Some(desc) = input
        .and_then(|v| v.get("description"))
        .and_then(|v| v.as_str())
    {
        return desc.to_string();
    }
    input
        .map(|v| v.to_string())
        .unwrap_or_default()
}

/// Every `permission_suggestions` rule with `behavior: "allow"`, formatted as
/// the `ToolName(ruleContent)` permission strings Claude Code's own
/// `permissions.allow` settings array uses. Empty if the request carried no
/// suggestions (then the pie menu's "Allow, don't ask again" slot just
/// behaves like a plain allow).
fn collect_always_rules(payload: &serde_json::Value) -> Vec<String> {
    let Some(suggestions) = payload
        .get("permission_suggestions")
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for s in suggestions {
        if s.get("behavior").and_then(|v| v.as_str()) != Some("allow") {
            continue;
        }
        let Some(rules) = s.get("rules").and_then(|v| v.as_array()) else {
            continue;
        };
        for r in rules {
            let tool = r.get("toolName").and_then(|v| v.as_str());
            let content = r.get("ruleContent").and_then(|v| v.as_str());
            match (tool, content) {
                (Some(tool), Some(content)) => out.push(format!("{tool}({content})")),
                (Some(tool), None) => out.push(tool.to_string()),
                _ => {}
            }
        }
    }
    out.dedup();
    out
}

/// Best-effort persist of "Allow, don't ask again" rules into
/// `{cwd}/.claude/settings.local.json`'s `permissions.allow` array — the same
/// file and format Claude Code's own `destination: "localSettings"`
/// suggestions target. Best-effort by design: if the read/merge/write fails
/// for any reason (file locked by Claude Code mid-write, unexpected shape),
/// the decision already returned is still a plain allow, so the command
/// proceeds — only the "remember it" part is lost, degrading to allow-once.
fn apply_always_rules(always: &AlwaysRules) {
    if always.cwd.is_empty() || always.rules.is_empty() {
        return;
    }
    let dir = std::path::Path::new(&always.cwd).join(".claude");
    let path = dir.join("settings.local.json");
    let _ = std::fs::create_dir_all(&dir);

    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }

    let allow = root
        .as_object_mut()
        .unwrap()
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .and_then(|p| Some(p.entry("allow").or_insert_with(|| serde_json::json!([]))));
    let Some(allow) = allow else { return };
    if !allow.is_array() {
        *allow = serde_json::json!([]);
    }
    let arr = allow.as_array_mut().unwrap();
    for rule in &always.rules {
        let exists = arr.iter().any(|v| v.as_str() == Some(rule.as_str()));
        if !exists {
            arr.push(serde_json::Value::String(rule.clone()));
        }
    }

    if let Ok(serialized) = serde_json::to_string_pretty(&root) {
        let _ = std::fs::write(&path, serialized);
        if debug_enabled() {
            eprintln!(
                "[permission] persisted {} always-allow rule(s) to {}",
                always.rules.len(),
                path.display()
            );
        }
    }
}

/// Reads an HTTP request off `stream` and returns just its body. Minimal by
/// design — reads until the `\r\n\r\n` header terminator, honors
/// `Content-Length`, and ignores everything else about HTTP (method, path,
/// other headers); this endpoint only ever receives Claude Code's own single
/// POST shape. A 5s read timeout guards against a half-open connection
/// wedging the connection thread.
fn read_request_body(stream: &mut TcpStream) -> Option<String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 1_000_000 {
            return None;
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().ok())
        })
        .flatten()
        .unwrap_or(0);
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = match stream.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        body.extend_from_slice(&tmp[..n]);
    }
    let end = content_length.min(body.len());
    Some(String::from_utf8_lossy(&body[..end]).to_string())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Builds the `hookSpecificOutput`-enveloped response body Claude Code's
/// `PermissionRequest` http hook expects (see module doc comment — confirmed
/// against Clawd on Desk's own `sendPermissionResponse`). `updated_input`
/// (only ever set alongside `"allow"`) is what lets an `AskUserQuestion`
/// answer skip the terminal entirely; `message` is shown to Claude Code as
/// the reason for a `"deny"`.
fn build_response(
    behavior: &str,
    updated_input: Option<&serde_json::Value>,
    message: Option<&str>,
) -> String {
    let mut decision = serde_json::Map::new();
    decision.insert(
        "behavior".to_string(),
        serde_json::Value::String(behavior.to_string()),
    );
    if let Some(input) = updated_input {
        decision.insert("updatedInput".to_string(), input.clone());
    }
    if let Some(msg) = message {
        decision.insert(
            "message".to_string(),
            serde_json::Value::String(msg.to_string()),
        );
    }
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": serde_json::Value::Object(decision),
        }
    })
    .to_string()
}

/// Writes a minimal `200 OK` JSON response and closes. `Connection: close`
/// (rather than honoring the request's keep-alive) keeps this dead simple —
/// Claude Code just opens a fresh connection for the next request.
fn respond(stream: &mut TcpStream, json_body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        json_body.len(),
        json_body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}
