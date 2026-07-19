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
cargo test -p protocol           # the only crate with unit tests (crc.rs, packet.rs, models/mic_mini.rs)
cargo test -p protocol <name>    # run a single test
cargo deny check                 # enforce the license allowlist in deny.toml
```

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
`cli`, or the GUI needs to change.

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
tray icon that reflects live device/battery state. `commands.rs` holds the general
snapshot/set-setting Tauri commands; Windows-only concerns get their own modules:
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
  effect on every press is currently unsolved.
- `mic_tap.rs` — detects 1-2 taps on the mic shell as an audio-domain gesture (3+ taps in a burst
  still report as a double tap). Feeds a handful of per-chunk features — amplitude/dynamics, a 4-band
  Goertzel spectral snapshot, and spectral-flux onset novelty (`microdsp`, a mature MIR/music
  information retrieval technique) — into a small trained neural net (not hand-tuned thresholds; those
  never fired reliably on this hardware). A Silero VAD (`voice_activity_detector`) runs alongside it
  and gates tap classification off while speech is active, since the classifier's own features alone
  still occasionally read a sharp consonant as a tap. See the module doc comment for the full
  reasoning and `test-tools/detect-test` (a standalone binary outside the workspace) for how the model
  was iterated on and retrained against real hardware before being ported here — that's also where
  `cargo run -- collect` / `cargo run -- train` live if the model ever needs retraining.
- `shortcut.rs` — stub for the receiver-button-remap feature carried over from a removed macOS-only
  implementation (CGEventTap + `hidutil`); currently always reports unavailable on Windows.

**Frontend** (`gui/src`): Svelte 5, poll-based throughout (no Tauri event emission anywhere in the
codebase) — `App.svelte` polls `snapshot()` on a 250ms `setInterval` and drives everything else off
that plus a couple of feature-specific pollers. `lib/api.js` is the thin `invoke()` wrapper layer; add
new Tauri commands there rather than calling `invoke` directly from components.
