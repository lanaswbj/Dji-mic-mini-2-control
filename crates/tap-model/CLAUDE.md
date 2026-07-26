# crates/tap-model

The mic-shell-tap classifier: model format, forward pass, training, and the DSP feature pipeline
feeding it. **Nothing here has anything to do with the DJI wire protocol** — it is a second, unrelated
shared crate that happens to live in the same workspace.

Shared between `gui/src-tauri` (inference, at runtime) and `test-tools/detect-test` (training,
offline) so the two can never drift apart the way hand-copied `const` weight arrays used to.
detect-test reaches it through a plain Cargo path dependency and is **not** a workspace member — see
the root CLAUDE.md's "Repo layout" for why that distinction matters when you run `cargo check`.

## Commands

Retraining runs from `test-tools/detect-test`:

```bash
cd test-tools/detect-test
cargo run --release -- collect             # ~4 min: quiet/speech/loud-speech/noise/nail-tap/pad-tap phases
cargo run --release -- collect-extra       # ~2 min: pairing-button press + blowing (hard negatives)
cargo run --release -- collect-friction    # ~1 min: finger rubbing the shell (hard negative)
cargo run --release -- train               # fits a new model, writes it to %APPDATA%\org.djimic.control\tap_model.json
cargo run --release -- train --bake-default  # also overwrites crates/tap-model/default_model.json
cargo run --release                        # (no subcommand) live detection console + pairing-button listener, no GUI
```

`train` on its own hot-swaps the *currently running* app's model (if any) within ~3s via its
file-poll thread — no rebuild, no restart. `--bake-default` is a separate, deliberate step for a
maintainer updating what ships in the installer; run it only when a training run should become the new
out-of-the-box default, not on every local retrain.

## `src/lib.rs`

`TapModel` is the trained artifact (weights, per-feature z-score `mean`/`std`, `class_names`,
`confidence_threshold`, `feature_schema_version`, plus `source`/`trained_at_unix_ms`/`training_rows`
metadata) with JSON (de)serialization that writes to a temp file and renames. Two things about it are
load-bearing:

- **`validate()` must run before a model is ever stored.** `predict()` never bounds-checks anything,
  so shape/version cross-checks are the only thing standing between a corrupt file and a panic on the
  realtime audio thread.
- **`TapModelStore` is an `ArcSwap<TapModel>`**, not a lock. The realtime audio callback calls
  `.current()`; the file-poll thread and `tap_feedback.rs`'s incremental trainer — two entirely
  different threads — call `.swap()`. There is no locking on the read path, deliberately.

`train` fits from scratch; `continue_training` warm-starts from an existing model's weights *and* its
existing `mean`/`std`. The second one exists for the GUI's incremental-training loop: a small
feedback-driven update must not shift the input normalization out from under already-good weights.

**Architecture**: 1 hidden layer (tanh) → softmax, same as the model's very first version, *plus* an
optional small 1D convolution. If `n_bands > 0`, the trailing `n_bands` entries of the feature vector
are treated as an ordered frequency-band sequence, run through `conv_w`/`conv_b` ('same'-padded,
`conv_channels` output channels) → ReLU → global-max-pool-per-channel, and the pooled result is
concatenated with the remaining scalar features before the dense layers. `n_bands == 0` degrades to
exactly the original plain-dense architecture, and old on-disk models with no conv fields at all still
load and predict identically via `#[serde(default)]`.

This was a deliberate answer to "would a CNN help" — not a full conv net over raw waveforms (overkill
for a ~10ms mechanical transient, and hard to hand-roll reliably), just enough structure to let the
model learn frequency-adjacency instead of treating each band as an independent unrelated scalar. The
conv forward *and hand-derived backward* pass have a dedicated numeric-gradient-check unit test
(`conv_backward_matches_finite_difference`) — the standard way to catch a wrong-but-plausible-looking
hand-rolled backprop derivation, and what actually gives confidence the conv path trains correctly
without an autodiff framework.

`FEATURE_SCHEMA_VERSION` is bumped **only** when `features::build_feature_vector`'s feature
order/count changes incompatibly. It is independent of `n_hidden`/`n_classes`/`n_bands`, which
`validate()` checks directly against array shapes — so a hidden-width or conv-channel change alone
does not need a version bump.

`default_model.json` is the checked-in baseline every fresh install starts from
(`TapModel::embedded_default()`, via `include_str!`). Regenerate it with `detect-test train
--bake-default` after a training run you want to become the new out-of-the-box default.

## `src/features.rs`

DSP feature extraction and voice-activity gating — the other half of what used to be duplicated
between `gui/src-tauri/src/mic_tap.rs` and `detect-test/main.rs`. The per-stream *detection state
machine* (debounce / confirm-delay / pie-menu wiring vs. plain console logging) legitimately still
differs between the two and stays duplicated on purpose.

- `goertzel_magnitude`/`spectral_bands` — a 12-band, log-spaced (150Hz–8kHz) Goertzel spectral profile
  per audio chunk (cheap alternative to a full FFT for just a dozen bins), replacing the original
  4-band centroid/hl_ratio-only summary with real spectral *shape* the conv path can use.
- `spectral_summary` — centroid / high-low-ratio / rolloff / flatness, all derived from the band
  profile and recomputed at feature-vector-build time rather than stored anywhere.
- `attack_shape` — normalized peak-sample position and first-half/second-half energy ratio within one
  chunk, folding attack/decay shape directly into the feature vector instead of only judging it
  post-hoc via `SUSTAIN_DECAY_FRACTION`-style cross-chunk heuristics.
- `NoveltyState` — spectral-flux onset novelty (`microdsp::sfnov`), unchanged from the original
  version, just relocated here.
- `VadState` — voice-activity gating, backed by `earshot` (pure-Rust neural-net VAD).
  **Important version caveat**: this project already tried an `earshot` once before and dropped it for
  "letting too much speech through" — that was `earshot` 0.1.x, a WebRTC/GMM-style port. The one used
  now is 1.x, a from-scratch neural-net rewrite by the same author (who also maintains `ort`) — a
  different algorithm entirely. It replaced Silero (`voice_activity_detector`), which pulled in `ort`
  plus a build-time-downloaded ONNX Runtime purely for this one speech gate.
- `build_feature_vector` — assembles the final 26-feature vector: 8 base scalars (unchanged from the
  original version: `[ln(peak), ln(rms), ln(ratio), zcr, ln(crest), centroid/1000, ln(hl_ratio),
  ln(novelty)]`), then rolloff / flatness / attack_pos / energy_skew / delta_ratio / delta_novelty,
  then the 12 band log-magnitudes **last** — the conv path treats that trailing slice as the ordered
  band sequence.

## Weight history — read this before changing the architecture

An early version modeled 3 classes (`none` / fingernail-tap / fingertip-pad-tap) purely as a
training-data trick to give the net a cleaner decision boundary; nothing downstream ever branched on
which tap type fired. An offline held-out sweep over the full recorded dataset found that merging them
into one true binary tap-vs-none target, *combined with* fixing a labeling bug (see detect-test's
`CAPTURE_NEIGHBOR_MIN_PEAK`) and a data-cleaning/augmentation recipe, raised real recall from roughly
45% to roughly 99.7% at only a small false-positive-rate cost — through the exact same runtime
inference gate.

**If accuracy ever regresses, check those two things (label quality; binary vs. multi-class target)
before assuming the architecture itself needs to change again.**

The same result is why `mic_tap.rs`'s hard amplitude/ratio/crest floors are now much less aggressive
than the model's original hand-tuned thresholds: label-cleaning plus a higher confidence threshold do
most of that filtering job now.

## Known limitations / open items

- **earshot 1.x VAD accuracy** — its accuracy claims are the author's own self-benchmark, not this
  project's. Real-hardware validation happens through detect-test's loud/exaggerated-speech collection
  phase and, in production, through the GUI's false-positive/false-negative feedback buttons (see
  `gui/src-tauri/src/tap_feedback.rs`) — ongoing via actual use, not a one-time check already closed
  out.
- **Raw-audio retention during `collect`** — identified as a valuable follow-up (so a future
  feature-vector change wouldn't force an entirely new recording session) but not implemented.
  Reintroducing `hound` (previously removed — see the root CLAUDE.md's Dependencies) is the natural
  way to do it.
- **`test-tools/detect-test/data/samples.csv` is append-only** and grows on every
  `collect`/`collect-extra`/`collect-friction` run, accumulating across many recording sessions rather
  than being replaced — it can reach hundreds of thousands of rows. `run_train`'s full-batch gradient
  descent (3000 epochs, `TRAIN_AUGMENT_FACTOR = 15` on top of that for the minority tap class)
  re-processes the entire file every epoch with no subsampling, so **retraining time scales with the
  file's size — a long training run is not a hang.** (A Python prototyping script used earlier to find
  the label-cleaning/augmentation recipe did subsample the majority class for speed, but that
  subsampling was never carried into the production Rust `train_inner`; full-batch is what ships.)
  `data/samples.csv.bak-*` files are auto-backed-up previous-schema data, from `CSV_HEADER` mismatch
  handling.
