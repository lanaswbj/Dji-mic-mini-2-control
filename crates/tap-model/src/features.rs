//! DSP feature extraction and voice-activity gating, shared by
//! `gui/src-tauri/src/mic_tap.rs` and `test-tools/detect-test/src/main.rs`
//! so this math has exactly one home instead of being hand-copied into
//! both (the per-stream detection *state machine* — debounce, confirm-
//! delay, pie-menu wiring vs. plain console logging — legitimately still
//! differs between the two and stays duplicated, per this project's own
//! documented reasoning for keeping detect-test's port "confirmed working
//! standalone before it's the copy that matters").
//!
//! Feature schema v2 (bumped from v1's plain 8-scalar vector): each chunk
//! now also gets a small ordered frequency-band profile (see `N_BANDS`)
//! instead of just two collapsed summary scalars (centroid/hl_ratio), plus
//! two more spectral summaries (rolloff/flatness) and four temporal/attack-
//! shape features. The bands are deliberately placed at the *end* of the
//! feature vector (see `build_feature_vector`) so `TapModel`'s optional 1D-
//! conv path (see its doc comment) can treat them as one contiguous ordered
//! sequence to convolve over, while the rest stay a plain scalar bag for the
//! dense layer.

use std::time::Instant;

/// Bumped whenever this module's feature order/count changes incompatibly —
/// mirrors `tap_model::FEATURE_SCHEMA_VERSION`, which a loaded `TapModel`
/// is checked against.
pub const N_BASE_SCALARS: usize = 8;
/// Log-spaced center frequencies from 150Hz-8kHz — wide enough to span the
/// mechanical-knock/vocal-transient distinction the project's own notes
/// describe, without so many bins that Goertzel's O(N·bins) cost stops
/// being trivial at typical chunk sizes.
pub const N_BANDS: usize = 12;
/// rolloff, flatness, attack_pos, energy_skew, delta_ratio, delta_novelty
pub const N_TEMPORAL: usize = 6;
pub const N_FEATURES: usize = N_BASE_SCALARS + N_TEMPORAL + N_BANDS;
/// Where the band sequence starts within the feature vector — everything
/// before this index is plain scalars; `TapModel::predict`'s conv path (if
/// enabled) slices from here to the end.
pub const N_SCALARS: usize = N_BASE_SCALARS + N_TEMPORAL;

/// This device's audio config sample rate (confirmed via `DJIMIC_DEBUG=1`
/// logging — `[mic_tap] opening ... rate=48000`), used to place the
/// Goertzel analysis bins. Hardcoded rather than threaded through every
/// call site since this specific receiver has only ever been observed at
/// this rate.
pub const SAMPLE_RATE_HZ: f32 = 48000.0;

/// Log-spaced 150Hz-8kHz, computed once as literals (a `const fn` can't
/// call `powf`) — regenerate with
/// `for i in 0..12 { println!("{}", 150.0 * (8000.0f32/150.0).powf(i as f32/11.0)) }`
/// if `N_BANDS` ever changes.
const BAND_FREQS: [f32; N_BANDS] = [
    150.0, 215.3, 308.9, 443.3, 636.1, 912.8, 1310.0, 1880.0, 2698.0, 3871.0, 5553.0, 7967.0,
];

/// Goertzel algorithm: signal magnitude at one target frequency, without
/// needing a full FFT (or an FFT crate) for just a handful of bins.
pub fn goertzel_magnitude(samples: &[i16], target_hz: f32) -> f32 {
    if samples.len() < 8 {
        return 0.0;
    }
    let n = samples.len() as f32;
    let k = (0.5 + n * target_hz / SAMPLE_RATE_HZ).floor();
    let omega = 2.0 * std::f32::consts::PI * k / n;
    let coeff = 2.0 * omega.cos();
    let (mut s_prev, mut s_prev2) = (0.0f32, 0.0f32);
    for &sample in samples {
        let s = sample as f32 + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    (s_prev2 * s_prev2 + s_prev * s_prev - coeff * s_prev * s_prev2).max(0.0).sqrt()
}

/// The 12-band magnitude profile for one chunk — a coarse log-mel-style
/// spectral snapshot, giving the classifier real spectral *shape* instead
/// of the two collapsed scalars (centroid/hl_ratio) the original 4-band
/// version produced.
pub fn spectral_bands(samples: &[i16]) -> [f32; N_BANDS] {
    std::array::from_fn(|i| goertzel_magnitude(samples, BAND_FREQS[i]))
}

/// Derived summaries from the band profile: `(centroid_hz, high/low energy
/// ratio, rolloff_hz, flatness)`. Kept as explicit scalar features (in
/// addition to feeding the raw bands into the conv path) since they were
/// already proven useful in the original 4-band version.
pub fn spectral_summary(bands: &[f32; N_BANDS]) -> (f32, f32, f32, f32) {
    let total: f32 = bands.iter().sum::<f32>().max(1e-6);
    let centroid = BAND_FREQS.iter().zip(bands.iter()).map(|(f, m)| f * m).sum::<f32>() / total;
    let mid = N_BANDS / 2;
    let low: f32 = bands[..mid].iter().sum();
    let high: f32 = bands[mid..].iter().sum();
    let hl_ratio = high / low.max(1e-6);

    // Rolloff: the lowest band frequency below which 85% of total band
    // energy sits — a standard MIR descriptor for "how broadband is this".
    let target = 0.85 * total;
    let mut cum = 0.0f32;
    let mut rolloff = BAND_FREQS[N_BANDS - 1];
    for i in 0..N_BANDS {
        cum += bands[i];
        if cum >= target {
            rolloff = BAND_FREQS[i];
            break;
        }
    }

    // Flatness: geometric-mean / arithmetic-mean of the band magnitudes —
    // near 1.0 for noise-like/broadband energy (a hard knock), near 0 for
    // strongly tonal energy concentrated in a few bands (voiced speech).
    let log_sum: f32 = bands.iter().map(|m| (m.max(1e-6)).ln()).sum();
    let geo_mean = (log_sum / N_BANDS as f32).exp();
    let arith_mean = total / N_BANDS as f32;
    let flatness = geo_mean / arith_mean.max(1e-6);

    (centroid, hl_ratio, rolloff, flatness)
}

/// Spectral-flux based onset novelty (`microdsp::sfnov`) — a mature MIR
/// technique for detecting percussive onsets, the same family of algorithm
/// behind tools like aubio/essentia's onset detectors. Measures
/// frame-to-frame *change* in the spectrum, targeting "how sudden is this
/// attack" more reliably than a crest-factor heuristic.
pub struct NoveltyState {
    detector: microdsp::sfnov::SpectralFluxNoveltyDetector<microdsp::sfnov::HardKneeCompression>,
    last_novelty: f32,
}

/// Onset detector's internal analysis window. Smaller = lower latency but
/// less frequency resolution; 512 samples at 48kHz is ~10.7ms.
const NOVELTY_WINDOW_SIZE: usize = 512;

impl Default for NoveltyState {
    fn default() -> Self {
        NoveltyState {
            detector: microdsp::sfnov::SpectralFluxNoveltyDetector::new(NOVELTY_WINDOW_SIZE),
            last_novelty: 0.0,
        }
    }
}

impl NoveltyState {
    pub fn update(&mut self, samples: &[i16]) -> f32 {
        let float_samples: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
        let mut latest = self.last_novelty;
        self.detector.process(&float_samples, |flux| {
            latest = flux.novelty();
        });
        self.last_novelty = latest;
        latest
    }
}

/// Sample-index (normalized 0..1) of the chunk's peak-amplitude sample, and
/// the ratio of first-half to second-half RMS energy — a cheap proxy for
/// "is this an attack-then-decay shape or something more sustained",
/// folded directly into the feature vector rather than only judged
/// post-hoc by `SUSTAIN_DECAY_FRACTION`-style heuristics across chunks.
pub fn attack_shape(samples: &[i16]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.5, 0.0);
    }
    let (peak_idx, _) = samples
        .iter()
        .enumerate()
        .max_by_key(|(_, &s)| (s as i32).unsigned_abs())
        .unwrap();
    let attack_pos = peak_idx as f32 / samples.len().max(1) as f32;

    let mid = samples.len() / 2;
    let energy = |s: &[i16]| -> f64 { s.iter().map(|&v| (v as f64) * (v as f64)).sum() };
    let first = energy(&samples[..mid]).max(1.0);
    let second = energy(&samples[mid..]).max(1.0);
    let energy_skew = ((first / second) as f32).ln();
    (attack_pos, energy_skew)
}

/// Turns raw per-chunk measurements into the model's input vector. Feature
/// order: 8 base scalars, then 6 temporal/spectral-summary scalars
/// (rolloff, flatness, attack_pos, energy_skew, delta_ratio, delta_novelty),
/// then the `N_BANDS` band-magnitude sequence — see `N_SCALARS`/`N_BANDS`.
#[allow(clippy::too_many_arguments)]
pub fn build_feature_vector(
    peak: f32,
    rms: f32,
    ratio: f32,
    zcr: f32,
    novelty: f32,
    bands: &[f32; N_BANDS],
    attack_pos: f32,
    energy_skew: f32,
    delta_ratio: f32,
    delta_novelty: f32,
) -> [f32; N_FEATURES] {
    let crest = (peak / rms.max(1.0)).max(1.0);
    let (centroid, hl_ratio, rolloff, flatness) = spectral_summary(bands);
    let mut out = [0.0f32; N_FEATURES];
    out[0] = peak.max(1.0).ln();
    out[1] = rms.max(1.0).ln();
    out[2] = ratio.max(1.0).ln();
    out[3] = zcr;
    out[4] = crest.ln();
    out[5] = centroid / 1000.0;
    out[6] = hl_ratio.max(1e-3).ln();
    out[7] = novelty.max(1e-4).ln();
    out[8] = rolloff / 1000.0;
    out[9] = flatness;
    out[10] = attack_pos;
    out[11] = energy_skew;
    out[12] = delta_ratio;
    out[13] = delta_novelty;
    for i in 0..N_BANDS {
        out[N_SCALARS + i] = bands[i].max(1.0).ln();
    }
    out
}

// ---------------------------------------------------------------------
// Speech gating: the tap classifier alone still occasionally reads a sharp
// consonant or loud exclamation as a tap, since its features are single
// small-frame amplitude/spectral snapshots, not a real model of speech.
// Running a dedicated voice-activity detector alongside it and suppressing
// tap classification while it reports speech closes that gap.
// ---------------------------------------------------------------------

/// earshot operates on 16kHz mono, 256-sample (16ms) frames; this device
/// captures at 48kHz, so every 3rd raw sample is kept (after a one-pole
/// low-pass to avoid aliasing high-frequency speech content into noise the
/// VAD can't read).
const VAD_DOWNSAMPLE_RATIO: usize = 3;
const VAD_FRAME_LEN: usize = 256;
/// alpha = 1 - exp(-2*pi*fc/fs) for fc=7000Hz, fs=48000Hz.
const VAD_LOWPASS_ALPHA: f32 = 0.6;
/// earshot's own docs describe scores over 0.5 as "generally voice", which
/// is where this started out. Real-hardware `DJIMIC_DEBUG=1` testing then
/// showed a sharp mechanical tap's own broadband transient routinely pushes
/// earshot's score to 0.51-0.72 for a single frame — well above 0.5 but far
/// below what genuine sustained speech scores — which was cancelling ~40%
/// of otherwise-confident (conf 0.90-1.00) tap detections as false "VAD
/// caught up" speech-onset events. Raised to 0.8 so only clearly-confident
/// speech gates tap detection; retune further from real-hardware testing if
/// speech still slips through as taps, or if legitimate taps are still
/// getting speech-gated.
const VAD_SCORE_THRESHOLD: f32 = 0.8;
/// How long tap detection stays suppressed after the last speech frame —
/// covers the brief pauses within a sentence so the gate doesn't flicker
/// open between words.
const VAD_HOLDOVER_MS: u64 = 450;

/// Pure-Rust neural-net voice activity detector (replaces
/// `voice_activity_detector`/Silero, which pulled in `ort` plus a build-
/// time-downloaded ONNX Runtime purely for this one speech gate — the
/// single heaviest, most build-fragile dependency the app had). IMPORTANT
/// version note: this project already tried an `earshot` once before and
/// dropped it for "still letting too much speech through" — that was
/// `earshot` 0.1.x, a WebRTC/GMM-style port. `earshot` 1.x (used here) is a
/// from-scratch rewrite as a small embedded neural network by the same
/// author (who also maintains `ort`), a different algorithm entirely; its
/// accuracy claims are the author's own self-benchmark, so real-hardware
/// testing (does speech still slip through as taps, do real taps get
/// speech-gated) is what actually validates this swap, not the crate
/// switch by itself.
pub struct VadState {
    detector: earshot::Detector<earshot::DefaultPredictor>,
    lp_state: f32,
    sample_counter: u64,
    frame_buf: Vec<i16>,
    speech_until: Option<Instant>,
    pub last_score: f32,
}

impl Default for VadState {
    fn default() -> Self {
        VadState {
            detector: earshot::Detector::const_default(),
            lp_state: 0.0,
            sample_counter: 0,
            frame_buf: Vec::new(),
            speech_until: None,
            last_score: 0.0,
        }
    }
}

impl VadState {
    pub fn update(&mut self, samples: &[i16], now: Instant) {
        for &s in samples {
            self.lp_state += VAD_LOWPASS_ALPHA * (s as f32 - self.lp_state);
            self.sample_counter += 1;
            if self.sample_counter % VAD_DOWNSAMPLE_RATIO as u64 == 0 {
                self.frame_buf.push(self.lp_state.clamp(i16::MIN as f32, i16::MAX as f32) as i16);
            }
        }

        while self.frame_buf.len() >= VAD_FRAME_LEN {
            let frame = &self.frame_buf[..VAD_FRAME_LEN];
            self.last_score = self.detector.predict_i16(frame);
            if self.last_score > VAD_SCORE_THRESHOLD {
                self.speech_until = Some(now + std::time::Duration::from_millis(VAD_HOLDOVER_MS));
            }
            self.frame_buf.drain(0..VAD_FRAME_LEN);
        }
    }

    pub fn is_speech(&self, now: Instant) -> bool {
        self.speech_until.is_some_and(|t| now < t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_vector_has_expected_length() {
        let bands = [1.0f32; N_BANDS];
        let v = build_feature_vector(100.0, 50.0, 5.0, 0.1, 0.01, &bands, 0.5, 0.0, 0.0, 0.0);
        assert_eq!(v.len(), N_FEATURES);
    }

    #[test]
    fn silence_and_tone_give_different_band_profiles() {
        let silence = vec![0i16; 1024];
        let tone: Vec<i16> = (0..1024)
            .map(|i| (3000.0 * (2.0 * std::f32::consts::PI * 2000.0 * i as f32 / SAMPLE_RATE_HZ).sin()) as i16)
            .collect();
        let bands_silence = spectral_bands(&silence);
        let bands_tone = spectral_bands(&tone);
        assert!(bands_tone.iter().sum::<f32>() > bands_silence.iter().sum::<f32>() * 10.0);
    }
}
