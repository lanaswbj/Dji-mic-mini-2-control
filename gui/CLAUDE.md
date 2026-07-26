# gui/

The shipped product: Tauri 2 backend (`src-tauri`) + Svelte 5 frontend (`src`). Build and dev commands
are in the root CLAUDE.md.

## Backend (`src-tauri`)

`main.rs` wires up the Tauri app — single-instance, autostart, close-to-tray (hides instead of
quitting; the tray "quit" item is the real exit path), and a tray icon reflecting live device/battery
state. Composited into its own top-left corner (the opposite corner from the device badges — the two
aren't mutually exclusive) is a coarse Claude Code idle/thinking/working/error/attention status from
`claude_status.rs`, a tiny OS-agnostic atomic updated by hook events relayed from `hook_bridge.rs`.
That status is deliberately last-write-wins rather than a precise per-session state machine, because
Claude Code's hooks are fire-and-forget with no request/response correlation.

`commands.rs` holds the general snapshot/set-setting commands plus `app_info`/`set_autostart` for the
偏好设置 screen. `app_info` reads the version, autostart state, `pie_menu::HOTKEY_LABEL` and the two
loopback `PORT` constants **from the modules that own them** rather than letting the frontend restate
them; `pie_menu::pie_menu_slots` does the same for the six fixed slot descriptions, declared right
beside the `match index` in `pie_menu_select` that implements them, so the UI cannot describe a slot as
doing something it doesn't.

Windows-only concerns get their own modules:

- **`driver.rs`** — one-click WinUSB driver install for the vendor control interface. A hand-rolled INF
  can't install under Code Integrity enforcement without a signed catalog, so this downloads the
  official signed Zadig release, verifies its Authenticode signature, pre-seeds `zadig.ini` so it opens
  in the right state, and deletes it afterward. It does not try to reimplement Zadig's catalog-signing.

- **`pairing_button.rs`** — reads the pairing button via the Win32 Raw Input API (`RIDEV_INPUTSINK`).
  `RIDEV_NOLEGACY` was tried to also suppress Windows' own default handling of that HID collection (an
  automatic system-volume change on every press) but is only valid for the Generic Desktop usage page
  (mouse/keyboard) — using it on the Consumer Control page makes `RegisterRawInputDevices` fail
  outright, **silently**, since the original code didn't check the return value. That meant detection
  itself was broken too, not just the volume suppression; both bugs shipped together undetected for a
  while. A `WH_KEYBOARD_LL` hook doesn't work for the suppression either — Windows' default handling
  for HID consumer-page usages happens below the synthesized-keystroke layer. The volume side effect is
  therefore handled after the fact by `volume_guard.rs`, not solved at the input level.
  Every press also unconditionally simulates an Enter keypress (via `key_inject.rs`) as a
  general-purpose "pairing button = Enter" remap — that is what lets the pairing button confirm the pie
  menu without `pie_menu.rs` needing to know the button exists.

- **`volume_guard.rs`** — neutralizes that volume side effect: continuously snapshots the default
  output device's volume/mute state while idle and forces it back shortly after every press, and
  separately keeps the volume OSD popup hidden for as long as the app runs. (A press-triggered
  one-shot version of the OSD suppression was tried first but still flashed occasionally, for reasons
  never pinned down — see the root CLAUDE.md's open items.)

- **`mic_tap.rs`** — detects 1–2 taps on the mic shell as an audio-domain gesture (3+ in a burst still
  report as a double tap). Feeds per-chunk features into the trained classifier (see
  `crates/tap-model/CLAUDE.md`), gated by `earshot` VAD so a sharp consonant or loud exclamation
  mid-speech doesn't slip through. `MicTapWatcher` holds the live `Arc<TapModelStore>` (swapped by a
  background thread watching `tap_model.json`'s mtime — `spawn_model_poll` — *and* by
  `tap_feedback.rs`'s incremental trainer, from two entirely different threads, with no locking on the
  audio-thread read path), an `Arc<tap_feedback::FeedbackRing>` (every chunk's raw measurements, pushed
  unconditionally *before* any suppression/floor branching), and `last_group_taps` (confirm instants of
  the most recently finalized group, for false-positive targeting). Hard amplitude/ratio/crest floors
  sit on top of the model as a last-resort safety net, plus a short confirm-delay/decay-check
  (`TAP_CONFIRM_DELAY`/`SUSTAIN_DECAY_FRACTION`) that rejects continuous noise (blowing, rubbing) which
  a single chunk's snapshot could otherwise mistake for an impact. The module doc comment has the full
  per-chunk pipeline order.

- **`tap_feedback.rs`** — incremental training driven by in-app user feedback, built on `mic_tap.rs`'s
  `FeedbackRing`. Two Tauri commands (`mic_tap_report_false_positive`/`…_false_negative`, wired to
  buttons in `src/lib/sections/InputGestures.svelte`) locate the actual acoustic event in the ring
  buffer — a false positive looks backward from the confirmed group's instants minus
  `TAP_CONFIRM_DELAY`; a false negative scans the last few seconds for the loudest chunk above a
  relaxed sanity floor — append it correctly labeled to a per-user CSV (`tap_feedback.csv`, same schema
  as detect-test's `samples.csv` so a future full retrain can fold it back in), then kick off a bounded
  warm-started retrain (`tap_model::continue_training`) on a background thread: a handful of low-LR
  epochs from the *live* model's weights, using the new rows (up-weighted by repetition) plus a bulk
  "background" replay sample drawn straight from whatever is currently in the ring buffer — real,
  this-room, this-hardware ambient audio, deliberately not a bundled dataset, so nothing extra has to
  ship. Before touching the live model the candidate must have all-finite weights **and** must not
  regress "none"-class accuracy on that same replay sample past a small tolerance (a cheap proxy for
  "did this just make false triggers on ordinary noise more likely"). A rejected candidate is
  discarded, but its CSV rows remain for a later attempt or a full offline retrain.
  `mic_tap_training_status` (polled by the UI) reports model source/age/row-count/pending-feedback
  count; `mic_tap_rollback_model`/`mic_tap_restore_factory_model` restore the pre-update backup
  (`tap_model.json.bak`) or the embedded baseline, each backing up the about-to-be-replaced file first,
  so "restore factory" is itself undoable.

- **`shortcut.rs`** — permanent stub, always unavailable on Windows. See the root CLAUDE.md.

- **`pie_menu.rs`** — a global Ctrl+Alt+P hotkey (`RegisterHotKey`; an earlier version bound bare `K`,
  which made that letter untypeable system-wide, before settling on a combo with no known collision)
  toggling a borderless, transparent, always-on-top overlay docked above the taskbar: an LG
  webOS-style fan/pie menu. Arrow keys (or a mic tap) move the highlight, Enter (or the pairing button)
  confirms, Escape or losing focus cancels. Like `pairing_button.rs`, the hotkey is registered on a
  hidden message-only window with its own `GetMessageW` loop on a dedicated thread; `WM_HOTKEY` hops
  back onto the Tauri main thread via `run_on_main_thread` before touching the window, since
  window/webview calls aren't meant to be made from arbitrary threads. The overlay is created once at
  startup and only shown/hidden/repositioned afterward, never rebuilt per toggle.
  The six fixed slots (`pie_menu_select`): voice-dictation hold (Win+Ctrl, held until a later
  pairing-button press ends it), Down, Up, Enter, types `"/btw "`, close. Beyond that,
  `permission_server.rs` can override what's showing with a pending Claude Code question — a
  `PermissionRequest` choice or a single-select `AskUserQuestion` — with real title/option text in a
  panel above the arc; an `AskUserQuestion`'s implicit freeform "Other" reuses the voice-dictation slot
  instead of a keypress. Fan geometry lives in `src/PieMenu.svelte` — a half-circle "dome", items
  placed along the upper arc by simple trig, with the question panel's `PIVOT_X`/`PIVOT_Y` split from
  the arc's own radius so a panel can grow without moving the arc.

- **`key_inject.rs`** — `SendInput`-based keystroke simulation, so a pie-menu slot acts on whatever
  application currently has focus, the same mechanism a hardware keyboard uses. `hold_win_ctrl_start`/
  `hold_win_ctrl_end` are separate key-down-only/key-up-only calls rather than one press-and-release
  function, because the Win+Ctrl voice-dictation hold spans an arbitrarily long gap between two
  unrelated events (the slot that starts it, a later pairing-button press that ends it). `type_text`
  uses Unicode key events rather than virtual-key codes so it isn't limited to characters with a
  virtual key on the current keyboard layout.

- **`hook_bridge.rs`** — a loopback-only TCP listener (`127.0.0.1:47215`) bridging Claude Code's
  *non-permission* lifecycle hook events (`PreToolUse`/`PostToolUse`/`Stop`/etc.) into the pie menu and
  `claude_status.rs`. `~/.claude/settings.json` (outside this repo) registers a `"command"`-type hook
  per event that reads the event JSON off stdin and forwards it unmodified via a PowerShell one-liner
  reading stdin as explicit UTF-8, so non-ASCII payloads survive. Two jobs: (1) an `AskUserQuestion`'s
  `PostToolUse` event auto-dismisses a stale pie-menu question overlay if it was answered some other
  way (typed in the terminal, or `permission_server.rs`'s own "answer in terminal instead" escape
  hatch) — a no-op if the pie menu already cleared its own pending state; (2) every event with a
  `hook_event_name` updates the coarse tray-icon status. `HookPayload`'s fields are all `Option<T>`
  with no `deny_unknown_fields`, so one struct parses every event type without one event's payload
  shape breaking another's. **Does not handle permission decisions at all anymore.**

- **`permission_server.rs`** — a loopback-only **HTTP** server (`127.0.0.1:47216/permission`, one port
  up from `hook_bridge.rs` so the two never collide) registered in `~/.claude/settings.json` as an
  `"http"`-type `PermissionRequest` hook. This is the real allow/deny decision channel, not a
  notification relay. An earlier iteration of this project believed the `"http"` hook type never
  actually delivered real `PermissionRequest` events, and built `hook_bridge.rs`'s keystroke-simulation
  fallback specifically to work around that; a later from-scratch probe proved that belief **wrong** (or
  a newer Claude Code fixed it). The `"http"` hook genuinely works, and a real decision response is
  honored with zero terminal interaction. Key points, since this reverses what older notes said:
  - `AskUserQuestion` fires this exact same `PermissionRequest` hook (confirmed empirically, documented
    nowhere) rather than some separate mechanism — a plain tool needing Allow/Deny and an interactive
    multi-choice question arrive at the identical endpoint, distinguished only by
    `tool_name == "AskUserQuestion"`.
  - The response body must be the `{"hookSpecificOutput":{"hookEventName":"PermissionRequest",
    "decision":{"behavior":"allow"|"deny",...}}}` envelope — only that shape's `updatedInput` field
    lets an `AskUserQuestion` answer be supplied directly (`answers: {question_text: chosen_label}`)
    instead of falling back to Claude Code's terminal picker. That is what makes answering from the pie
    menu possible with **no keystroke injection and no focus stealing at all**.
  - `QUEUE` (a `Mutex<VecDeque<PendingRequest>>`, only the front item ever shown via `show_front`)
    exists because requests routinely overlap in practice — a Claude Code session working on this repo
    is itself a constant client of this server, so a human's separate session can easily have its own
    request in flight simultaneously. An earlier single-`Option`-slot design let a second arrival
    silently overwrite the first, surfacing as two real bugs at once: "Allow" appearing to do nothing
    (the visible card's request had already been denied out from under it), and the overlay
    flashing/re-focusing on every new arrival even when one was already shown.
  - **No `permission_mode`-based gating.** An earlier version auto-allowed everything except `default`
    mode, on the theory that only `default` needs a human decision; real use falsified that — a Bash
    command in `acceptEdits` mode (which only auto-accepts *file edits*) still fired this hook
    expecting a real answer. Every request now pops the pie menu and waits, full stop: Claude Code
    firing the hook at all already means it wants an answer.
  - "Allow, don't ask again" persists matching `permission_suggestions` rules into
    `{cwd}/.claude/settings.local.json`'s `permissions.allow` array — the same file/format Claude
    Code's own `destination: "localSettings"` suggestions target. Best-effort: a failed
    read/merge/write still lets the already-decided allow proceed, just without being remembered.

## Frontend (`src`)

Svelte 5 (runes), poll-based throughout. There is no Tauri event emission anywhere in the codebase
*except* `pie-menu:open`, which `pie_menu.rs` emits to reset the overlay's selection each time it is
shown. `main.js` mounts one of two root components depending on `getCurrentWindow().label`: `App` for
`"main"`, `PieMenu` for `"pie-menu"` — **both windows load the same bundle**, so anything window-scoped
(the appearance preference, for one) has to be applied by *both* roots. `lib/api.js` is the thin
`invoke()` wrapper layer; add new Tauri commands there rather than calling `invoke` directly from a
component. All copy is Simplified Chinese.

The main window is a **sidebar-navigated, section-based app**, not a tabbed one. See
`design-system/dji-mic-control/MASTER.md` for the design system that `app.css` and `lib/ui/`
implement, including a table mapping each of its sections to the code implementing it.

The window is **transparent with a Windows 11 Mica backdrop** (`transparent: true` +
`windowEffects` in `tauri.conf.json`), so `body` is `background: transparent` and the shell paints
every pixel itself at three material weights — sidebar thinnest, content plane thickest. Two
consequences worth knowing before touching a background anywhere: an opaque surface anywhere in that
chain cancels the effect, and cards stay opaque *on purpose* (a translucent card over the
translucent content plane is the one stack Apple's material rule forbids). `lib/glass.svelte.js`
owns the on/off preference and must always move both halves together — the `data-translucent`
attribute that flattens the `--glass-*` alphas, **and** Tauri's `setEffects`/`clearEffects`; either
one alone is visibly wrong (Mica nobody can see, or an unblurred see-through window).

- **`lib/nav.js`** — the navigation model. Two tiers: **设备** (概览 → one section per protocol setting
  *group* → 设备信息) and **应用** (敲击与按键 / 快捷菜单 / 偏好设置). The middle of the 设备 tier is
  data-driven from `Setting.group`, so a new model declaring a new group gets a section without anyone
  editing the frontend. `Ctrl+1..9` jumps to a section, `Ctrl+B` toggles the sidebar, `Ctrl+,` opens
  preferences, `Ctrl+R` forces a refresh.
- **`lib/store.svelte.js`** — the single owner of device state (`devices`, a rune-based class): the
  snapshot poll, the v2 carry-forward merge (`mergeStatus`/`mergeTx`/`mergeRx`), optimistic writes, and
  `lockReason`. Four behaviors matter, each fixing a real defect in the pre-redesign build:
  - The poll **pauses when the window is hidden** — closing to tray used to leave a 250ms USB poll
    running forever. Otherwise it runs at `TEMPO.live` (250ms, only while 概览 with its live meters is
    showing) or `TEMPO.calm` (1s).
  - Every optimistic value has a **3s deadline**, after which it reverts rather than leaving the UI
    permanently showing a value the hardware doesn't have.
  - `writeState(id)` exposes idle/writing/ok/error so a row can show a write in flight, and every
    failure also raises a toast carrying the reason plus a retry. The toast is raised in the store, not
    the calling component, because the timeout path has no caller to return to — nobody is awaiting it
    three seconds later.
  - Per-transmitter noise-cancel values are **deliberately never retired by a poll**: the receiver
    mirrors them into both slots, so a frame can't confirm one slot alone. That is exactly why
    `#forget()` has to clear them on a device swap — otherwise they'd describe the new receiver's
    transmitters using the old one's state.
- **`lib/ui/`** — the shared primitives, and the only place a control's markup and CSS may live.
  `Icon`'s `<script module>` holds the app's only icon table; there are no emoji anywhere in the UI.
  Errors are toasts, never in-flow banners — a banner reflowed the whole page at the worst possible
  moment. No component may hardcode a color, spacing, radius, duration or font size; every one comes
  from a custom property in `app.css`. Three traps here have each already cost a real defect:
  - `Icon`'s svg is `display: inline-block`, not `block`. A block-level SVG dropped into an inline
    context takes its own line, which is what put 概览's「全部音频设置」chevron on a second row. A
    trailing glyph on a `Button` goes through `iconEnd`, never into `children`.
  - Responsive rules are **container queries** against `.content` (`@container content (…)`), not
    viewport media queries. At the 760px minimum window width an open sidebar leaves the reading
    column ~520px wide, and `@media (max-width: 640px)` cannot see that — which is exactly how
    `Row`'s stacking breakpoint failed to fire at the size it existed for.
  - An icon leading text that can wrap uses `.u-icon-line` (app.css). Four components had each
    grown an `align-items: center` copy, correct on one line and visibly wrong on two.
- **`lib/sections/`** — one component per navigation entry, all wrapped in `Section.svelte` (sticky
  translucent header + one measured column). `InputGestures.svelte` is the old 接收器快捷键 tab, renamed
  after what it actually does and now explaining that both gestures feed the pie menu; it hosts the
  mic-tap feedback panel described under `tap_feedback.rs` above, with restore-factory behind a
  confirmation `Dialog`. `QuickMenu.svelte` and `Preferences.svelte` are surfaces for things that
  previously had no in-window presence at all: the pie menu's hotkey and slot list, and autostart /
  close-to-tray / the tray badge legend / the two loopback ports the Claude Code integration binds.
