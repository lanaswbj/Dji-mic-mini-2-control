//! Global Ctrl+Alt+P hotkey fan/pie menu — press it anywhere, even while the
//! app has no focused window, to pop up a semicircular arc menu of shortcut
//! slots docked just above the taskbar (à la the old LG webOS quick menu,
//! called up from the remote's five-way wheel). Left/Right (or Up/Down) move
//! the highlighted slot, Enter confirms, Escape or losing focus cancels.
//!
//! The menu can also be opened and driven entirely from the receiver's own
//! hardware, without touching the keyboard: a double mic-tap opens it (see
//! `open_if_closed`, called from `mic_tap.rs`) and a single tap moves the
//! highlight right, wrapping back to the first slot past the last one
//! (`navigate`, wraparound lives in `PieMenu.svelte`'s `move`). The pairing
//! button (`pairing_button.rs`) doesn't call into this module to confirm at
//! all — it just always simulates a real Enter keypress, which lands on the
//! overlay's own focused window and triggers its normal Enter handling when
//! the menu is open — except when the voice-input slot's Win+Ctrl hold is
//! currently active, which a press ends instead
//! (`end_voice_hold_if_active`). See `pie_menu_select` for what each of the
//! six slots actually does.
//!
//! Beyond that fixed six-slot menu, the overlay can be overridden with a
//! pending prompt relayed from Claude Code itself — a `PermissionRequest`
//! (relayed by `permission_server`, either a plain Bash/Edit-style approval
//! or a single-select `AskUserQuestion` tool call, since it turns out Claude
//! Code fires the exact same hook for both — see `permission_server`'s
//! module doc comment) — complete with real title/option text in a
//! redesigned card above the arc (see `PendingQuestion`,
//! `show_pending_permission`, `show_pending_ask_user_question`). Both kinds
//! answer by resolving `permission_server`'s held http connection directly
//! — no terminal keystrokes, no focus stealing, no navigation state to keep
//! in sync — see the `PendingAnswer` enum and `pie_menu_answer_question`.
//!
//! An earlier version bound a bare, unmodified `K` (deliberately making `K`
//! untypeable system-wide while the app ran), then briefly tried Ctrl+Alt+K
//! and Win+Alt+B. A bare `Fn` modifier was considered and ruled out first:
//! on essentially all non-Apple laptops `Fn` is intercepted by the
//! keyboard's own firmware/embedded controller and never reaches Windows as
//! a real keystroke, so there is no `MOD_FN` and `RegisterHotKey` cannot see
//! it. Win+Alt+B turned out to already be reserved by Windows 11 (HDR
//! toggle) — it sits in the same `Win+Alt+{K,B,G,R,M,PrtScn}` family Windows
//! reserves for its own mic-mute/HDR/Xbox Game Bar features, so any `Win+Alt`
//! combo risks the same collision. Ctrl+Alt+P avoids the Windows-key
//! modifier entirely and isn't a known OS or common-app shortcut.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};
#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    SetForegroundWindow,
};

use crate::key_inject;

/// Debug-only: the target window's title, so `DJIMIC_DEBUG=1` logging can
/// show *which* window a foreground hand-off actually landed on/targeted,
/// instead of just an opaque HWND value.
#[cfg(windows)]
fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

/// Plain `SetForegroundWindow` is denied silently and unpredictably by
/// Windows' focus-stealing prevention (the "foreground lock timeout") once
/// the calling thread no longer owns the foreground and enough time or
/// unrelated input has passed — which is exactly the situation every
/// hand-off in `pie_menu_select` is in (a background thread, well after the
/// hotkey/HID event that triggered it). A denied call fails with no error
/// and no visible change, which read as "sometimes typing/keyboard nav just
/// doesn't work" and, on the calls that *did* partially apply before being
/// denied, a visible flicker. `AttachThreadInput` temporarily shares input
/// state with both the current foreground window's thread and the target's
/// thread, which is the standard, documented way to make the switch succeed
/// deterministically instead of racing Windows' heuristic.
#[cfg(windows)]
unsafe fn force_foreground(hwnd: HWND) {
    if hwnd.0.is_null() {
        return;
    }
    let current_thread = GetCurrentThreadId();
    let foreground = GetForegroundWindow();
    let foreground_thread = GetWindowThreadProcessId(foreground, None);
    let target_thread = GetWindowThreadProcessId(hwnd, None);

    let attach_fg = foreground_thread != 0 && foreground_thread != current_thread;
    let attach_target = target_thread != 0 && target_thread != current_thread;

    if attach_fg {
        let _ = AttachThreadInput(current_thread, foreground_thread, true);
    }
    if attach_target {
        let _ = AttachThreadInput(current_thread, target_thread, true);
    }

    let _ = SetForegroundWindow(hwnd);
    let _ = BringWindowToTop(hwnd);

    if attach_target {
        let _ = AttachThreadInput(current_thread, target_thread, false);
    }
    if attach_fg {
        let _ = AttachThreadInput(current_thread, foreground_thread, false);
    }
}

/// Set by the voice-input slot (index 0) right after it toggles voice input
/// on (`key_inject::press_voice_toggle`); cleared by
/// `end_voice_hold_if_active` when a pairing-button press toggles it back
/// off. Module-level since `pairing_button.rs` needs to check it on every
/// press regardless of which `PairingButtonWatcher` instance fired. Named
/// for an earlier held-key-combo design (see `key_inject::press_voice_toggle`'s
/// doc comment) — nothing is actually held anymore, this just tracks
/// whether *this app* still thinks voice input is on, which is a purely
/// logical flag with no way to be verified against the target app's own
/// real state. That gap is exactly why real use surfaced a stuck case: the
/// user ending dictation some other way (not via the pairing button) leaves
/// this stuck `true` — `spawn_voice_hold_watchdog` auto-clears it after
/// `VOICE_HOLD_TIMEOUT` as a recovery net, not a real fix for the
/// underlying unobservability.
static VOICE_HOLD_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Set (to the current time, via `now_millis`) whenever `VOICE_HOLD_ACTIVE`
/// is set `true` — read by `spawn_voice_hold_watchdog` to detect a stuck
/// hold. 0 has no special meaning beyond "not currently relevant" (checked
/// only while `VOICE_HOLD_ACTIVE` is also true).
static VOICE_HOLD_STARTED_AT_MILLIS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Generous on purpose: a real dictated answer can legitimately run long,
/// and this is only a recovery net for the case where the app's own
/// `VOICE_HOLD_ACTIVE` flag never got cleared because the user ended voice
/// input some other way than the pairing button — not a UX-tuned limit on
/// how long dictation is allowed to take.
const VOICE_HOLD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Stashed by `spawn` so `end_voice_hold_if_active` (called from
/// `pairing_button.rs`, which has no `AppHandle` of its own) can reclaim
/// overlay focus once dictation ends — see that function's doc comment.
static APP_HANDLE: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

/// Whatever window was in the foreground right before the overlay opened
/// (see `open`) — every non-close slot (see `pie_menu_select`) briefly hands
/// focus back to this so its simulated keystroke actually lands there
/// instead of on the still-open overlay, then reclaims focus for the
/// overlay again afterward.
static PREVIOUS_FOREGROUND: AtomicIsize = AtomicIsize::new(0);

/// The slot index that actually closes the menu — every other slot leaves
/// it open (see `pie_menu_select`'s doc comment for why).
const CLOSE_INDEX: u32 = 5;

/// What kind of question is currently pending an answer, and what
/// `pie_menu_answer_question` needs to know to answer it — set by
/// `show_question`, taken (read-and-cleared) by `pie_menu_answer_question`
/// so a stray double-invocation is harmless.
enum PendingAnswer {
    /// A `PermissionRequest` — three fixed slots (Allow / Allow-always /
    /// Deny). Answering does NOT simulate any terminal keystroke anymore:
    /// it resolves the real `"http"` hook decision channel directly, via
    /// `permission_server::resolve` (see `permission_server`'s module doc
    /// comment for the whole story — the http hook genuinely works and
    /// returns the allow/deny decision to Claude Code outright). Slot index
    /// maps positionally: 0 = Allow, 1 = Allow-always, 2 = Deny.
    Permission,
    /// An `AskUserQuestion` tool call — `options` is the real listed option
    /// labels (slots `0..options.len()`), then a trailing "answer in
    /// terminal instead" escape-hatch slot at index `options.len()` (see
    /// `show_pending_ask_user_question`). Answering resolves
    /// `permission_server`'s held http connection directly, same as
    /// `Permission` — a listed pick sends `Some(index)` (which
    /// `permission_server::resolve_question` turns into an `updatedInput`
    /// answer), the escape hatch sends `None` (resolves as a deny, which
    /// makes Claude Code fall back to its own native interactive picker).
    AskUserQuestion { options: Vec<String> },
}
static PENDING_ANSWER: std::sync::Mutex<Option<PendingAnswer>> = std::sync::Mutex::new(None);

pub const WINDOW_LABEL: &str = "pie-menu";

/// The arc's total width (its "length" — the straight-line span of the
/// semicircle) as a fraction of the taskbar's (monitor work-area) width, so
/// it reads as a small, mini overlay rather than a wide dock. A first pass
/// at TV/couch-distance ("10-foot UI", à la SteamOS Big Picture) legibility
/// bumped this fraction/clamp range up, but that made the *arc itself* feel
/// oversized — real feedback was that only the question panel's text needed
/// to grow (see QUESTION_PANEL_TITLE_HEIGHT et al. and
/// PieMenu.svelte's matching `.question-title`/`.question-option`), not the
/// arc/icons, so this is back to its original size.
const TASKBAR_WIDTH_FRACTION: f64 = 1.0 / 8.0;
/// Clamp so the arc stays legible on narrow/small-DPI screens and doesn't
/// balloon on ultrawide ones.
const MIN_ARC_WIDTH: f64 = 200.0;
const MAX_ARC_WIDTH: f64 = 420.0;
/// Vertical gap (logical px) left between the arc's bottom edge and the
/// taskbar, so it reads as floating above it rather than flush against it.
const FLOAT_GAP: f64 = 18.0;
/// How much wider than the arc itself the OS window (and therefore the
/// question card) grows in question mode only — requested after real use:
/// the arc's own diameter should stay exactly as-is (see
/// `TASKBAR_WIDTH_FRACTION`'s doc comment on why that was reverted once
/// already), but the card's title/detail text was still cramped at
/// `arc_width`. `reposition` only applies this when `panel_height > 0.0`
/// (i.e. a question is actually showing); a normal open's window stays
/// exactly `arc_width` wide, unaffected.
const QUESTION_PANEL_WIDTH_FRACTION: f64 = 1.35;
const MAX_PANEL_WIDTH: f64 = 560.0;

/// Logical geometry of the current overlay, computed fresh per monitor in
/// `reposition` and sent to the frontend (via the `pie-menu:open` event) so
/// `gui/src/PieMenu.svelte` can size its geometry to match the actual window
/// instead of assuming a fixed constant. `width`/`height` are the arc's own
/// dimensions (unchanged meaning from before question cards could widen the
/// window — still what the SVG/PIVOT math uses). `panel_width` is the actual
/// OS window width, which only exceeds `width` while a question card is
/// showing (see `QUESTION_PANEL_WIDTH_FRACTION`) — the frontend uses it to
/// size `.arc-wrap`/`.question-panel` and to re-center the arc SVG plus its
/// item buttons within the now-wider window. `panel_height` is the extra
/// logical height reserved above the arc for a question's text panel (0
/// outside question mode — see `question_panel_height`). `question` is
/// `None` for a normal open (mic-tap/hotkey) and `Some` when surfacing a
/// pending Claude Code permission-request or `AskUserQuestion` prompt
/// instead — see `show_pending_permission`/`show_pending_ask_user_question`.
#[derive(Clone, serde::Serialize)]
struct ArcGeometry {
    width: f64,
    height: f64,
    panel_height: f64,
    panel_width: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    question: Option<PendingQuestion>,
}

/// Extra logical height (px) reserved above the arc for the question panel —
/// not scaled with the arc's own width like the arc itself is, since legible
/// text needs a stable minimum regardless of monitor size. Computed per
/// question from `question_panel_height` rather than a single fixed
/// constant: the panel now renders the title and each option as their own
/// separate nested pill (`PieMenu.svelte`'s `.question-title`/
/// `.question-option`), so its natural height actually depends on how many
/// options there are — a fixed height sized for the 5-row AskUserQuestion
/// case left a 2-choice permission dialog awkwardly mostly-empty, and a
/// height sized for the 2-choice case truncated longer questions (see this
/// constant's git history: a first version fixed exactly this kind of
/// truncation bug once already, for a *single* growing text block, before
/// the panel had a per-option row layout at all).
// Row/title heights here need to track PieMenu.svelte's own
// .question-title/.question-option font-size + padding — kept in sync by
// hand (see that file's matching comment) since Rust has no way to measure
// the frontend's actual rendered text height ahead of time.
const QUESTION_PANEL_TOP_GAP: f64 = 12.0;
// Sized for up to 3 lines (`.question-title` clamps to 3, larger font/
// padding than an option row — see that rule's comment for why the title
// deliberately gets more room than the options below it).
const QUESTION_PANEL_TITLE_HEIGHT: f64 = 100.0;
const QUESTION_PANEL_GROUP_GAP: f64 = 18.0;
// Sized for the 2-line-wrap worst case (`.question-option` now clamps to 2
// lines instead of truncating to 1 — see that rule's comment) rather than a
// single line; a short one-line option just leaves its own row's reserved
// space partly unused; harmless, since the panel is top-aligned and that
// slack only pushes the (already-deliberately-empty) gap before the arc.
const QUESTION_PANEL_ROW_HEIGHT: f64 = 54.0;
const QUESTION_PANEL_ROW_GAP: f64 = 8.0;
/// Reserved, deliberately unfilled space between the last option pill and
/// the arc itself, so the panel reads as a distinct group floating above
/// the arc rather than sitting flush against it.
const QUESTION_PANEL_ARC_GAP: f64 = 42.0;
/// Extra height reserved for a permission card's monospaced command/detail
/// block (see `PendingQuestion.detail`) — only added for permission mode,
/// which is the only kind that renders it. Generous (a few wrapped lines);
/// the card top-aligns so any slack just widens the gap before the arc.
const QUESTION_PANEL_DETAIL_HEIGHT: f64 = 84.0;

fn question_panel_height(option_count: usize, has_detail: bool) -> f64 {
    let n = option_count.max(1) as f64;
    QUESTION_PANEL_TOP_GAP
        + QUESTION_PANEL_TITLE_HEIGHT
        + if has_detail { QUESTION_PANEL_DETAIL_HEIGHT } else { 0.0 }
        + QUESTION_PANEL_GROUP_GAP
        + n * QUESTION_PANEL_ROW_HEIGHT
        + (n - 1.0).max(0.0) * QUESTION_PANEL_ROW_GAP
        + QUESTION_PANEL_ARC_GAP
}

/// Sent to `PieMenu.svelte` to replace the normal 6-slot layout with one
/// slot per element of `icons`/`labels` (kept parallel — `labels[i]` is the
/// real option text shown in the card above the arc, `icons[i]` is which
/// glyph that slot renders; see `PieMenu.svelte`'s `QUESTION_ICON_MAP` for
/// the icon-key strings). The frontend renders a polished "Claude Code is
/// asking you…" card from these fields:
/// - `kind` — `"permission"` or `"question"`, drives the card's header text
///   and whether `detail` is shown.
/// - `title` — the card's main line: the permission-requesting tool's name
///   (e.g. `"Bash"`), or the actual `AskUserQuestion` question text.
/// - `detail` — permission mode only: the concrete thing being requested
///   (e.g. the shell command), shown monospaced; empty (and hidden) for a
///   question.
#[derive(Clone, serde::Serialize)]
pub struct PendingQuestion {
    kind: &'static str,
    title: String,
    detail: String,
    icons: Vec<String>,
    labels: Vec<String>,
}

/// Set `DJIMIC_DEBUG=1` to log hotkey/selection activity to stderr.
fn debug_enabled() -> bool {
    std::env::var_os("DJIMIC_DEBUG").is_some()
}

/// Create the overlay window once, hidden, so later toggles are just a
/// show/hide instead of paying webview-creation cost on every press.
fn create_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(WINDOW_LABEL).is_some() {
        return Ok(());
    }
    WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::App("index.html".into()))
        .title("")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .focused(false)
        .inner_size(MIN_ARC_WIDTH, MIN_ARC_WIDTH / 2.0)
        .build()?;
    Ok(())
}

/// Force WebView2 to finish standing up its GPU swapchain/compositor surface
/// and finish loading + first-rendering the Svelte bundle right away, at app
/// startup, instead of paying that one-time cost synchronously on the first
/// real Ctrl+Alt+P press (which otherwise stalls noticeably — the overlay
/// window was created hidden and Windows appears to defer that setup until a
/// hidden window is actually shown for the first time). Parks the window far
/// off-screen first so this warm-up show/hide cycle never produces a visible
/// flash at the wrong (unpositioned) spot.
fn warm_up(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    let _ = window.set_position(PhysicalPosition::new(-10_000, -10_000));
    let _ = window.show();

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let app_for_main_thread = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = app_for_main_thread.get_webview_window(WINDOW_LABEL) {
                let _ = window.hide();
            }
        });
    });
}

/// Move the overlay to sit horizontally centered a little above the top of
/// the taskbar (see `FLOAT_GAP`), on whichever monitor currently has the
/// cursor — `work_area` already excludes the taskbar's own space. Resizes
/// the window to `TASKBAR_WIDTH_FRACTION` of that monitor's width (widened
/// further to `QUESTION_PANEL_WIDTH_FRACTION` when `panel_height > 0.0` —
/// i.e. only while a question card needs the extra room, never for a normal
/// open) plus `panel_height` logical px of extra height above the arc (0
/// outside question mode — see `question_panel_height`), so the window grows
/// *upward* off the same floated-above-the-taskbar bottom edge rather than
/// overflowing it. Returns the logical (arc_width, arc_height, window_width)
/// — `window_width` equals `arc_width` outside question mode, and the
/// frontend uses it to size the outer window-bound elements while `width`
/// keeps meaning just the arc's own diameter, same as before panels could
/// widen the window past it.
fn reposition(window: &tauri::WebviewWindow, panel_height: f64) -> Option<(f64, f64, f64)> {
    let monitor = window
        .cursor_position()
        .ok()
        .and_then(|cursor| window.monitor_from_point(cursor.x, cursor.y).ok()?)
        .or_else(|| window.primary_monitor().ok().flatten());
    let monitor = monitor?;

    let area = monitor.work_area();
    let scale = monitor.scale_factor();

    let logical_taskbar_width = area.size.width as f64 / scale;
    let arc_width =
        (logical_taskbar_width * TASKBAR_WIDTH_FRACTION).clamp(MIN_ARC_WIDTH, MAX_ARC_WIDTH);
    let arc_height = arc_width / 2.0;
    let window_width = if panel_height > 0.0 {
        (arc_width * QUESTION_PANEL_WIDTH_FRACTION).clamp(arc_width, MAX_PANEL_WIDTH)
    } else {
        arc_width
    };

    let width = (window_width * scale).round() as u32;
    let height = ((arc_height + panel_height) * scale).round() as u32;
    let gap = (FLOAT_GAP * scale).round() as i32;
    let x = area.position.x + (area.size.width as i32 - width as i32) / 2;
    let y = area.position.y + area.size.height as i32 - height as i32 - gap;

    let _ = window.set_size(PhysicalSize::new(width, height));
    let _ = window.set_position(PhysicalPosition::new(x, y));

    Some((arc_width, arc_height, window_width))
}

fn is_open(window: &tauri::WebviewWindow) -> bool {
    window.is_visible().unwrap_or(false)
}

/// Captures whatever window currently owns OS foreground into
/// `PREVIOUS_FOREGROUND`, so a later action (a normal slot, or answering a
/// pending question) knows where to hand focus back before sending its
/// keystroke — see `pie_menu_select`/`pie_menu_answer_question`.
///
/// Skips the capture entirely (leaving whatever `PREVIOUS_FOREGROUND`
/// already held) if the current foreground window belongs to *this app's
/// own process* — real testing showed a Claude Code question can fire while
/// this app's own main window happens to be what the user last clicked
/// (e.g. checking device/battery status), not their actual terminal. There
/// is never a legitimate reason to target our own window with a simulated
/// keystroke meant for Claude Code's terminal, so capturing it here would
/// just mean every subsequent navigate/confirm action force-focuses our own
/// main window back to the foreground (visible as it repeatedly "popping
/// up") while the real keystrokes go nowhere useful. This doesn't identify
/// the *correct* target window any more precisely than before — it only
/// rules out one specific, always-wrong answer.
#[cfg(windows)]
unsafe fn capture_previous_foreground() {
    let fg = GetForegroundWindow();
    let mut fg_pid = 0u32;
    GetWindowThreadProcessId(fg, Some(&mut fg_pid));
    if fg_pid == GetCurrentProcessId() {
        if debug_enabled() {
            eprintln!(
                "[pie-menu] foreground {:?} title={:?} belongs to this app — not capturing",
                fg.0,
                window_title(fg)
            );
        }
        return;
    }
    PREVIOUS_FOREGROUND.store(fg.0 as isize, Ordering::SeqCst);
    if debug_enabled() {
        eprintln!(
            "[pie-menu] captured previous foreground {:?} title={:?}",
            fg.0,
            window_title(fg)
        );
    }
}

/// Shows, focuses, and announces the overlay's geometry — shared by `open`
/// (normal mic-tap/hotkey open, `panel_height: 0.0`, `question: None`) and
/// `show_question` (`panel_height: question_panel_height(..)`, `question:
/// Some(..)`).
fn show_window(
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
    panel_height: f64,
    panel_width: f64,
    question: Option<PendingQuestion>,
) {
    let _ = window.show();
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            force_foreground(hwnd);
        }
    }
    let _ = window.set_focus();
    let _ = window.emit(
        "pie-menu:open",
        ArcGeometry {
            width,
            height,
            panel_height,
            panel_width,
            question,
        },
    );
    if debug_enabled() {
        eprintln!("[pie-menu] show");
    }
}

/// Positions, shows, focuses, and re-announces the overlay's geometry.
/// Assumes the window isn't already open — callers check `is_open` first.
fn open(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    let Some((width, height, window_width)) = reposition(&window, 0.0) else {
        return;
    };
    #[cfg(windows)]
    unsafe {
        capture_previous_foreground();
    }
    show_window(&window, width, height, 0.0, window_width, None);
}

/// Shared by `show_pending_permission`/`show_pending_ask_user_question`:
/// stashes what `pie_menu_answer_question` needs to answer with
/// (`PENDING_ANSWER`), then shows the overlay with
/// `question_panel_height(..)` reserved for `question`'s card, overriding
/// whatever the overlay was already showing — there's no "already open for
/// something else" case worth preserving once a real prompt needs an
/// answer. Always captures `PREVIOUS_FOREGROUND` regardless of whether the
/// overlay was already open — answering a question means simulating a
/// keystroke into whatever window is showing the real dialog right now,
/// which is only meaningful if it's captured fresh for *this* question, not
/// left over from whenever the overlay last opened for something else.
fn show_question(app: &AppHandle, question: PendingQuestion, answer: PendingAnswer) {
    *PENDING_ANSWER.lock().unwrap() = Some(answer);
    let panel_height = question_panel_height(question.labels.len(), !question.detail.is_empty());
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
            return;
        };
        #[cfg(windows)]
        unsafe {
            capture_previous_foreground();
        }
        let Some((width, height, window_width)) = reposition(&window, panel_height) else {
            return;
        };
        show_window(&window, width, height, panel_height, window_width, Some(question));
    });
}

/// Auto-opens the overlay to show a pending `PermissionRequest`, relayed from
/// `permission_server` (the HTTP decision endpoint — not `hook_bridge`
/// anymore). `tool_name` is the requesting tool (e.g. `"Bash"`) and `detail`
/// the concrete thing being requested (e.g. the shell command), both shown
/// in the redesigned card. Always exactly three fixed slots — Allow /
/// Allow-and-don't-ask-again / Deny — since the decision is now a clean
/// three-way http response, not a variable list of terminal choices. The
/// pick resolves the held http request directly (`permission_server::resolve`
/// from the `Permission` branch of `pie_menu_answer_question`); nothing is
/// typed into any terminal.
pub fn show_pending_permission(app: &AppHandle, tool_name: String, detail: String) {
    show_question(
        app,
        PendingQuestion {
            kind: "permission",
            title: tool_name,
            detail,
            icons: vec!["yes".into(), "dont_ask".into(), "no".into()],
            labels: vec![
                "Allow".into(),
                "Allow, don't ask again".into(),
                "Deny".into(),
            ],
        },
        PendingAnswer::Permission,
    );
}

/// Auto-opens the overlay to show a pending single-select `AskUserQuestion`
/// tool call relayed from `permission_server` (it skips multi-select
/// questions entirely — a single arc pick can't represent "confirm N checked
/// boxes" — see that module's `handle_ask_user_question`). `options` is each
/// listed choice's label text, in the tool call's own order. One synthetic
/// escape-hatch slot is always appended after them: "answer in terminal
/// instead", for anyone who wants to type/speak a genuinely freeform answer
/// rather than pick a listed one — picking it resolves as a deny (see
/// `permission_server::resolve_question`), which makes Claude Code fall back
/// to showing its own native interactive picker.
pub fn show_pending_ask_user_question(app: &AppHandle, question: String, options: Vec<String>) {
    let n = options.len();
    let mut icons: Vec<String> = (0..n).map(|i| format!("n{}", i + 1)).collect();
    icons.push("terminal".into());
    let mut labels = options.clone();
    labels.push("Answer in terminal instead".into());
    show_question(
        app,
        PendingQuestion {
            kind: "question",
            title: question,
            detail: String::new(),
            icons,
            labels,
        },
        PendingAnswer::AskUserQuestion { options },
    );
}

/// Show or hide the overlay. Called from the hotkey thread via
/// `run_on_main_thread`, so this itself always runs on the Tauri main
/// thread.
fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    if is_open(&window) {
        let _ = window.hide();
        if debug_enabled() {
            eprintln!("[pie-menu] hide");
        }
        return;
    }
    open(app);
}

/// Opens the overlay if it's currently closed — does nothing if it's
/// already open (unlike `toggle`, which would close it in that case).
/// Called from `mic_tap.rs`'s detection thread on a double-tap while the
/// menu isn't already showing.
pub fn open_if_closed(app: &AppHandle) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    if is_open(&window) {
        return;
    }
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || open(&app));
}

/// True if the overlay is currently showing — lets `mic_tap.rs` decide
/// whether a tap should navigate within the menu or instead open it.
pub fn is_showing(app: &AppHandle) -> bool {
    app.get_webview_window(WINDOW_LABEL)
        .map(|w| is_open(&w))
        .unwrap_or(false)
}

/// Moves the highlighted slot by `delta` (matching the frontend's own
/// Left/Right arrow handling) — only has an effect while the overlay is
/// actually open. Called from `mic_tap.rs` on a single/double tap (always
/// `delta: 1` in practice — see `mic_tap.rs`'s `finalize_group`).
///
/// Suppressed entirely while a voice-dictation hold (`VOICE_HOLD_ACTIVE`) is
/// running: `mic_tap.rs`'s classifier is tuned against the sharp, brief
/// mechanical sound of an actual tap, but real speech routinely contains
/// similarly sharp high-frequency content (consonants especially) that the
/// classifier still occasionally reads as one — normally rare enough not to
/// matter, but guaranteed to happen repeatedly over the course of the normal-
/// mode voice slot's whole dictation (see `pie_menu_select`'s slot 0, the
/// only remaining use of `VOICE_HOLD_ACTIVE` — `AskUserQuestion` no longer
/// has a voice slot of its own, see `show_pending_ask_user_question`). The
/// Silero VAD gate `mic_tap.rs` already runs during normal speech doesn't
/// cover this case: that gate exists for *ambient* speech happening while
/// the user might also want to tap, not for the hold this app itself just
/// started specifically *to* dictate into.
///
/// Just moves the highlight in the frontend (`pie-menu:move`) — no longer
/// does anything with a pending `AskUserQuestion`'s answer. An earlier
/// version also live-synced every tap to a real arrow-key press into
/// whatever window was showing Claude Code's own terminal picker, so the
/// *real* selection tracked the mic in real time; that entire mechanism (and
/// the focus-stealing back-and-forth it required on every single tap) is
/// gone now that answering resolves `permission_server`'s held http
/// connection directly instead of driving a terminal UI at all — see
/// `permission_server`'s module doc comment.
pub fn navigate(app: &AppHandle, delta: i32) {
    if VOICE_HOLD_ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    if !is_open(&window) {
        return;
    }
    let _ = window.emit("pie-menu:move", delta);
}

/// If the voice-input slot's Win+Ctrl hold is currently active, ends it
/// (releases the keys) and reports that it did. Called by
/// `pairing_button.rs` on every press — a press while this hold is active
/// means "stop dictation" rather than the button's normal confirm meaning.
///
/// Every other slot (see `pie_menu_select`) hands focus back to the overlay
/// itself right after its action completes, so a *following* pairing-button
/// press keeps landing on the still-open menu as a normal Enter/confirm
/// instead of on whatever app the previous action's keystroke went to.
/// Voice input skips that reclaim while the hold is running (dictation needs
/// the target to keep focus), which means this end-of-hold path is the one
/// place responsible for doing it — previously it did not, so after ending
/// dictation the overlay was left permanently unfocused: still visible and
/// still reachable via mic-tap's event-based navigate (which doesn't need
/// focus), but a real Enter/arrow keypress no longer reached its keydown
/// handler at all.
///
/// The reclaim itself is delayed rather than immediate: releasing the
/// Win+Ctrl hold only tells WeChat's voice-to-text to *stop recording* — it
/// still needs a moment afterward (a real network round-trip to WeChat's own
/// speech-to-text backend, not a local/instant operation) to finish
/// transcribing and insert whatever it just heard into the focused chat
/// input box. Pulling focus away from the target too early (confirmed by
/// watching its caret stop blinking right as the pairing button was
/// pressed) lands before that insert happens, so the text never arrives at
/// all. 600ms wasn't enough and still lost it; this is a blind, generous
/// guess at "longer than the round-trip normally takes," not a measured
/// value — if it's still cutting things off (especially for longer
/// sentences, which take the backend longer to transcribe), it needs to go
/// up further.
pub fn end_voice_hold_if_active() -> bool {
    if VOICE_HOLD_ACTIVE.swap(false, Ordering::SeqCst) {
        if debug_enabled() {
            #[cfg(windows)]
            unsafe {
                let fg = GetForegroundWindow();
                eprintln!(
                    "[pie-menu] ending voice input; foreground was {:?} title={:?}",
                    fg.0,
                    window_title(fg)
                );
            }
        }
        key_inject::press_voice_toggle();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(2500));
            if debug_enabled() {
                eprintln!("[pie-menu] reclaiming overlay focus after voice input");
            }
            reclaim_focus_if_open();
        });
        true
    } else {
        false
    }
}

/// Hands OS focus back to the overlay if it's currently open — a no-op
/// otherwise (in particular, never shows/focuses a closed overlay; only
/// `open` does that).
fn reclaim_focus_if_open() {
    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
            return;
        };
        if !is_open(&window) {
            return;
        }
        #[cfg(windows)]
        if let Ok(hwnd) = window.hwnd() {
            unsafe {
                force_foreground(hwnd);
            }
        }
        let _ = window.set_focus();
    });
}

/// Called by `pairing_button.rs` for its "every press = Enter" confirm,
/// instead of it simulating a bare `key_inject::press_enter()` straight
/// away. The overlay is only ever real-focused at the moment it's shown
/// (`show_window`) — anything else grabbing OS focus afterward (routine
/// while a person takes a moment to read/decide on a pending question, see
/// `onBlur` in `PieMenu.svelte`) leaves it still open but no longer
/// focused, so a bare Enter keystroke would land wherever else that focus
/// went instead of the overlay's own keydown handler, with no visible
/// feedback that it did nothing. Explicitly reclaiming focus first,
/// synchronously before the Enter within the same main-thread callback (not
/// a separate `reclaim_focus_if_open()` call followed by a same-thread
/// `press_enter()` — that would race the main-thread focus change against
/// this call returning), makes the confirm land reliably regardless of
/// what's had focus since the overlay was shown. A no-op window (not open)
/// just falls through to the plain Enter, matching the pre-existing
/// behavior for every other real use of the pairing button (e.g. ending a
/// voice hold, or no menu open at all).
pub fn confirm_via_button() {
    let Some(app) = APP_HANDLE.get() else {
        key_inject::press_enter();
        return;
    };
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            if is_open(&window) {
                #[cfg(windows)]
                if let Ok(hwnd) = window.hwnd() {
                    unsafe {
                        force_foreground(hwnd);
                    }
                }
                let _ = window.set_focus();
            }
        }
        key_inject::press_enter();
    });
}

/// Hide the overlay without picking anything — the frontend calls this after
/// its own close animation finishes (Escape, or losing focus).
#[tauri::command]
pub fn pie_menu_close(app: AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.hide();
    }
}

/// Called by `hook_bridge` on `AskUserQuestion`'s `PostToolUse` — the tool
/// call has resolved, however that actually happened. If `PENDING_ANSWER`
/// is still `Some(AskUserQuestion { .. })` at this point, the pie menu's
/// own answer path never ran (that path always takes/clears it the moment
/// the user picks something *there* — see `pie_menu_answer_question`), so
/// the question must have been answered some other way (typed or clicked
/// directly in the terminal). In that case the overlay, if still open, is
/// now showing a stale question with nothing left to confirm — hide it and
/// clear the stale state so a later pairing-button press doesn't try to
/// act on it. A no-op if the pie menu's own path already handled it (the
/// overwhelmingly common case), or if nothing was pending at all.
pub fn question_answered_externally(app: &AppHandle) {
    let mut pending = PENDING_ANSWER.lock().unwrap();
    if !matches!(*pending, Some(PendingAnswer::AskUserQuestion { .. })) {
        return;
    }
    *pending = None;
    drop(pending);
    if debug_enabled() {
        eprintln!("[pie-menu] AskUserQuestion answered outside the pie menu — closing stale overlay");
    }
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.hide();
    }
}

/// Called by `permission_server` when a request's own wait times out (the
/// user never picked) and nothing else is left queued behind it — clears
/// whatever's pending (`Permission` or `AskUserQuestion`) and dismisses the
/// overlay. The server has already resolved its side (denied); this just
/// cleans up the UI. Safe no-op if the user already picked (pie menu cleared
/// `PENDING_ANSWER` itself) or nothing is pending. Runs the hide on the main
/// thread since it's invoked from the server's own connection thread.
pub fn force_close_permission(app: &AppHandle) {
    {
        let mut pending = PENDING_ANSWER.lock().unwrap();
        if pending.is_none() {
            return;
        }
        *pending = None;
    }
    if debug_enabled() {
        eprintln!("[pie-menu] request timed out — closing stale overlay");
    }
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            let _ = window.hide();
        }
    });
}

/// A pending question (see `permission_server::show_pending_permission` /
/// `show_pending_ask_user_question`) was answered with slot `index`.
/// Branches on what kind of question is pending (`PENDING_ANSWER`, taken —
/// read-and-cleared, so a stray double-invocation is harmless). Both kinds
/// resolve `permission_server`'s held http connection directly — no
/// terminal keystroke, no focus stealing, no delay:
///
/// - `Permission`: the three fixed slots map positionally — 0 = Allow,
///   1 = Allow-and-don't-ask-again, 2 = Deny (`permission_server::resolve`).
/// - `AskUserQuestion`: `index < options.len()` answers with that listed
///   option (`permission_server::resolve_question(Some(index))`, which
///   turns into an `updatedInput` Claude Code treats as if the user had
///   answered exactly that); the trailing escape-hatch slot
///   (`index == options.len()`) sends `None`, which resolves as a deny so
///   Claude Code falls back to showing its own native interactive picker —
///   see `permission_server`'s module doc comment for the whole mechanism
///   (this is what replaced the old arrow-key/Enter keystroke simulation).
///
/// Every branch closes the overlay — a pending question is answered once,
/// there's no "leave it open, repeat" behavior like the normal 6-slot menu
/// has for e.g. repeated Down presses.
#[tauri::command]
pub fn pie_menu_answer_question(app: AppHandle, index: u32) {
    if debug_enabled() {
        eprintln!("[pie-menu] answering pending question with choice {index}");
    }
    let pending = PENDING_ANSWER.lock().unwrap().take();
    match pending {
        None => pie_menu_close(app),
        Some(PendingAnswer::Permission) => {
            let decision = match index {
                0 => crate::permission_server::Decision::Allow,
                1 => crate::permission_server::Decision::AllowAlways,
                _ => crate::permission_server::Decision::Deny,
            };
            crate::permission_server::resolve(decision);
            pie_menu_close(app);
        }
        Some(PendingAnswer::AskUserQuestion { options }) => {
            let pick = if (index as usize) < options.len() {
                Some(index)
            } else {
                None
            };
            crate::permission_server::resolve_question(pick);
            pie_menu_close(app);
        }
    }
}

/// A slot was confirmed (Enter, pairing-button press, or a click). Slots:
/// 0 voice input (starts a Win+Ctrl hold, ended later by a pairing-button
/// press — see `end_voice_hold_if_active`), 1 Down arrow, 2 Up arrow,
/// 3 Enter, 4 types "/btw ", 5 closes the menu (`CLOSE_INDEX`).
///
/// Only `CLOSE_INDEX` actually closes the overlay — every other slot leaves
/// it open, so pressing e.g. "down" several times in a row doesn't require
/// reopening the menu each time. That does mean the simulated keystroke
/// itself needs somewhere else to land: focus is handed back to whatever
/// window was in the foreground before the menu opened
/// (`PREVIOUS_FOREGROUND`, captured in `open`) right before sending it.
///
/// For the one-shot actions (1-4) focus is then reclaimed by the overlay
/// right after, so the pairing button's simulated Enter keeps landing on
/// the still-open menu for the *next* confirm instead of on whatever app
/// just received the actual keystroke. Voice input (0) is deliberately
/// different: it isn't a one-shot keystroke, it's an ongoing dictation that
/// keeps feeding recognized text to whatever window currently has focus for
/// as long as the hold lasts — reclaiming focus back to the overlay right
/// after starting it would immediately redirect all of that dictated text
/// to this focus-less overlay instead of the actual target input box, so
/// for slot 0 focus is left on the target app until the hold ends (a later
/// pairing-button press, via `end_voice_hold_if_active`, which doesn't need
/// window focus at all).
#[tauri::command]
pub fn pie_menu_select(app: AppHandle, index: u32) {
    if debug_enabled() {
        eprintln!("[pie-menu] selected slot {index}");
    }
    if index == CLOSE_INDEX {
        pie_menu_close(app);
        return;
    }
    std::thread::spawn(move || {
        #[cfg(windows)]
        unsafe {
            let prev = PREVIOUS_FOREGROUND.load(Ordering::SeqCst);
            if prev != 0 {
                if debug_enabled() {
                    eprintln!(
                        "[pie-menu] restoring foreground to {:#x} title={:?}",
                        prev,
                        window_title(HWND(prev as _))
                    );
                }
                force_foreground(HWND(prev as _));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
        match index {
            0 => {
                if debug_enabled() {
                    #[cfg(windows)]
                    unsafe {
                        let fg = GetForegroundWindow();
                        eprintln!(
                            "[pie-menu] starting voice input; foreground is now {:?} title={:?}",
                            fg.0,
                            window_title(fg)
                        );
                    }
                }
                key_inject::press_voice_toggle();
                VOICE_HOLD_ACTIVE.store(true, Ordering::SeqCst);
                VOICE_HOLD_STARTED_AT_MILLIS.store(now_millis(), Ordering::SeqCst);
                return;
            }
            1 => key_inject::press_down_arrow(),
            2 => key_inject::press_up_arrow(),
            3 => key_inject::press_enter(),
            4 => key_inject::type_text("/btw "),
            _ => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
        let _ = app.clone().run_on_main_thread(move || {
            if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
                #[cfg(windows)]
                if let Ok(hwnd) = window.hwnd() {
                    unsafe {
                        force_foreground(hwnd);
                    }
                }
                let _ = window.set_focus();
            }
        });
    });
}

/// Runs for the lifetime of the app, polling once a second: if
/// `VOICE_HOLD_ACTIVE` has been set for longer than `VOICE_HOLD_TIMEOUT`,
/// auto-clears it — see `VOICE_HOLD_ACTIVE`'s own doc comment for why this
/// exists: this app has no way to observe whether the target app is actually
/// still recording, so a press-based end via `end_voice_hold_if_active` is
/// the *only* other way this flag ever clears, and real use surfaced that
/// the user doesn't always end dictation that way (e.g. stopping it directly
/// in the target app instead) — without this, the flag then stays stuck
/// `true` forever, which both silently swallows every further mic-tap
/// (`navigate`'s own gate) and makes the *next* pairing-button press get
/// misrouted into `end_voice_hold_if_active`'s "stop dictation" handling
/// instead of acting as a normal confirm.
fn spawn_voice_hold_watchdog() {
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if !VOICE_HOLD_ACTIVE.load(Ordering::SeqCst) {
            continue;
        }
        let started = VOICE_HOLD_STARTED_AT_MILLIS.load(Ordering::SeqCst);
        if started == 0 || now_millis().saturating_sub(started) < VOICE_HOLD_TIMEOUT.as_millis() as u64 {
            continue;
        }
        VOICE_HOLD_ACTIVE.store(false, Ordering::SeqCst);
        if debug_enabled() {
            eprintln!(
                "[pie-menu] voice input auto-reset after {}s with no ending press",
                VOICE_HOLD_TIMEOUT.as_secs()
            );
        }
    });
}

#[cfg(windows)]
pub fn spawn(app: AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
    if let Err(e) = create_window(&app) {
        eprintln!("[pie-menu] failed to create overlay window: {e}");
        return;
    }
    warm_up(&app);
    spawn_voice_hold_watchdog();
    std::thread::spawn(move || win32::run(app));
}

#[cfg(not(windows))]
pub fn spawn(_app: AppHandle) {}

/// Unregisters the global hotkey, restoring `K` to normal typing. Called on
/// app quit.
#[cfg(windows)]
pub fn uninstall() {
    win32::stop();
}

#[cfg(not(windows))]
pub fn uninstall() {}

#[cfg(windows)]
mod win32 {
    use super::{toggle, AppHandle};
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_P,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetWindowLongPtrW, PostMessageW, PostQuitMessage, RegisterClassExW, SetWindowLongPtrW,
        TranslateMessage, CW_USEDEFAULT, GWLP_USERDATA, HWND_MESSAGE, MSG, WM_CLOSE, WM_DESTROY,
        WM_HOTKEY, WNDCLASSEXW, WNDCLASS_STYLES,
    };

    const HOTKEY_ID: i32 = 1;

    /// The hidden message-only window's handle, so `stop()` can ask its
    /// message loop (running on a different thread) to shut down.
    static WINDOW: AtomicIsize = AtomicIsize::new(0);

    pub fn run(app: AppHandle) {
        unsafe {
            let Ok(hinstance) = GetModuleHandleW(None) else {
                return;
            };
            let class_name = w!("DjiMicPieMenuWnd");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: WNDCLASS_STYLES::default(),
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            // Ignore failure: benign if this ever runs twice and the class
            // is already registered.
            RegisterClassExW(&wc);

            let Ok(hwnd) = CreateWindowExW(
                Default::default(),
                class_name,
                w!(""),
                Default::default(),
                0,
                0,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                Some(HWND_MESSAGE),
                None,
                Some(hinstance.into()),
                None,
            ) else {
                return;
            };

            let app_ptr = Box::into_raw(Box::new(app));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, app_ptr as isize);
            WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);

            let _ = RegisterHotKey(
                Some(hwnd),
                HOTKEY_ID,
                MOD_NOREPEAT | MOD_CONTROL | MOD_ALT,
                VK_P.0 as u32,
            );

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            WINDOW.store(0, Ordering::SeqCst);
            drop(Box::from_raw(app_ptr));
        }
    }

    /// Ask the message loop to exit, which unregisters the hotkey.
    pub fn stop() {
        let raw = WINDOW.load(Ordering::SeqCst);
        if raw != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(raw as _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_HOTKEY => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppHandle;
                if !ptr.is_null() {
                    let app = (*ptr).clone();
                    let _ = app.clone().run_on_main_thread(move || toggle(&app));
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = UnregisterHotKey(Some(hwnd), HOTKEY_ID);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
