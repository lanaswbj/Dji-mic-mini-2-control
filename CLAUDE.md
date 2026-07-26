# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Two further CLAUDE.md files cover the two areas with the most hard-won detail, and load automatically
when you work inside them:

- `crates/tap-model/CLAUDE.md` — the mic-shell-tap classifier, its feature pipeline, and the offline
  retraining workflow in `test-tools/detect-test`.
- `gui/CLAUDE.md` — the Tauri backend (`gui/src-tauri`) and the Svelte frontend (`gui/src`).

## What this is

A Windows desktop control app for DJI wireless microphones (DJI Mic Mini / Mini 2), forked from an
upstream cross-platform project and re-scoped to Windows only. macOS-specific features that depended
on BlackHole (virtual audio device) and CoreAudio (Voice Comfort real-time processing, in-app audio
device switching) were removed entirely rather than ported. The `protocol`/`device`/`cli` crates keep
their cross-platform structure (Linux udev packaging files still exist under `packaging/`) but only
Windows is actively built and shipped — see `README.md` for the user-facing feature list.

## Commands

Rust workspace (`protocol`, `device`, `cli`, `tap-model`, `gui/src-tauri`):

```bash
cargo check --workspace          # fast type-check, do this before anything heavier
cargo build --workspace
cargo test -p protocol           # crc.rs, packet.rs, models/mic_mini.rs
cargo test -p tap-model          # model forward-pass/training tests, incl. a numeric gradient check
cargo test -p protocol <name>    # run a single test
cargo deny check                 # enforce the license allowlist in deny.toml
```

`--workspace` does **not** reach `test-tools/detect-test` — see "Repo layout" below. Its own commands
(data collection, retraining) are documented in `crates/tap-model/CLAUDE.md`.

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
`cli`, or the GUI needs to change. `crates/tap-model` is a second, unrelated shared crate that has
nothing to do with the DJI wire protocol — see its own CLAUDE.md.

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

## Repo layout

`ls` covers the shape; these are the parts it won't tell you:

- **`test-tools/detect-test` is deliberately outside the Cargo workspace** (it carries its own empty
  `[workspace]` table) and reaches `crates/tap-model` through a plain path dependency. So
  `cargo check --workspace` at the root never compiles it — check it separately after changing
  anything in `tap-model`. The point of keeping it out is fast `cargo run` iteration on tap/pairing
  detection without rebuilding the whole Tauri app.
- **`PROTOCOL.md`** — byte-level DUML framing/CRC/packet-shape reverse-engineering notes. Required
  reading before touching `crates/protocol/src/packet.rs`.
- **`design-system/dji-mic-control/MASTER.md`** — the GUI's design system (intent, IA, contrast math,
  type/space/motion tokens, component contracts, feedback model, polling strategy) and a table
  mapping each section to the code that implements it. Change design decisions there, not in a
  component.
- **`packaging/`** — Linux-only leftovers from the cross-platform upstream (a udev rule granting
  non-root USB access, plus .deb/.rpm install hooks). Not exercised by the Windows build.
- **`native/`** is empty; **`Release/`** and **`target/`** are build output and gitignored;
  **`reference.txt`** holds the upstream project URLs this repo was forked from.

## Dependencies

Kept deliberately light — no ML framework, no async runtime beyond `futures-lite`, no heavy DSP/audio
stack beyond what `cpal` requires. When adding one, prefer a small, focused, pure-Rust crate over one
that pulls in a runtime or toolchain of its own; this project has twice removed a dependency for
exactly that reason. Per-crate manifests are the source of truth — read the `Cargo.toml`.

**Deliberately avoided** (documented so they aren't re-added by accident):

- `ort` + `voice_activity_detector` (Silero VAD) — replaced by `earshot` 1.x: `ort` downloads a
  prebuilt ONNX Runtime at build time and pins a pre-1.0 release candidate, the single heaviest and
  most build-fragile dependency the app ever had, purely for one speech gate.
- `linfa`/`tch`/`candle`/any ML framework — the mic-tap classifier (including its small 1D-conv path)
  is hand-rolled with plain `Vec<f32>` math; the model is small enough that hand-derived
  forward/backward passes, verified with a numeric gradient-check test, are both sufficient and easier
  to reason about than a framework's dependency tree.
- `rand`/`rand_distr` — `tap-model::Rng` is a ~10-line hand-rolled xorshift64 PRNG (weight init) plus
  a Box-Muller transform (Gaussian jitter for augmentation); not worth a dependency for this little.
- `hound` — was used only for an optional `DJIMIC_DEBUG=1` "dump what the VAD hears" WAV file in
  `detect-test`'s old Silero-based `VadState`; dropped when VAD moved into the shared, hound-free
  `tap-model::features::VadState`. Reintroducing it is the natural way to do raw-audio retention
  during `collect` if that follow-up is ever picked up.
- tokio — the USB actor loop (`crates/device/src/actor.rs`) uses `futures-lite`/`async-channel`
  instead; no part of this codebase needs a full async runtime.

## Known limitations / open items

Genuinely unfinished or unvalidated, consolidated so they don't have to be rediscovered by grepping
for "TODO" (there isn't one). Tap-classifier items live in `crates/tap-model/CLAUDE.md`.

- **`shortcut.rs`** — the receiver-button-remap feature (short-press receiver → `Fn+Control`) is a
  stub ported from a removed macOS-only implementation (CGEventTap + `hidutil`); it always reports
  unavailable on Windows. No Windows implementation exists yet, and it deliberately has no navigation
  entry in the GUI — a section that permanently reads "unavailable" is noise.
- **Volume OSD flash on pairing-button press** — `volume_guard.rs` keeps the OSD popup suppressed for
  the app's whole runtime and restores volume/mute state shortly after every press, but an earlier
  press-triggered one-shot suppression attempt still let it flash occasionally for reasons never
  root-caused; the always-on approach shipped because it works in practice, not because the root cause
  was found.
