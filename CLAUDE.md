# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Windows desktop control app for DJI wireless microphones (DJI Mic Mini / Mini 2), forked from an
upstream cross-platform project and re-scoped to Windows only. macOS-specific features that depended
on BlackHole (virtual audio device) and CoreAudio (Voice Comfort real-time processing, in-app audio
device switching) were removed entirely rather than ported. The `protocol`/`device`/`cli` crates keep
their cross-platform structure (Linux udev packaging files still exist under `packaging/`) but only
Windows is actively built and shipped — see `README.md` for the user-facing feature list.

## Commands

Rust workspace (`protocol`, `device`, `cli`, `gui/src-tauri`):

```bash
cargo check --workspace          # fast type-check, do this before anything heavier
cargo build --workspace
cargo test -p protocol           # crc.rs, packet.rs, models/mic_mini.rs
cargo test -p tap-model          # model forward-pass/training tests, incl. a numeric gradient check
cargo test -p protocol <name>    # run a single test
cargo deny check                 # enforce the license allowlist in deny.toml
```

Mic-tap model retraining (`test-tools/detect-test`, standalone crate outside the workspace — see
"Project layout" below):

```bash
cd test-tools/detect-test
cargo run --release -- collect             # ~4 min: quiet/speech/loud-speech/noise/nail-tap/pad-tap phases
cargo run --release -- collect-extra       # ~2 min: pairing-button press + blowing (hard negatives)
cargo run --release -- collect-friction    # ~1 min: finger rubbing the shell (hard negative)
cargo run --release -- train               # fits a new model, writes it to %APPDATA%\org.djimic.control\tap_model.json
cargo run --release -- train --bake-default  # also overwrites crates/tap-model/default_model.json (the checked-in baseline for fresh installs)
cargo run --release                        # (no subcommand) live detection console + pairing-button listener, no GUI
```

`train` on its own hot-swaps the *currently running* app's model (if any) within ~3s via its
file-poll thread — no rebuild, no restart. `--bake-default` is a separate, deliberate step for a
maintainer updating what ships in the installer; run it only when a training run should become the
new out-of-the-box default, not on every local retrain.

CLI tool (one-shot; connects, acts, exits — see its own doc comment in `crates/cli/src/main.rs`):

```bash
cargo run -p cli -- list
cargo run -p cli -- status --json
cargo run -p cli -- set noise-cancel strong --tx 1
```

GUI (Tauri 2 + Svelte 5), from `gui/`:

```bash
npm install
npm run tauri dev                # full app: Rust backend + Svelte frontend, hot reload
npm run dev                      # Vite dev server only, no Tauri backend/device access
npm run build                    # frontend build only (also run automatically by `tauri build`)
```

Windows release build — use `.\build-release.ps1` from the repo root, not a raw `tauri build`. It
wraps `npx tauri build --bundles nsis`, then scrubs and **verifies** (fails the build otherwise) that
no build-machine home directory, username, or hostname leaked into the compiled binary via
`--remap-path-prefix`, since `strip` alone doesn't remove those from panic/backtrace strings. Produces
`Release\windows\DJI Mic Control.exe` (portable) and the NSIS installer. `build-release.sh` is the
Linux/macOS-upstream equivalent and explicitly rejects Darwin.

**Windows toolchain gotcha**: building requires the MSVC toolchain (VS Build Tools, `link.exe`) on
`PATH`, which a plain PowerShell/Git Bash session usually doesn't have — source it from
`VsDevCmd.bat -arch=x64 -host_arch=x64` first. Also run Rust/MSVC builds through the PowerShell tool,
not Git Bash: Git Bash's coreutils `link` shadows MSVC's `link.exe` on `PATH` and produces confusing
link failures.

## Architecture

**Layering**: `crates/protocol` (pure logic, no I/O: framing, CRC, per-model command/decode) →
`crates/device` (USB transport + multi-device orchestration, depends on `protocol`) → front-ends
`crates/cli` and `gui/src-tauri` (both depend on `device` only, sharing its blocking API). Adding a
new microphone model means implementing the `DeviceModel` trait in `crates/protocol/src/models/` and
adding it to the `MODELS` registry (`crates/protocol/src/models/mod.rs`) — nothing in `device`,
`cli`, or the GUI needs to change. `crates/tap-model` is a second, unrelated shared crate — it has
nothing to do with the DJI wire protocol; see its own paragraph below.

**`crates/tap-model`** — the mic-shell-tap classifier's model format, forward pass, and training,
shared between `gui/src-tauri` (inference, at runtime) and `test-tools/detect-test` (training, offline)
so the two can never drift apart the way hand-copied `const` weight arrays used to. A plain Cargo path
dependency, not a workspace member relationship on detect-test's side — see "Project layout" for why
that distinction matters.
- `src/lib.rs` — `TapModel` (the trained artifact: weights, per-feature z-score `mean`/`std`,
  `class_names`, `confidence_threshold`, `feature_schema_version`, and metadata like `source`/
  `trained_at_unix_ms`/`training_rows`), its JSON (de)serialization (`load_from_file`/
  `load_or_default`/`save_to_file`, atomic write-tmp-then-rename), `validate()` (shape/version
  cross-checks — `predict` never bounds-checks, so a bad file must be rejected before it's ever
  stored), `predict()` (the forward pass), `TapModelStore` (an `ArcSwap<TapModel>` wrapper — the
  realtime audio callback calls `.current()`, a training/hot-swap thread calls `.swap()`, no locking
  on the read path), and `train`/`continue_training` (full-batch gradient descent — `train` fits from
  scratch, `continue_training` warm-starts from an existing model's weights *and* its existing
  `mean`/`std`, used by `gui/src-tauri`'s incremental-training loop so a small feedback-driven update
  doesn't shift the input normalization out from under already-good weights).
  - **Architecture**: 1 hidden layer (tanh) → softmax, same as the model's very first version, *plus*
    an optional small 1D convolution: if `n_bands > 0`, the trailing `n_bands` entries of the feature
    vector are treated as an ordered frequency-band sequence, run through `conv_w`/`conv_b`
    ('same'-padded, `conv_channels` output channels) → ReLU → global-max-pool-per-channel, and the
    pooled result is concatenated with the remaining scalar features before the dense layers. `n_bands
    == 0` degrades to exactly the original plain-dense architecture (and old on-disk models with no
    conv fields at all still load and predict identically, via `#[serde(default)]`). This was a
    deliberate answer to "would a CNN help" — not a full conv net over raw waveforms (overkill for a
    ~10ms mechanical transient and hard to hand-roll reliably), just enough structure to let the model
    learn frequency-adjacency instead of treating each band as an independent unrelated scalar. The
    conv forward *and hand-derived backward* pass have a dedicated numeric-gradient-check unit test
    (`conv_backward_matches_finite_difference`) — this is the standard way to catch a wrong-but-
    plausible-looking hand-rolled backprop derivation, and is what actually gives confidence the conv
    path trains correctly without an autodiff framework.
  - `FEATURE_SCHEMA_VERSION` is bumped only when `features::build_feature_vector`'s feature
    order/count changes incompatibly — independent of `n_hidden`/`n_classes`/`n_bands`, which
    `validate()` checks directly against array shapes, so a hidden-width or conv-channel change alone
    doesn't need a version bump.
  - `default_model.json` — the checked-in baseline every fresh install starts from
    (`TapModel::embedded_default()`, via `include_str!`). Regenerate it with `detect-test train
    --bake-default` after a training run you want to become the new out-of-the-box default.
- `src/features.rs` — DSP feature extraction and voice-activity gating, the other half of what used
  to be duplicated between `mic_tap.rs` and `detect-test/main.rs` (the per-stream *detection state
  machine* — debounce/confirm-delay/pie-menu-wiring vs. plain console logging — legitimately still
  differs between the two and stays duplicated on purpose).
  - `goertzel_magnitude`/`spectral_bands` — a 12-band, log-spaced (150Hz–8kHz) Goertzel spectral
    profile per audio chunk (cheap alternative to a full FFT for just a dozen bins), replacing the
    original 4-band centroid/hl_ratio-only summary with real spectral *shape* the conv path above can
    use.
  - `spectral_summary` — centroid/high-low-ratio/rolloff/flatness, all derived from the band profile
    (not stored separately anywhere — recomputed at feature-vector-build time).
  - `attack_shape` — normalized peak-sample position and first-half/second-half energy ratio within
    one chunk, folding attack/decay shape directly into the feature vector instead of only judging it
    post-hoc via `SUSTAIN_DECAY_FRACTION`-style cross-chunk heuristics.
  - `NoveltyState` — spectral-flux onset novelty (`microdsp::sfnov`), unchanged from the original
    version, just relocated here.
  - `VadState` — voice-activity gating, now backed by `earshot` (pure-Rust neural-net VAD) instead of
    Silero (`voice_activity_detector`, which pulled in `ort` plus a build-time-downloaded ONNX
    Runtime purely for this one speech gate — the single heaviest, most build-fragile dependency the
    app had). Important version caveat: this project already tried an `earshot` once before and
    dropped it for "letting too much speech through" — that was `earshot` 0.1.x, a WebRTC/GMM-style
    port. The `earshot` used now is 1.x, a from-scratch neural-net rewrite by the same author (who
    also maintains `ort`) — a different algorithm entirely. Its accuracy is the author's own
    self-benchmark; real-hardware testing (via `detect-test`'s "loud/exaggerated speech" collection
    phase, and in practice via the GUI's false-positive/false-negative feedback buttons — see
    `tap_feedback.rs` below) is what actually validates the swap, not the crate switch by itself.
  - `build_feature_vector` — assembles the final vector: 8 base scalars (unchanged from the original
    version: `[ln(peak), ln(rms), ln(ratio), zcr, ln(crest), centroid/1000, ln(hl_ratio), ln(novelty)]`),
    then rolloff/flatness/attack_pos/energy_skew/delta_ratio/delta_novelty, then the 12 band
    log-magnitudes last (the conv path treats that trailing slice as the ordered band sequence — see
    `TapModel`'s doc comment above). 26 features total.
- Weight history worth knowing if the model's behavior ever looks wrong: an early version modeled
  3 classes (`none`/fingernail-tap/fingertip-pad-tap) purely as a training-data trick to give the net a
  cleaner decision boundary — nothing downstream ever branched on which tap type fired. An offline
  held-out sweep over the full recorded dataset found that merging them into one true binary
  tap-vs-none target, *combined with* fixing a labeling bug (see `test-tools/detect-test`'s
  `CAPTURE_NEIGHBOR_MIN_PEAK` below) and a data-cleaning/augmentation recipe, raised real recall from
  roughly 45% to roughly 99.7% at only a small false-positive-rate cost, through the exact same
  runtime inference gate. If accuracy ever regresses, check those two things (label quality, binary
  vs. multi-class target) before assuming the architecture itself needs to change again.

**Wire protocol** (`crates/protocol`): DUML-style framing over USB bulk transfers on the vendor
control interface (interface 6 — see `DeviceModel::interface()`/`bulk_in()`/`bulk_out()`). Two
protocol dialects (v1/v2) coexist depending on firmware version, auto-detected from the heartbeat
stream (`packet::heartbeat_dialect`) since there's no way to query it directly. Full byte-level
framing, CRC, and per-dialect packet shapes are documented in `PROTOCOL.md` — read it before touching
`packet.rs` or adding settings. `DeviceModel::decode` turns an incoming frame into a `DeviceStatus`
snapshot; most dialects push one self-sufficient snapshot per heartbeat, but some split identity/level
info across several periodic frame types and rely on `previous` to carry forward what one frame alone
doesn't contain.

**Transport/orchestration** (`crates/device`): `DeviceManager` (`manager.rs`) rescans the USB bus
periodically, adopts/drops devices, and exposes a small blocking API (`list`, `status`, `set`,
`set_tx`) that both front-ends share. Each opened device gets its own OS thread (`actor.rs`, spawned
from `manager.rs`) running a `futures-lite` async loop (no tokio in this codebase) that races a queued
`bulk_in` read against an `async_channel` of outgoing setting-write commands, so reads and writes
never block each other. Set `DJIMIC_DEBUG=1` to get every raw frame (`[read]`/`[frame]`/`[write]`)
logged to stderr from that loop — the fastest way to reverse-engineer new protocol behavior.

**A device is more than one USB interface.** The vendor control interface (MI_06, the one `device`
talks to) only carries settings/heartbeats — it does *not* report the receiver's physical button
presses. Those instead show up as standard HID input reports on a separate interface (MI_00), which
Windows splits into multiple HID top-level collections (Consumer Control, Telephony, two
vendor-defined ones — enumerate with `Get-PnpDevice | Where-Object InstanceId -match VID_2CA3` to see
them all). `gui/src-tauri/src/pairing_button.rs` has the details of which collection/report shape the
pairing button actually uses, discovered by capturing raw HID reports while pressing it — the vendor
bulk interface never changes on a button press, so don't look there. The power button was tested the
same way and never produced any HID report at all; it appears to be a pure hardware toggle.

**GUI backend** (`gui/src-tauri`): `main.rs` wires up the Tauri app — single-instance, autostart,
close-to-tray (hides instead of quitting; the tray "quit" item is the real exit path), and a
tray icon that reflects live device/battery state, plus (composited in its own top-left corner, the
opposite corner from the device badges — the two aren't mutually exclusive) a coarse Claude Code
idle/thinking/working/error/attention status tracked by `claude_status.rs`, a tiny OS-agnostic atomic
updated from hook events relayed by `hook_bridge.rs` (see below) — deliberately last-write-wins, not a
precise per-session state machine, since Claude Code's hooks are fire-and-forget with no
request/response correlation. `commands.rs` holds the general snapshot/set-setting Tauri commands;
Windows-only concerns get their own modules:
- `driver.rs` — one-click WinUSB driver install for the vendor control interface. A hand-rolled INF
  can't install under Code Integrity enforcement without a signed catalog, so this downloads the
  official signed Zadig release, verifies its Authenticode signature, pre-seeds `zadig.ini` so it
  opens in the right state, and deletes it again afterward — it doesn't try to reimplement Zadig's
  catalog-signing.
- `pairing_button.rs` — reads the pairing button via the Win32 Raw Input API (`RIDEV_INPUTSINK`).
  `RIDEV_NOLEGACY` was tried to also suppress Windows' own default handling of that HID collection (an
  automatic system-volume change on every press) but is only valid for the Generic Desktop usage page
  (mouse/keyboard) — using it on the Consumer Control page makes `RegisterRawInputDevices` fail
  outright, silently, since the original code didn't check the return value. That means detection
  itself was broken too, not just the volume suppression; both bugs shipped together undetected for a
  while. A `WH_KEYBOARD_LL` hook doesn't work for the suppression either — Windows' default handling
  for HID consumer-page usages happens below the synthesized-keystroke layer. The volume-change side
  effect on every press is handled after the fact by `volume_guard.rs` (below) rather than solved at
  the input level. Every press also unconditionally simulates an Enter keypress (via `key_inject.rs`,
  below) as a general-purpose "pairing button = Enter" remap — this is what lets the pairing button
  confirm the pie menu (`pie_menu.rs`) without that module needing to know about the button at all.
- `volume_guard.rs` — neutralizes the pairing button's system-volume side effect from the previous
  bullet, since it can't be suppressed at the input level: continuously snapshots the default output
  device's volume/mute state while idle and forces it back shortly after every press, and separately
  keeps the volume OSD popup hidden for as long as the app runs (a press-triggered one-shot version of
  the OSD suppression was tried first but still flashed occasionally for reasons never pinned down).
- `mic_tap.rs` — detects 1-2 taps on the mic shell as an audio-domain gesture (3+ taps in a burst
  still report as a double tap). Feeds per-chunk features (amplitude/dynamics + the 12-band spectral
  profile + attack/temporal features — see `tap_model::features`) into the trained classifier
  (`tap_model::TapModel`, hot-swappable — see `crates/tap-model` above), gated by `earshot` (VAD) so a
  sharp consonant or loud exclamation mid-speech doesn't slip through as a tap. `MicTapWatcher` holds
  the live `Arc<TapModelStore>` (swapped by a background poll thread watching
  `tap_model.json`'s mtime — see `spawn_model_poll` — and by `tap_feedback.rs`'s incremental trainer,
  from two entirely different threads, with no locking on the audio-thread read path), an
  `Arc<tap_feedback::FeedbackRing>` (every chunk's raw measurements, pushed unconditionally before any
  suppression/floor branching — see below), and `last_group_taps` (confirm instants of the most
  recently finalized tap group, for false-positive targeting). Hard amplitude/ratio/crest floors sit on
  top of the model as a last-resort safety net (deliberately much less aggressive than the model's
  original hand-tuned thresholds now that label-cleaning + a higher confidence threshold do most of
  that filtering job — see `crates/tap-model`'s weight-history note above) plus a short confirm-delay/
  decay-check (`TAP_CONFIRM_DELAY`/`SUSTAIN_DECAY_FRACTION`) that rejects continuous noise (blowing,
  rubbing) a single chunk's snapshot could otherwise mistake for an impact. See the module doc comment
  for the full per-chunk pipeline order, and `test-tools/detect-test` (a standalone binary outside the
  workspace — see "Project layout") for how the model is iterated on/retrained against real hardware
  before being ported here.
- `tap_feedback.rs` — incremental training driven by in-app user feedback on the tap classifier, built
  on top of `mic_tap.rs`'s `FeedbackRing`: two Tauri commands
  (`mic_tap_report_false_positive`/`mic_tap_report_false_negative`, wired to buttons in
  `ReceiverShortcut.svelte`) locate the actual acoustic event in the ring buffer (a false positive
  looks backward from the confirmed tap group's instants minus `TAP_CONFIRM_DELAY`; a false negative
  scans the last few seconds for the loudest chunk above a relaxed sanity floor), append it correctly
  labeled to a per-user CSV (`tap_feedback.csv`, same schema as `detect-test`'s `samples.csv` — see
  below — so a future full retrain there can fold it back in), then kick off a bounded warm-started
  retrain (`tap_model::continue_training`) on a background thread: a handful of low-LR epochs from the
  *live* model's weights, using the new feedback rows (up-weighted by repetition) plus a bulk
  "background" replay sample drawn straight from whatever's currently in the ring buffer (real,
  this-room, this-hardware ambient audio — deliberately not a bundled dataset, so no extra shipped
  resource is needed). Before ever touching the live model, the candidate must have all-finite weights
  and must not regress "none"-class accuracy on that same replay sample past a small tolerance (a cheap
  proxy for "did this just make false triggers on ordinary noise more likely") — a rejected candidate
  is simply discarded, but its CSV rows remain for a later attempt or a full offline retrain.
  `mic_tap_training_status` (polled by the UI) reports model source/age/row-count/pending-feedback-
  count; `mic_tap_rollback_model`/`mic_tap_restore_factory_model` restore the pre-update backup
  (`tap_model.json.bak`) or the embedded baseline, respectively, backing up the about-to-be-replaced
  file first either way so "restore factory" is itself undoable.
- `shortcut.rs` — stub for the receiver-button-remap feature carried over from a removed macOS-only
  implementation (CGEventTap + `hidutil`); currently always reports unavailable on Windows.
- `pie_menu.rs` — a global Ctrl+Alt+P hotkey (`RegisterHotKey`; an earlier version bound bare `K`,
  deliberately making it untypeable system-wide, before settling on a combo with no known collision)
  that toggles a borderless, transparent, always-on-top overlay window docked above the taskbar: an LG
  webOS-style fan/pie quick menu, arrow keys (or a mic tap — see `mic_tap.rs`) to move the highlighted
  slot, Enter (or the pairing button) to confirm, Escape/losing focus to cancel. Like `pairing_button.rs`,
  the hotkey is registered on a hidden message-only window with its own `GetMessageW` loop on a
  dedicated thread; `WM_HOTKEY` hops back onto the Tauri main thread via `run_on_main_thread` before
  touching the window (window/webview calls aren't meant to be made from arbitrary threads). The
  overlay window is created once at startup and only shown/hidden/repositioned afterward, not rebuilt
  per toggle. The six real slots (`pie_menu_select`): voice-dictation hold (Win+Ctrl, held until a
  later pairing-button press ends it — see `key_inject.rs`), Down, Up, Enter, types `"/btw "`, close.
  Beyond that fixed menu, `permission_server.rs` can override what's showing with a pending question
  relayed from Claude Code itself — a `PermissionRequest` choice or a single-select `AskUserQuestion`
  tool call — complete with real title/option text in a small panel above the arc; an
  `AskUserQuestion`'s implicit freeform "Other" choice reuses the same voice-dictation slot instead of
  a keypress. See `gui/src/PieMenu.svelte` for the fan geometry (a half-circle "dome" shape, items
  placed along the upper arc by simple trig, the question panel's `PIVOT_X`/`PIVOT_Y` split from the
  arc's own radius) and the frontend-side open/close animation.
- `key_inject.rs` — `SendInput`-based keystroke simulation so a pie-menu slot can act on whatever
  application currently has focus, the same mechanism a hardware keyboard uses. `hold_win_ctrl_start`/
  `hold_win_ctrl_end` are separate key-down-only/key-up-only calls rather than one press-and-release
  function, since the Win+Ctrl voice-dictation hold is a toggle spanning an arbitrarily long gap
  between two unrelated events (the pie-menu slot that starts it, a later pairing-button press that
  ends it). `type_text` uses Unicode key events rather than virtual-key codes so it isn't limited to
  characters with a virtual key on the current keyboard layout.
- `hook_bridge.rs` — a loopback-only TCP listener (`127.0.0.1:47215`) that bridges Claude Code's own
  *non-permission* lifecycle hook events (`PreToolUse`/`PostToolUse`/`Stop`/etc.) into the pie menu and
  `claude_status.rs`. `~/.claude/settings.json` (outside this repo) registers a `"command"`-type hook
  per event that reads the event JSON off stdin and forwards it, unmodified, via a plain PowerShell
  one-liner that reads stdin as explicit UTF-8 (so non-ASCII payloads survive intact). Two jobs: (1) an
  `AskUserQuestion`'s `PostToolUse` event auto-dismisses a stale pie-menu question overlay if it was
  answered some other way than picking a pie-menu slot (typed directly in the terminal, or
  `permission_server.rs`'s own "answer in terminal instead" escape hatch) — a no-op if the pie menu
  already cleared its own pending-answer state when the pick happened there first; (2) every event with
  a `hook_event_name` updates the coarse idle/thinking/working/error/attention tray-icon status
  (`claude_status.rs`) — deliberately last-write-wins, not a precise per-session state machine, since
  hooks are fire-and-forget with no request/response correlation. `HookPayload`'s fields are all
  `Option<T>` with no `deny_unknown_fields`, so the same struct parses every event type without one
  event's payload shape breaking another's. **Does not** handle permission decisions at all anymore —
  see `permission_server.rs`.
- `permission_server.rs` — a loopback-only **HTTP** server (`127.0.0.1:47216/permission`, one port up
  from `hook_bridge.rs`'s raw-TCP port so the two never collide) registered in
  `~/.claude/settings.json` as an `"http"`-type `PermissionRequest` hook — this is the real allow/deny
  decision channel, not just a notification relay. An earlier iteration of this project believed the
  `"http"` hook type never actually delivered real `PermissionRequest` events in practice and built
  `hook_bridge.rs`'s keystroke-simulation fallback specifically to work around that; a later from-
  scratch probe proved that belief **wrong** (or it was fixed by a newer Claude Code version) — the
  `"http"` hook genuinely works as a decision channel, and a real decision response is honored with
  zero terminal interaction. Key points, since this reverses what earlier notes in this file said:
  - `AskUserQuestion` fires this exact same `PermissionRequest` hook (confirmed empirically, not
    documented anywhere) rather than some separate mechanism — so both a plain tool needing Allow/Deny
    and an interactive multi-choice question arrive at the identical endpoint, distinguished only by
    `tool_name == "AskUserQuestion"`.
  - The response body must be the `{"hookSpecificOutput":{"hookEventName":"PermissionRequest",
    "decision":{"behavior":"allow"|"deny",...}}}` envelope — only that shape's `updatedInput` field
    lets an `AskUserQuestion` answer be supplied directly (`answers: {question_text: chosen_label}`)
    instead of falling back to Claude Code's own terminal picker, which is what makes answering a
    question from the pie menu possible with **no keystroke injection, no focus stealing at all**.
  - `QUEUE` (a `Mutex<VecDeque<PendingRequest>>`, only the front item ever shown via `show_front`)
    exists because requests routinely overlap in practice — this very Claude Code session (the one
    that built this file) is itself a constant client of this exact server, so a human's separate
    session can easily have its own request in flight at the same moment. An earlier single-`Option`-
    slot design let a second arrival silently overwrite the first, which surfaced as two real bugs at
    once: "Allow" appearing to do nothing (the visible card's request had already been denied out from
    under it) and the overlay flashing/re-focusing on every new arrival regardless of one already being
    shown.
  - No `permission_mode`-based gating — an earlier version auto-allowed everything except `default`
    mode, on the theory that only `default` genuinely needs a human decision; real use falsified that
    (a Bash command in `acceptEdits` mode, which only auto-accepts *file edits*, still fired this hook
    expecting a real answer). Every request now pops the pie menu and waits, full stop, since Claude
    Code firing the hook at all already means it wants an answer.
  - "Allow, don't ask again" persists matching `permission_suggestions` rules into
    `{cwd}/.claude/settings.local.json`'s `permissions.allow` array — the same file/format Claude
    Code's own `destination: "localSettings"` suggestions target — best-effort (a failed read/merge/
    write still lets the already-decided allow proceed, just without being remembered).

**Frontend** (`gui/src`): Svelte 5, poll-based throughout (no Tauri event emission anywhere in the
codebase, *except* `pie-menu:open`, which `pie_menu.rs` emits to reset the overlay's selection each
time it's shown — see `PieMenu.svelte`) — `App.svelte` polls `snapshot()` on a 250ms `setInterval` and
drives everything else off that plus a couple of feature-specific pollers. `lib/api.js` is the thin
`invoke()` wrapper layer; add new Tauri commands there rather than calling `invoke` directly from
components. `main.js` mounts one of two root components depending on `getCurrentWindow().label`: `App`
for `"main"`, `PieMenu` for `"pie-menu"` — both windows load the same bundle. `ReceiverShortcut.svelte`
(the "接收器快捷键" workspace tab) also hosts the mic-tap feedback/incremental-training panel: two
buttons calling `micTapReportFalsePositive`/`micTapReportFalseNegative` (see `tap_feedback.rs` above),
a status line polling `micTapTrainingStatus` every 2s (model source/age, pending-feedback count,
idle-vs-training state), and rollback/restore-factory buttons — all Simplified Chinese copy, matching
the rest of the app.

## Project layout

```text
Cargo.toml                       Workspace root: protocol/device/cli/tap-model/gui-src-tauri members
CLAUDE.md                        This file
README.md                        User-facing feature list and setup instructions (Simplified Chinese)
PROTOCOL.md                      Byte-level DUML framing/CRC/packet-shape reverse-engineering notes
deny.toml                        `cargo deny` license allowlist
build-release.ps1                Windows release build (portable exe + NSIS installer) — see Commands
build-release.sh                 Linux/macOS-upstream equivalent; explicitly rejects Darwin
LICENSE                          Unlicense (this repo); third-party deps/models keep their own licenses
reference.txt                    Upstream project URLs this repo was forked/adapted from (see README 致谢)
dark.png / light.png / zadig.png Screenshots/assets referenced by README and the Zadig driver installer
Release/                         Build output of build-release.ps1 (portable exe + installer); gitignored
target/                          Cargo build artifacts; gitignored
native/                          Empty — harmless upstream leftover, not used by anything

crates/
  protocol/                      Pure DUML framing/CRC/per-model command+decode logic, zero I/O
    src/models/                  One file per supported mic model, registered in `models/mod.rs`'s MODELS
  device/                        USB transport (nusb) + multi-device orchestration; depends on protocol
    src/manager.rs                 DeviceManager: bus rescan, adopt/drop, blocking list/status/set/set_tx API
    src/actor.rs                   Per-device OS thread running a futures-lite async read/write loop
  cli/                            One-shot CLI front-end (djimic binary); depends on device only
  tap-model/                      Mic-tap classifier: model format, forward pass, training, hot-swap store —
                                  shared by gui/src-tauri (inference) and test-tools/detect-test (training).
                                  Nothing to do with the DJI wire protocol — see its own paragraph above.
    default_model.json              Checked-in embedded baseline (regenerate: `detect-test train --bake-default`)
    src/lib.rs                      TapModel, TapModelStore, train/continue_training, Rng
    src/features.rs                 Goertzel bands, spectral summary, attack shape, NoveltyState, VadState

gui/                              Tauri 2 + Svelte 5 desktop app — the shipped product
  src-tauri/                      Rust backend
    src/main.rs                     App wiring: tray icon/menu, single-instance, autostart, close-to-tray
    src/commands.rs                 General snapshot/set-setting Tauri commands (device list/status/settings)
    src/driver.rs                   One-click WinUSB driver install (downloads+verifies signed Zadig)
    src/pairing_button.rs           Pairing-button press detection via Win32 Raw Input API
    src/volume_guard.rs             Neutralizes the pairing button's system-volume side effect
    src/mic_tap.rs                  Mic-shell-tap detection — see its own paragraph above
    src/tap_feedback.rs             Incremental training from in-app user feedback — see above
    src/shortcut.rs                 Receiver-button-remap stub, always unavailable on Windows
    src/pie_menu.rs                 Ctrl+Alt+P pie-menu overlay (also renders pending hook questions)
    src/key_inject.rs               SendInput-based keystroke simulation for pie-menu actions
    src/hook_bridge.rs              Non-permission Claude Code hook events -> pie menu / tray status
    src/permission_server.rs        Claude Code PermissionRequest/AskUserQuestion http-hook decision server
    src/claude_status.rs            Coarse idle/thinking/working/error/attention atomic for the tray badge
    capabilities/pie-menu.json      Tauri capability grant for the separate "pie-menu" window
    icons/                          App/tray icons baked in via include_bytes!
  src/                             Svelte 5 frontend
    App.svelte                       Root component for the "main" window (device list + settings)
    PieMenu.svelte                   Root component for the "pie-menu" window (see pie_menu.rs)
    main.js                          Picks which root component to mount based on window label
    lib/api.js                       Sole invoke() wrapper layer — add new Tauri commands here
    lib/DevicePanel.svelte           Per-device settings panel (NC mode, voice tone, LEDs, etc.)
    lib/ReceiverShortcut.svelte      "接收器快捷键" tab — pairing/tap test status + tap feedback panel
    lib/Sidebar.svelte               Device list sidebar
    lib/*Control.svelte, *Picker.svelte, *Artwork.svelte, *Icon.svelte, *Picto.svelte
                                     Small presentational settings-control/artwork components
    lib/UdevModal.svelte             Linux udev-rule setup instructions modal
    lib/txCovers.js                  Static asset map for Mic Mini 2 cover-color artwork

test-tools/detect-test/          Standalone crate OUTSIDE the Cargo workspace (own empty [workspace]
                                  table) — fast `cargo run` iteration for mic-tap/pairing-button
                                  detection without rebuilding the full Tauri app; depends on
                                  crates/tap-model via a plain path dep (no shared workspace membership
                                  needed for that). See "Commands" above for its subcommands.
  data/samples.csv                Collected training data (label + 21 raw feature columns, append-only)
  data/samples.csv.bak-*          Auto-backed-up previous-schema data (see CSV_HEADER mismatch handling)

packaging/                        Linux-only packaging leftovers from the cross-platform upstream
  60-dji-mic.rules                  udev rule granting non-root USB access to the vendor control interface
  postinstall.sh / postremove.sh    udev-rule install/removal hooks for a .deb/.rpm-style package
```

## Dependencies

Kept deliberately light — no ML framework, no async runtime beyond `futures-lite`, no heavy DSP/audio
stack beyond what `cpal` requires. When adding a dependency, prefer a small, focused, pure-Rust crate
over one that pulls in a runtime/toolchain of its own (the project has twice removed a dependency for
exactly this reason — see `ort`/`voice_activity_detector` below).

**Workspace-wide** (`Cargo.toml`'s `[workspace.dependencies]`): `serde`/`serde_json` (data model +
on-disk formats), `thiserror`/`anyhow` (error types), `nusb` (cross-platform USB), `futures-lite`/
`async-channel` (the per-device actor's async loop — deliberately not tokio), `clap` (CLI arg parsing).

- **`crates/protocol`** — `serde` only (plus `serde_json` as a dev-dependency for tests). Pure logic,
  no I/O, so nothing else is needed.
- **`crates/device`** — `protocol`, `nusb`, `futures-lite`, `async-channel`, `thiserror`, `serde`.
- **`crates/cli`** — `device`, `clap`, `serde_json`, `anyhow`.
- **`crates/tap-model`** — `serde`/`serde_json` (the `TapModel` on-disk format), `arc-swap` (lock-free
  hot-swap store — a tiny, single-purpose crate, not a general concurrency framework), `microdsp`
  (spectral-flux onset novelty — a mature MIR/percussive-onset technique, cheaper and more reliable
  than a hand-rolled "how sudden is this" heuristic), `earshot` 1.x (pure-Rust neural-net VAD — see the
  weight-history/VAD note above for why this replaced `voice_activity_detector`+`ort`).
- **`gui/src-tauri`** — `device`, `tap-model`, `tauri` (`image-png`/`tray-icon` features),
  `tauri-plugin-single-instance`, `tauri-plugin-autostart`, `serde`/`serde_json`.
  Windows-only (`[target.'cfg(windows)'.dependencies]`): `cpal` (audio capture for mic-tap detection),
  `windows` (Win32 API bindings — Raw Input, `SendInput`, Com, audio endpoint volume control, threading;
  feature-gated to just the API families actually used). Unix-only: `libc`/`signal-hook` (upstream
  cross-platform leftovers, not exercised on the Windows-only shipped build).
- **`test-tools/detect-test`** — `cpal`, `serde_json`, `tap-model` (path dep, see "Project layout").
  Windows-only: `windows` (Raw Input for its own pairing-button test path, `Win32_Graphics_Gdi`).
- **Build-time**: `tauri-build` (`gui/src-tauri`'s `build.rs` codegen).

**Deliberately avoided** (with a documented reason, so they aren't re-added by accident):
- `ort` + `voice_activity_detector` (Silero VAD) — replaced by `earshot` 1.x: `ort` downloads a
  prebuilt ONNX Runtime at build time and pins a pre-1.0 `ort` release candidate, the single heaviest
  and most build-fragile dependency the app ever had, purely for one speech-gate.
- `linfa`/`tch`/`candle`/any ML framework — the mic-tap classifier (including its small 1D-conv path,
  see `crates/tap-model` above) is hand-rolled with plain `Vec<f32>` math instead; the model is small
  enough that hand-derived forward/backward passes (verified with a numeric gradient-check test) are
  both sufficient and easier to reason about than pulling in a framework's dependency tree.
- `rand`/`rand_distr` — `tap-model::Rng` is a ~10-line hand-rolled xorshift64 PRNG (weight init) plus a
  Box-Muller transform (Gaussian jitter for data augmentation); not worth a dependency for this little.
- `hound` — was used only for an optional `DJIMIC_DEBUG=1` "dump what the VAD hears" WAV file in
  `detect-test`'s old Silero-based `VadState`; dropped when VAD moved into the shared, hound-free
  `tap-model::features::VadState`. Raw-audio retention during `collect` (so a future feature-vector
  change wouldn't require a brand-new recording session) was identified as a valuable follow-up but not
  implemented — reintroducing `hound` for that is the natural way to do it.
- tokio — the USB actor loop (`crates/device/src/actor.rs`) uses `futures-lite`/`async-channel` instead;
  no part of this codebase needs a full async runtime.

## Known limitations / open items

Things flagged during development as genuinely unfinished or unvalidated, consolidated here so they
don't have to be rediscovered by grepping for "TODO" (there isn't one — each item below is discussed
in more depth at the file/bullet cited):

- **`shortcut.rs`** — the receiver-button-remap feature (short-press receiver → `Fn+Control`) is a stub
  ported from a removed macOS-only implementation (CGEventTap + `hidutil`); it always reports
  unavailable on Windows. No Windows implementation exists yet.
- **earshot 1.x VAD accuracy** — its accuracy claims are the author's own self-benchmark, not this
  project's. Real-hardware validation happens through `detect-test`'s loud/exaggerated-speech
  collection phase and, in production, through the GUI's false-positive/false-negative feedback
  buttons (see `tap_feedback.rs`) — ongoing via actual use, not a one-time check that's already closed
  out.
- **Volume OSD flash on pairing-button press** — `volume_guard.rs` keeps the OSD popup suppressed for
  the app's whole runtime and restores volume/mute state shortly after every press, but an earlier
  press-triggered one-shot suppression attempt still let it flash occasionally for reasons never root-
  caused; the always-on approach shipped instead because it works in practice, not because the root
  cause was found.
- **Raw-audio retention during `collect`** — identified as a valuable follow-up (so a future
  feature-vector change wouldn't force an entirely new recording session) but not implemented;
  reintroducing `hound` (previously removed — see Dependencies above) is the natural way to do it if
  this is ever picked up.
- **`test-tools/detect-test/data/samples.csv`** is append-only and grows every `collect`/
  `collect-extra`/`collect-friction` run — it accumulates across many separate recording sessions over
  time rather than being replaced, so it can grow to hundreds of thousands of rows. `run_train`'s
  full-batch gradient descent (3000 epochs, `TRAIN_AUGMENT_FACTOR = 15` on top of that for the minority
  tap class) re-processes the entire file every epoch with no subsampling, so retraining time scales
  with however large the file has grown — a training run can legitimately take longer than it used to
  as more data is collected, this is not a hang. (A Python prototyping script used earlier to find the
  label-cleaning/augmentation recipe did subsample the majority class for speed, but that subsampling
  was never carried into the production Rust `train_inner` — full-batch is what actually ships.)
