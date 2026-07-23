//! Trained mic-shell-tap classifier as runtime data instead of compile-time
//! constants. Previously `gui/src-tauri/src/mic_tap.rs` and
//! `test-tools/detect-test/src/main.rs` each hardcoded the exact same
//! `TAP_W1`/`TAP_B1`/`TAP_W2`/`TAP_B2`/`TAP_MEAN`/`TAP_STD` arrays as Rust
//! `const`s — retraining meant running detect-test's `train` subcommand,
//! hand-copying the printed arrays into *both* files, and recompiling. This
//! crate is the single place the model (its shape, its forward pass, and how
//! it's fit) lives: `gui/src-tauri` loads it for inference and can hot-swap
//! it at runtime; `test-tools/detect-test` uses the exact same `predict`/
//! `train` so the two can never drift apart.
//!
//! On-disk format is plain JSON (`TapModel`'s `Serialize`/`Deserialize`) at
//! `%APPDATA%\org.djimic.control\tap_model.json` — human-inspectable, and
//! easy to atomically replace ([`TapModel::save_to_file`]) without a reader
//! ever observing a torn write.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

pub mod features;

/// Bumped whenever `build_feature_vector`'s feature order/count changes
/// incompatibly. A file whose `feature_schema_version` doesn't match this
/// crate's compiled-in value is rejected by [`TapModel::validate`] rather
/// than silently fed to a forward pass expecting a different feature
/// layout. Independent of `n_hidden`/`n_classes`, which `validate` checks
/// directly against the array shapes — a hidden-layer-width or class-count
/// change doesn't need a schema bump, only a change to the feature vector
/// itself does.
///
/// v2: `features::build_feature_vector` grew from 8 plain scalars to 26 —
/// a wider spectral-band profile (see `features::N_BANDS`) plus rolloff/
/// flatness/temporal features, replacing the old 4-band centroid/hl_ratio
/// summary. `TapModel` gained an optional 1D-conv path over the trailing
/// band sequence (see its doc comment) to go with it.
pub const FEATURE_SCHEMA_VERSION: u32 = 2;

/// Matches `tauri.conf.json`'s `identifier`, so the standalone `detect-test`
/// binary (which has no `AppHandle`) resolves the exact same folder the
/// running app uses.
pub const APP_IDENTIFIER: &str = "org.djimic.control";

const DEFAULT_MODEL_JSON: &str = include_str!("../default_model.json");

/// Where a model came from — purely informational metadata for a status
/// panel, never consulted by `predict`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    /// The checked-in baseline baked into the binary via `include_str!`.
    Embedded,
    /// A from-scratch fit over the full dataset (`detect-test`'s `train`).
    FullRetrain,
    /// A bounded, warm-started update from user feedback ([`continue_training`]).
    Incremental,
}

/// A trained classifier: weights, input normalization, and just enough
/// metadata to show a human what they're running. `n_features`/`n_hidden`/
/// `n_classes` are stored explicitly (not just inferred from the array
/// lengths) so [`TapModel::validate`] can cross-check shape independently of
/// `feature_schema_version`.
///
/// Architecture: a normalized feature vector optionally splits into a
/// scalar prefix (length `n_features - n_bands`) and a trailing ordered
/// band sequence (length `n_bands`, see `features::N_BANDS`). When
/// `n_bands > 0`, the band sequence runs through a small 1D convolution
/// (`conv_w`/`conv_b`, `conv_channels` output channels, `conv_kernel`-wide,
/// 'same'-padded) + ReLU + global-max-pool *per channel* before being
/// concatenated back onto the scalar prefix; the combined vector (length
/// `(n_features - n_bands) + conv_channels`) then feeds the same dense
/// hidden/output layers as before. `n_bands == 0` (with `conv_channels`/
/// `conv_kernel` also 0 and `conv_w`/`conv_b` empty) degrades to exactly
/// the original plain dense-over-all-features architecture — the
/// `#[serde(default)]`s below mean an older on-disk/embedded model with no
/// conv fields at all still loads and predicts identically to how it
/// always did. A small conv layer over the frequency-band axis is a better
/// match for "learn what shape of spectral energy means a tap" than
/// treating each band as an unrelated, independently-weighted scalar the
/// dense layer has to rediscover the adjacency of on its own — the
/// project's ethos of no heavy ML crates just means this is hand-rolled
/// (forward + backward) rather than reached for `tch`/`candle`/etc, not
/// that a small conv is off the table.
#[derive(Clone, Serialize, Deserialize)]
pub struct TapModel {
    pub feature_schema_version: u32,
    pub n_features: usize,
    pub n_hidden: usize,
    pub n_classes: usize,
    /// Trailing slice length of the feature vector treated as an ordered
    /// band sequence for the conv path; `0` disables it entirely.
    #[serde(default)]
    pub n_bands: usize,
    #[serde(default)]
    pub conv_channels: usize,
    /// Must be odd ('same' padding assumes `(conv_kernel - 1) / 2` per side).
    #[serde(default)]
    pub conv_kernel: usize,
    /// `[conv_channels][conv_kernel]`
    #[serde(default)]
    pub conv_w: Vec<Vec<f32>>,
    /// `[conv_channels]`
    #[serde(default)]
    pub conv_b: Vec<f32>,
    /// `[(n_features - n_bands) + conv_channels][n_hidden]`
    pub w1: Vec<Vec<f32>>,
    /// `[n_hidden]`
    pub b1: Vec<f32>,
    /// `[n_hidden][n_classes]`
    pub w2: Vec<Vec<f32>>,
    /// `[n_classes]`
    pub b2: Vec<f32>,
    /// `[n_features]` z-score normalization, computed once at training time.
    pub mean: Vec<f32>,
    /// `[n_features]`
    pub std: Vec<f32>,
    /// `[n_classes]`, e.g. `["none", "tap"]`. `class != 0` is "a tap" —
    /// nothing downstream branches on any name past index 0.
    pub class_names: Vec<String>,
    /// Decision-confidence bar (`predict().confidence` must clear this),
    /// tunable per-retrain instead of a compile-time const.
    pub confidence_threshold: f32,
    pub trained_at_unix_ms: u64,
    pub training_rows: usize,
    pub source: ModelSource,
}

/// One forward pass's result.
#[derive(Debug, Clone)]
pub struct PredictResult {
    pub class: usize,
    pub confidence: f32,
    pub probabilities: Vec<f32>,
}

/// Intermediate values from the conv+pool path, kept only when training
/// needs to backprop through them — `predict` discards this.
struct ConvForward {
    /// `[conv_channels]` — global-max-pooled, post-ReLU output per channel.
    pooled: Vec<f32>,
    /// `[conv_channels]` — the band-axis index each channel's max came from.
    argmax_t: Vec<usize>,
    /// Zero-padded band input, cached so backward doesn't need to redo the
    /// padding to recover the exact values each `argmax_t` position saw.
    padded: Vec<f32>,
}

/// 'Same'-padded 1D convolution over `band_part` (one input channel) into
/// `conv_channels` output channels, ReLU, then global-max-pool per channel.
/// `conv_kernel` must be odd (checked by [`TapModel::validate`]).
fn conv_forward(band_part: &[f32], conv_w: &[Vec<f32>], conv_b: &[f32], conv_channels: usize, conv_kernel: usize) -> ConvForward {
    let n_bands = band_part.len();
    let pad = (conv_kernel - 1) / 2;
    let mut padded = vec![0.0f32; n_bands + 2 * pad];
    padded[pad..pad + n_bands].copy_from_slice(band_part);

    let mut pooled = vec![0.0f32; conv_channels];
    let mut argmax_t = vec![0usize; conv_channels];
    for c in 0..conv_channels {
        let mut best = 0.0f32; // ReLU floor: a channel that never activates pools to 0.
        let mut best_t = 0usize;
        for t in 0..n_bands {
            let mut y = conv_b[c];
            for k in 0..conv_kernel {
                y += conv_w[c][k] * padded[t + k];
            }
            let h = y.max(0.0);
            if h > best {
                best = h;
                best_t = t;
            }
        }
        pooled[c] = best;
        argmax_t[c] = best_t;
    }
    ConvForward { pooled, argmax_t, padded }
}

/// Builds the dense layer's actual input vector from a normalized feature
/// vector `x`: unchanged if `n_bands == 0` (the original plain-dense
/// architecture), otherwise the scalar prefix with the conv+pool result of
/// the trailing band sequence appended. The second return value is `None`
/// exactly when there's no conv path to backprop through.
fn hidden_input_for(
    x: &[f32],
    n_bands: usize,
    conv_w: &[Vec<f32>],
    conv_b: &[f32],
    conv_channels: usize,
    conv_kernel: usize,
) -> (Vec<f32>, Option<ConvForward>) {
    if n_bands == 0 {
        return (x.to_vec(), None);
    }
    let n_scalar = x.len() - n_bands;
    let (scalar_part, band_part) = x.split_at(n_scalar);
    let conv = conv_forward(band_part, conv_w, conv_b, conv_channels, conv_kernel);
    let mut combined = scalar_part.to_vec();
    combined.extend_from_slice(&conv.pooled);
    (combined, Some(conv))
}

impl TapModel {
    /// The checked-in baseline every fresh install starts from. Parsing a
    /// failure here is a build-time bug (a hand-edited `default_model.json`
    /// that no longer matches the shape fields), not a runtime condition to
    /// recover from.
    pub fn embedded_default() -> TapModel {
        let model: TapModel = serde_json::from_str(DEFAULT_MODEL_JSON)
            .expect("crates/tap-model/default_model.json must parse as TapModel");
        model.validate().expect("embedded default_model.json must be internally consistent");
        model
    }

    /// Cross-checks every array's shape against the declared
    /// `n_features`/`n_hidden`/`n_classes`, and the schema version against
    /// this crate's compiled-in [`FEATURE_SCHEMA_VERSION`]. `predict` never
    /// bounds-checks its indexing, so this must run before a model is ever
    /// stored or swapped in.
    pub fn validate(&self) -> Result<(), String> {
        if self.feature_schema_version != FEATURE_SCHEMA_VERSION {
            return Err(format!(
                "feature schema mismatch: file has {}, this build expects {FEATURE_SCHEMA_VERSION}",
                self.feature_schema_version
            ));
        }
        if self.n_features == 0 || self.n_hidden == 0 || self.n_classes < 2 {
            return Err(format!(
                "degenerate dimensions: n_features={} n_hidden={} n_classes={}",
                self.n_features, self.n_hidden, self.n_classes
            ));
        }
        if self.n_bands > self.n_features {
            return Err("n_bands exceeds n_features".into());
        }
        if self.n_bands > 0 {
            if self.conv_kernel == 0 || self.conv_kernel % 2 == 0 {
                return Err("conv_kernel must be a positive odd number".into());
            }
            if self.conv_channels == 0 {
                return Err("conv_channels must be positive when n_bands > 0".into());
            }
            if self.conv_w.len() != self.conv_channels
                || self.conv_w.iter().any(|row| row.len() != self.conv_kernel)
            {
                return Err("conv_w shape mismatch".into());
            }
            if self.conv_b.len() != self.conv_channels {
                return Err("conv_b shape mismatch".into());
            }
        } else if self.conv_channels != 0 || self.conv_kernel != 0 || !self.conv_w.is_empty() || !self.conv_b.is_empty() {
            return Err("conv fields must be empty/zero when n_bands is 0".into());
        }
        let hidden_input_dim = (self.n_features - self.n_bands) + self.conv_channels;
        if self.w1.len() != hidden_input_dim || self.w1.iter().any(|row| row.len() != self.n_hidden) {
            return Err("w1 shape mismatch".into());
        }
        if self.b1.len() != self.n_hidden {
            return Err("b1 shape mismatch".into());
        }
        if self.w2.len() != self.n_hidden || self.w2.iter().any(|row| row.len() != self.n_classes) {
            return Err("w2 shape mismatch".into());
        }
        if self.b2.len() != self.n_classes {
            return Err("b2 shape mismatch".into());
        }
        if self.mean.len() != self.n_features || self.std.len() != self.n_features {
            return Err("mean/std shape mismatch".into());
        }
        if self.class_names.len() != self.n_classes {
            return Err("class_names shape mismatch".into());
        }
        if self.std.iter().any(|s| !s.is_finite() || *s <= 0.0) {
            return Err("std must be positive and finite".into());
        }
        Ok(())
    }

    /// Missing file, I/O error, corrupt JSON, or a failed [`validate`] all
    /// fall back to [`Self::embedded_default`] rather than propagating —
    /// callers on the realtime audio path must never be left without a
    /// usable model.
    pub fn load_or_default(path: &Path) -> TapModel {
        Self::load_from_file(path).unwrap_or_else(|_| Self::embedded_default())
    }

    pub fn load_from_file(path: &Path) -> Result<TapModel, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let model: TapModel = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        model.validate()?;
        Ok(model)
    }

    /// Write-tmp-then-rename: on Windows, `rename` over an existing path is
    /// backed by `MoveFileExW`'s replace-existing semantics, so a
    /// concurrent reader (the hot-swap poll thread) never observes a
    /// half-written file.
    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).expect("TapModel always serializes");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// One forward pass. `features.len()` must equal `self.n_features` —
    /// callers control both sides (the same `build_feature_vector` always
    /// produces `FEATURE_SCHEMA_VERSION`'s feature count) so this asserts
    /// rather than returning a `Result` for a condition that can only mean a
    /// programming error, not bad input data.
    pub fn predict(&self, features: &[f32]) -> PredictResult {
        assert_eq!(features.len(), self.n_features, "feature vector length mismatch");
        let x: Vec<f32> = (0..self.n_features)
            .map(|i| (features[i] - self.mean[i]) / self.std[i])
            .collect();
        let (hidden_input, _conv_cache) =
            hidden_input_for(&x, self.n_bands, &self.conv_w, &self.conv_b, self.conv_channels, self.conv_kernel);
        let hidden: Vec<f32> = (0..self.n_hidden)
            .map(|j| {
                let mut sum = self.b1[j];
                for i in 0..hidden_input.len() {
                    sum += hidden_input[i] * self.w1[i][j];
                }
                sum.tanh()
            })
            .collect();
        let logits: Vec<f32> = (0..self.n_classes)
            .map(|k| {
                let mut z = self.b2[k];
                for j in 0..self.n_hidden {
                    z += hidden[j] * self.w2[j][k];
                }
                z
            })
            .collect();
        let max = logits.iter().cloned().fold(f32::MIN, f32::max);
        let exps: Vec<f32> = logits.iter().map(|z| (z - max).exp()).collect();
        let sum: f32 = exps.iter().sum::<f32>().max(1e-9);
        let probabilities: Vec<f32> = exps.iter().map(|e| e / sum).collect();
        let (class, &confidence) = probabilities
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        PredictResult { class, confidence, probabilities }
    }
}

/// `%APPDATA%\org.djimic.control` — used by `detect-test`, which has no
/// `AppHandle`. The real app resolves the identical physical folder via
/// Tauri's own `app_handle.path().app_data_dir()` instead of this (both are
/// documented to agree in the standard, non-redirected-profile case).
pub fn app_data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join(APP_IDENTIFIER))
}

pub fn model_file_path() -> Option<PathBuf> {
    app_data_dir().map(|dir| dir.join("tap_model.json"))
}

/// Lock-free hot-swap point for the live model: the realtime audio callback
/// calls [`TapModelStore::current`] every chunk (a couple of atomic ops, no
/// blocking, no allocation beyond the returned `Arc` clone), while training
/// or a file-poll reload calls [`TapModelStore::swap`] from an entirely
/// different thread. Any chunk already mid-flight keeps its own `Arc` alive
/// via refcounting even if a swap lands mid-callback.
pub struct TapModelStore(ArcSwap<TapModel>);

impl TapModelStore {
    pub fn new(initial: TapModel) -> Self {
        TapModelStore(ArcSwap::from_pointee(initial))
    }

    pub fn current(&self) -> Arc<TapModel> {
        self.0.load_full()
    }

    pub fn swap(&self, new_model: TapModel) {
        self.0.store(Arc::new(new_model));
    }
}

// ---------------------------------------------------------------------
// Training — full-batch gradient descent, ported from detect-test's
// original `run_train`. `train` fits from scratch (random init); with the
// `binary tap-vs-none` decision below, both share the same numerical core.
// ---------------------------------------------------------------------

/// Tiny deterministic xorshift64 PRNG — avoids pulling in `rand` for the
/// handful of random numbers weight init needs.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(if seed == 0 { 0x9E3779B97F4A7C15 } else { seed })
    }

    fn next_unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 as f64 / u64::MAX as f64) as f32 * 2.0 - 1.0
    }

    /// Standard-normal sample via Box-Muller, built on `next_unit` — used by
    /// `detect-test`'s data augmentation (jittering rare tap rows) rather
    /// than pulling in `rand_distr` for one distribution.
    pub fn next_gaussian(&mut self) -> f32 {
        let u1 = (((self.next_unit() + 1.0) * 0.5) as f32).clamp(1e-6, 1.0 - 1e-6);
        let u2 = ((self.next_unit() + 1.0) * 0.5) as f32;
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
    }
}

pub struct TrainConfig {
    pub n_hidden: usize,
    pub n_classes: usize,
    pub epochs: u32,
    pub lr: f32,
    pub l2: f32,
    pub class_names: Vec<String>,
    pub confidence_threshold: f32,
    pub seed: u64,
    /// `0` disables the conv path (plain dense net over all `n_features`).
    /// Ignored by [`continue_training`], which always inherits the warm-
    /// started model's own architecture instead — see its doc comment.
    pub n_bands: usize,
    pub conv_channels: usize,
    /// Must be odd.
    pub conv_kernel: usize,
}

#[derive(Debug, Clone)]
pub struct TrainReport {
    pub rows: usize,
    pub class_counts: Vec<usize>,
    pub final_loss: f32,
    pub train_accuracy: f32,
    /// `[actual][predicted]`
    pub confusion: Vec<Vec<usize>>,
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// sqrt-inverse-frequency class weighting: full inverse-frequency
/// over-corrects at this class's typical ~100:1+ imbalance (the rare class
/// ends up drowning out the common one, spiking false positives); the
/// square root compresses the correction while still giving the minority
/// class real weight during training.
fn class_weights(counts: &[usize], n_classes: usize, n: usize) -> Vec<f32> {
    (0..n_classes)
        .map(|k| {
            if counts[k] > 0 {
                (n as f32 / (n_classes as f32 * counts[k] as f32)).sqrt()
            } else {
                0.0
            }
        })
        .collect()
}

fn forward_hidden(hidden_input: &[f32], w1: &[Vec<f32>], b1: &[f32], n_hidden: usize) -> Vec<f32> {
    (0..n_hidden)
        .map(|j| {
            let mut sum = b1[j];
            for (i, &xi) in hidden_input.iter().enumerate() {
                sum += xi * w1[i][j];
            }
            sum.tanh()
        })
        .collect()
}

fn softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().cloned().fold(f32::MIN, f32::max);
    let exps: Vec<f32> = logits.iter().map(|z| (z - max).exp()).collect();
    let sum: f32 = exps.iter().sum::<f32>().max(1e-9);
    exps.iter().map(|e| e / sum).collect()
}

/// One full-batch gradient-descent run, either from a fresh random
/// initialization (`init` is `None`) or warm-started from an existing
/// model's weights and normalization (`init` is `Some`, used by
/// [`continue_training`] so an incremental update fine-tunes rather than
/// re-fitting from scratch and shifting the input normalization out from
/// under otherwise-still-valid weights).
fn train_inner(
    rows: &[(Vec<f32>, usize)],
    n_features: usize,
    cfg: &TrainConfig,
    init: Option<&TapModel>,
) -> (TapModel, TrainReport) {
    assert!(!rows.is_empty(), "train requires at least one row");
    let n = rows.len();

    // A warm start always inherits the base model's own architecture
    // (n_bands/conv_channels/conv_kernel) rather than `cfg`'s — an
    // incremental update can't change the shape of what it's fine-tuning.
    let (n_bands, conv_channels, conv_kernel) = match init {
        Some(m) => (m.n_bands, m.conv_channels, m.conv_kernel),
        None => (cfg.n_bands, cfg.conv_channels, cfg.conv_kernel),
    };
    let n_scalar = n_features - n_bands;
    let hidden_input_dim = n_scalar + conv_channels;

    let (mean, std) = match init {
        Some(m) => (m.mean.clone(), m.std.clone()),
        None => {
            let mut mean = vec![0.0f32; n_features];
            for (f, _) in rows {
                for i in 0..n_features {
                    mean[i] += f[i];
                }
            }
            for m in &mut mean {
                *m /= n as f32;
            }
            let mut std = vec![0.0f32; n_features];
            for (f, _) in rows {
                for i in 0..n_features {
                    std[i] += (f[i] - mean[i]).powi(2);
                }
            }
            for s in &mut std {
                *s = (*s / n as f32).sqrt().max(1e-6);
            }
            (mean, std)
        }
    };

    let normalized: Vec<(Vec<f32>, usize)> = rows
        .iter()
        .map(|(f, c)| ((0..n_features).map(|i| (f[i] - mean[i]) / std[i]).collect(), *c))
        .collect();

    let mut counts = vec![0usize; cfg.n_classes];
    for (_, c) in &normalized {
        counts[*c] += 1;
    }
    let weight = class_weights(&counts, cfg.n_classes, n);

    let mut rng = Rng::new(cfg.seed);
    let (mut conv_w, mut conv_b, mut w1, mut b1, mut w2, mut b2) = match init {
        Some(m) => (m.conv_w.clone(), m.conv_b.clone(), m.w1.clone(), m.b1.clone(), m.w2.clone(), m.b2.clone()),
        None => {
            let conv_w = (0..conv_channels)
                .map(|_| (0..conv_kernel).map(|_| rng.next_unit() * 0.3).collect())
                .collect();
            let conv_b = vec![0.0f32; conv_channels];
            let w1 = (0..hidden_input_dim)
                .map(|_| (0..cfg.n_hidden).map(|_| rng.next_unit() * 0.3).collect())
                .collect();
            let b1 = vec![0.0f32; cfg.n_hidden];
            let w2 = (0..cfg.n_hidden)
                .map(|_| (0..cfg.n_classes).map(|_| rng.next_unit() * 0.3).collect())
                .collect();
            let b2 = vec![0.0f32; cfg.n_classes];
            (conv_w, conv_b, w1, b1, w2, b2)
        }
    };

    let mut final_loss = 0.0f32;
    const EPS: f32 = 1e-7;
    for _epoch in 0..cfg.epochs {
        let mut grad_conv_w = vec![vec![0.0f32; conv_kernel]; conv_channels];
        let mut grad_conv_b = vec![0.0f32; conv_channels];
        let mut grad_w1 = vec![vec![0.0f32; cfg.n_hidden]; hidden_input_dim];
        let mut grad_b1 = vec![0.0f32; cfg.n_hidden];
        let mut grad_w2 = vec![vec![0.0f32; cfg.n_classes]; cfg.n_hidden];
        let mut grad_b2 = vec![0.0f32; cfg.n_classes];
        let mut loss = 0.0f32;

        for (x, class) in &normalized {
            let (hidden_input, conv_cache) = hidden_input_for(x, n_bands, &conv_w, &conv_b, conv_channels, conv_kernel);
            let hidden = forward_hidden(&hidden_input, &w1, &b1, cfg.n_hidden);
            let logits: Vec<f32> = (0..cfg.n_classes)
                .map(|k| {
                    let mut z = b2[k];
                    for j in 0..cfg.n_hidden {
                        z += hidden[j] * w2[j][k];
                    }
                    z
                })
                .collect();
            let probs = softmax(&logits);
            let w = weight[*class];
            loss -= w * (probs[*class] + EPS).ln();

            let dz: Vec<f32> = (0..cfg.n_classes)
                .map(|k| w * (probs[k] - if k == *class { 1.0 } else { 0.0 }))
                .collect();
            for j in 0..cfg.n_hidden {
                for k in 0..cfg.n_classes {
                    grad_w2[j][k] += dz[k] * hidden[j];
                }
            }
            for k in 0..cfg.n_classes {
                grad_b2[k] += dz[k];
            }
            let mut d_hidden_input = vec![0.0f32; hidden_input_dim];
            for j in 0..cfg.n_hidden {
                let dh: f32 = (0..cfg.n_classes).map(|k| dz[k] * w2[j][k]).sum();
                let dh_raw = dh * (1.0 - hidden[j] * hidden[j]);
                grad_b1[j] += dh_raw;
                for (i, &xi) in hidden_input.iter().enumerate() {
                    grad_w1[i][j] += dh_raw * xi;
                    d_hidden_input[i] += dh_raw * w1[i][j];
                }
            }

            // Backprop into the conv+pool path, if any — the band sequence
            // itself is a leaf input (nothing upstream needs its gradient),
            // so only conv_w/conv_b need updating. Max-pool routes the
            // gradient to exactly the winning band position; a channel that
            // never activated (pooled == 0) gets no gradient this sample.
            if let Some(conv) = &conv_cache {
                let d_pooled = &d_hidden_input[n_scalar..];
                for c in 0..conv_channels {
                    if conv.pooled[c] > 0.0 {
                        let t = conv.argmax_t[c];
                        let dy = d_pooled[c];
                        grad_conv_b[c] += dy;
                        for k in 0..conv_kernel {
                            grad_conv_w[c][k] += dy * conv.padded[t + k];
                        }
                    }
                }
            }
        }

        if n_bands > 0 {
            for c in 0..conv_channels {
                conv_b[c] -= cfg.lr * grad_conv_b[c] / n as f32;
                for k in 0..conv_kernel {
                    conv_w[c][k] -= cfg.lr * (grad_conv_w[c][k] / n as f32 + cfg.l2 * conv_w[c][k]);
                }
            }
        }
        for i in 0..hidden_input_dim {
            for j in 0..cfg.n_hidden {
                w1[i][j] -= cfg.lr * (grad_w1[i][j] / n as f32 + cfg.l2 * w1[i][j]);
            }
        }
        for j in 0..cfg.n_hidden {
            b1[j] -= cfg.lr * grad_b1[j] / n as f32;
            for k in 0..cfg.n_classes {
                w2[j][k] -= cfg.lr * (grad_w2[j][k] / n as f32 + cfg.l2 * w2[j][k]);
            }
        }
        for k in 0..cfg.n_classes {
            b2[k] -= cfg.lr * grad_b2[k] / n as f32;
        }
        final_loss = loss / n as f32;
    }

    let mut confusion = vec![vec![0usize; cfg.n_classes]; cfg.n_classes];
    for (x, class) in &normalized {
        let (hidden_input, _) = hidden_input_for(x, n_bands, &conv_w, &conv_b, conv_channels, conv_kernel);
        let hidden = forward_hidden(&hidden_input, &w1, &b1, cfg.n_hidden);
        let logits: Vec<f32> = (0..cfg.n_classes)
            .map(|k| {
                let mut z = b2[k];
                for j in 0..cfg.n_hidden {
                    z += hidden[j] * w2[j][k];
                }
                z
            })
            .collect();
        let probs = softmax(&logits);
        let predicted = probs.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).unwrap().0;
        confusion[*class][predicted] += 1;
    }
    let correct: usize = (0..cfg.n_classes).map(|k| confusion[k][k]).sum();
    let train_accuracy = correct as f32 / normalized.len() as f32;

    let model = TapModel {
        feature_schema_version: FEATURE_SCHEMA_VERSION,
        n_features,
        n_hidden: cfg.n_hidden,
        n_classes: cfg.n_classes,
        n_bands,
        conv_channels,
        conv_kernel,
        conv_w,
        conv_b,
        w1,
        b1,
        w2,
        b2,
        mean,
        std,
        class_names: cfg.class_names.clone(),
        confidence_threshold: cfg.confidence_threshold,
        trained_at_unix_ms: now_unix_ms(),
        training_rows: n,
        source: if init.is_some() { ModelSource::Incremental } else { ModelSource::FullRetrain },
    };
    let report = TrainReport { rows: n, class_counts: counts, final_loss, train_accuracy, confusion };
    (model, report)
}

/// Fits a brand-new model from scratch (random weight init, mean/std
/// recomputed over `rows`). This is what `detect-test train` runs.
pub fn train(rows: &[(Vec<f32>, usize)], n_features: usize, cfg: &TrainConfig) -> (TapModel, TrainReport) {
    train_inner(rows, n_features, cfg, None)
}

/// Warm-started fine-tune: continues from `base`'s weights and — crucially —
/// its existing `mean`/`std`, so the input normalization a feedback-driven
/// update trains against matches what `base` was already fit on (recomputing
/// normalization from scratch would shift every existing weight's meaning
/// out from under it). `n_hidden`/`n_classes`/`class_names`/
/// `confidence_threshold` in `cfg` should normally just be copied from
/// `base` — this only takes a full `TrainConfig` so callers can still retune
/// `epochs`/`lr`/`l2`/`seed` for a smaller, gentler update than a full
/// retrain.
pub fn continue_training(
    base: &TapModel,
    rows: &[(Vec<f32>, usize)],
    n_features: usize,
    cfg: &TrainConfig,
) -> (TapModel, TrainReport) {
    train_inner(rows, n_features, cfg, Some(base))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_default_is_valid() {
        let model = TapModel::embedded_default();
        assert!(model.validate().is_ok());
        // Sanity: predicting on the model's own mean feature vector should
        // not panic and should return a probability distribution summing to ~1.
        let features = model.mean.clone();
        let result = model.predict(&features);
        let sum: f32 = result.probabilities.iter().sum();
        assert!((sum - 1.0).abs() < 1e-3);
    }

    #[test]
    fn train_then_predict_roundtrip() {
        let mut rows = Vec::new();
        for i in 0..200 {
            rows.push((vec![0.0, 0.0, 0.0, 0.0], 0usize));
            let _ = i;
        }
        for _ in 0..40 {
            rows.push((vec![5.0, 5.0, 5.0, 5.0], 1usize));
        }
        let cfg = TrainConfig {
            n_hidden: 4,
            n_classes: 2,
            epochs: 200,
            lr: 0.2,
            l2: 1e-4,
            class_names: vec!["none".into(), "tap".into()],
            confidence_threshold: 0.5,
            seed: 1,
            n_bands: 0,
            conv_channels: 0,
            conv_kernel: 0,
        };
        let (model, report) = train(&rows, 4, &cfg);
        assert!(model.validate().is_ok());
        assert!(report.train_accuracy > 0.9);
        let none_pred = model.predict(&[0.0, 0.0, 0.0, 0.0]);
        let tap_pred = model.predict(&[5.0, 5.0, 5.0, 5.0]);
        assert_eq!(none_pred.class, 0);
        assert_eq!(tap_pred.class, 1);
    }

    #[test]
    fn conv_path_trains_and_predicts() {
        // 2 scalar features + 4 "band" features (n_features=6, n_bands=4).
        // Class 1's band sequence is a sharp bump (tap-like broadband
        // transient shape); class 0's is flat (ambient-noise-like) — a conv
        // over the band axis should tell these apart more easily than an
        // unstructured dense layer would need many more samples to.
        let mut rows = Vec::new();
        for _ in 0..150 {
            rows.push((vec![0.0, 0.0, 0.1, 0.1, 0.1, 0.1], 0usize));
        }
        for _ in 0..150 {
            rows.push((vec![3.0, 3.0, 0.1, 4.0, 4.0, 0.1], 1usize));
        }
        let cfg = TrainConfig {
            n_hidden: 6,
            n_classes: 2,
            epochs: 300,
            lr: 0.15,
            l2: 1e-4,
            class_names: vec!["none".into(), "tap".into()],
            confidence_threshold: 0.5,
            seed: 7,
            n_bands: 4,
            conv_channels: 3,
            conv_kernel: 3,
        };
        let (model, report) = train(&rows, 6, &cfg);
        assert!(model.validate().is_ok(), "{:?}", model.validate());
        assert_eq!(model.n_bands, 4);
        assert_eq!(model.conv_w.len(), 3);
        assert!(report.train_accuracy > 0.9, "train_accuracy={}", report.train_accuracy);
        let none_pred = model.predict(&[0.0, 0.0, 0.1, 0.1, 0.1, 0.1]);
        let tap_pred = model.predict(&[3.0, 3.0, 0.1, 4.0, 4.0, 0.1]);
        assert_eq!(none_pred.class, 0);
        assert_eq!(tap_pred.class, 1);
    }

    /// Verifies the hand-derived conv backward pass against a numeric
    /// (finite-difference) gradient — the standard way to check hand-rolled
    /// backprop math without an autodiff framework. Checks both a conv
    /// weight and a conv bias.
    #[test]
    fn conv_backward_matches_finite_difference() {
        let n_scalar = 2;
        let n_bands = 5;
        let n_features = n_scalar + n_bands;
        let n_hidden = 3;
        let n_classes = 2;
        let conv_channels = 2;
        let conv_kernel = 3;
        let hidden_input_dim = n_scalar + conv_channels;

        let mut rng = Rng::new(42);
        let x: Vec<f32> = (0..n_features).map(|_| rng.next_unit()).collect();
        let class = 1usize;

        let conv_w: Vec<Vec<f32>> =
            (0..conv_channels).map(|_| (0..conv_kernel).map(|_| rng.next_unit() * 0.5).collect()).collect();
        let conv_b: Vec<f32> = (0..conv_channels).map(|_| rng.next_unit() * 0.2).collect();
        let w1: Vec<Vec<f32>> =
            (0..hidden_input_dim).map(|_| (0..n_hidden).map(|_| rng.next_unit() * 0.5).collect()).collect();
        let b1: Vec<f32> = (0..n_hidden).map(|_| rng.next_unit() * 0.2).collect();
        let w2: Vec<Vec<f32>> = (0..n_hidden).map(|_| (0..n_classes).map(|_| rng.next_unit() * 0.5).collect()).collect();
        let b2: Vec<f32> = (0..n_classes).map(|_| rng.next_unit() * 0.2).collect();

        let loss_for = |conv_w: &Vec<Vec<f32>>, conv_b: &Vec<f32>| -> f32 {
            let (hidden_input, _) = hidden_input_for(&x, n_bands, conv_w, conv_b, conv_channels, conv_kernel);
            let hidden = forward_hidden(&hidden_input, &w1, &b1, n_hidden);
            let logits: Vec<f32> = (0..n_classes)
                .map(|k| {
                    let mut z = b2[k];
                    for j in 0..n_hidden {
                        z += hidden[j] * w2[j][k];
                    }
                    z
                })
                .collect();
            let probs = softmax(&logits);
            -(probs[class] + 1e-7).ln()
        };

        // Analytic gradient — the same math as one `train_inner` sample step.
        let (hidden_input, conv_cache) = hidden_input_for(&x, n_bands, &conv_w, &conv_b, conv_channels, conv_kernel);
        let hidden = forward_hidden(&hidden_input, &w1, &b1, n_hidden);
        let logits: Vec<f32> = (0..n_classes)
            .map(|k| {
                let mut z = b2[k];
                for j in 0..n_hidden {
                    z += hidden[j] * w2[j][k];
                }
                z
            })
            .collect();
        let probs = softmax(&logits);
        let dz: Vec<f32> = (0..n_classes).map(|k| probs[k] - if k == class { 1.0 } else { 0.0 }).collect();
        let mut d_hidden_input = vec![0.0f32; hidden_input_dim];
        for j in 0..n_hidden {
            let dh: f32 = (0..n_classes).map(|k| dz[k] * w2[j][k]).sum();
            let dh_raw = dh * (1.0 - hidden[j] * hidden[j]);
            for i in 0..hidden_input_dim {
                d_hidden_input[i] += dh_raw * w1[i][j];
            }
        }
        let conv = conv_cache.expect("n_bands > 0 must produce a conv cache");
        let d_pooled = &d_hidden_input[n_scalar..];
        let mut grad_conv_w = vec![vec![0.0f32; conv_kernel]; conv_channels];
        let mut grad_conv_b = vec![0.0f32; conv_channels];
        for c in 0..conv_channels {
            if conv.pooled[c] > 0.0 {
                let t = conv.argmax_t[c];
                grad_conv_b[c] = d_pooled[c];
                for k in 0..conv_kernel {
                    grad_conv_w[c][k] = d_pooled[c] * conv.padded[t + k];
                }
            }
        }

        let eps = 1e-3;
        // Check conv_w[0][0].
        let mut w_plus = conv_w.clone();
        w_plus[0][0] += eps;
        let mut w_minus = conv_w.clone();
        w_minus[0][0] -= eps;
        let numeric = (loss_for(&w_plus, &conv_b) - loss_for(&w_minus, &conv_b)) / (2.0 * eps);
        assert!(
            (numeric - grad_conv_w[0][0]).abs() < 1e-2,
            "conv_w grad mismatch: analytic={} numeric={}",
            grad_conv_w[0][0],
            numeric
        );

        // Check conv_b[1].
        let mut b_plus = conv_b.clone();
        b_plus[1] += eps;
        let mut b_minus = conv_b.clone();
        b_minus[1] -= eps;
        let numeric_b = (loss_for(&conv_w, &b_plus) - loss_for(&conv_w, &b_minus)) / (2.0 * eps);
        assert!(
            (numeric_b - grad_conv_b[1]).abs() < 1e-2,
            "conv_b grad mismatch: analytic={} numeric={}",
            grad_conv_b[1],
            numeric_b
        );
    }
}
