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
  Beyond that fixed menu, `hook_bridge.rs` can override what's showing with a pending question relayed
  from Claude Code itself — a `PermissionRequest` choice or a single-select `AskUserQuestion` tool
  call — complete with real title/option text in a small panel above the arc; an `AskUserQuestion`'s
  implicit freeform "Other" choice reuses the same voice-dictation slot instead of a keypress. See
  `gui/src/PieMenu.svelte` for the fan geometry (a half-circle "dome" shape, items placed along the
  upper arc by simple trig, the question panel's `PIVOT_X`/`PIVOT_Y` split from the arc's own radius)
  and the frontend-side open/close animation.
- `key_inject.rs` — `SendInput`-based keystroke simulation so a pie-menu slot can act on whatever
  application currently has focus, the same mechanism a hardware keyboard uses. `hold_win_ctrl_start`/
  `hold_win_ctrl_end` are separate key-down-only/key-up-only calls rather than one press-and-release
  function, since the Win+Ctrl voice-dictation hold is a toggle spanning an arbitrarily long gap
  between two unrelated events (the pie-menu slot that starts it, a later pairing-button press that
  ends it). `type_text` uses Unicode key events rather than virtual-key codes so it isn't limited to
  characters with a virtual key on the current keyboard layout.
- `hook_bridge.rs` — a loopback-only TCP listener (`127.0.0.1:47215`) that bridges Claude Code's own
  hook events into the pie menu and `claude_status.rs`. `~/.claude/settings.json` (outside this repo)
  registers a `"command"`-type hook per event that reads the event JSON off stdin and forwards it,
  unmodified, via a plain PowerShell one-liner — chosen over the `"http"` hook type (which would let a
  hook answer a permission request directly, skipping Claude Code's own prompt entirely) after that
  approach was tried first, confirmed correctly registered, and confirmed reachable in isolation, but
  never once received a real `PermissionRequest` event in practice; see the module's own doc comment
  for the full story. Answering a question therefore means simulating the real keypress a human would
  make into whatever terminal window is actually showing Claude Code's prompt (`key_inject.rs`), not a
  Claude-Code-API decision channel. `HookPayload`'s fields are all `Option<T>` with no
  `deny_unknown_fields`, so the same struct parses every event type without one event's payload shape
  breaking another's.

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
