//! Detects taps on the mic's shell via an audio-domain classifier, ported
//! from the standalone test tool at `test-tools/detect-test` (see that
//! crate's `main.rs` for the full history of how this was tuned — several
//! rounds of real hardware testing against a naive hand-tuned-threshold
//! version that didn't work).
//!
//! The receiver's physical buttons mostly don't produce any USB event at
//! all (see `pairing_button.rs` for the one that does, and its doc comment
//! for what was tried and ruled out for the others) — tapping the mic's
//! shell and picking up the resulting impact transient on its own audio
//! signal is a way to get more distinct gestures out of the hardware
//! without depending on undocumented button behavior.
//!
//! Only single and double taps are exposed — a burst of 3 or more taps is
//! deliberately still reported as a double tap (see `report_count`) rather
//! than adding a third gesture, since nail-vs-pad splitting the training
//! data already accounts for most of what a third class would've bought.
//!
//! Pipeline per audio chunk:
//! - `RMS`/`PEAK`/`RATIO` (peak vs. a 20-chunk rolling baseline) — basic
//!   dynamics.
//! - 4-band Goertzel spectral snapshot (centroid, high/low energy ratio) —
//!   *timbre*, so loud speech doesn't look like a tap just because it's
//!   loud.
//! - Silero VAD (`voice_activity_detector`) — gates tap classification off
//!   while speech is active; loud/sharp speech can slip past the
//!   classifier's own features otherwise.
//! - Spectral-flux onset novelty (`microdsp`) — a mature MIR (music
//!   information retrieval) technique for detecting percussive onsets,
//!   used as an extra feature rather than another hand-tuned "how sudden
//!   is this" threshold.
//! - All of the above feed a small trained neural net (1 hidden layer,
//!   softmax over none/nail/pad) instead of fixed thresholds — the
//!   original macOS-reference thresholds never fired on this hardware at
//!   all.
//! - Hard amplitude/transient floors on top of the model, and a short
//!   confirm-delay that lets a late-arriving VAD verdict retroactively
//!   cancel a candidate (the classifier reacts to speech onset before the
//!   VAD catches up) — see `TAP_CONFIRM_DELAY`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Set `DJIMIC_DEBUG=1` (same env var `crates/device/src/actor.rs` uses) to
/// log tap detection and device discovery to stderr.
fn debug_enabled() -> bool {
    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var_os("DJIMIC_DEBUG").is_some())
}

// ---------------------------------------------------------------------
// Detection tuning — see test-tools/detect-test/src/main.rs for the much
// longer history of why these specific values, several of which were
// discovered the hard way against real hardware.
// ---------------------------------------------------------------------

/// Prevents literally re-triggering on the same instant (adjacent/
/// overlapping chunks of one continuous spike). A same-tap-echo "has it
/// gone quiet since the last tap" release check was tried and dropped —
/// it ended up blocking legitimate fast double/triple taps more than it
/// helped. Same-tap-echo double-counting is handled by the hard amplitude
/// floors and the multi-candidate confirm system instead (a resonance
/// decay is quieter than the original impact, so it tends to fail
/// `HARD_PEAK_FLOOR`/`HARD_RATIO_FLOOR` on its own).
const DEBOUNCE: Duration = Duration::from_millis(100);
const TAP_WINDOW: Duration = Duration::from_millis(450);
const RMS_WINDOW_LEN: usize = 20;

/// Supplementary hard floors on top of the ML classifier, deliberately set
/// aggressive: quiet/soft incidental sounds (clothing rustle, footsteps, a
/// tap several feet away, a light brush of the mic) can still have a
/// tap-shaped spectrum and slip past the model, but they're both quieter
/// and less sharply transient than someone deliberately, firmly tapping the
/// mic shell. Trading sensitivity for precision here on purpose — a missed
/// light tap is much less annoying than a random false trigger while
/// talking or moving around.
const HARD_PEAK_FLOOR: f32 = 600.0;
const HARD_RATIO_FLOOR: f32 = 10.0;
/// Crest factor (peak / rms *within this one chunk*) turned out to be an
/// unreliable gate in practice — genuine taps measured anywhere from 1.6 to
/// 9+ against real hardware. Left in place but set near its mathematical
/// floor (peak >= rms always, so crest >= 1.0) so it's effectively a no-op;
/// `HARD_PEAK_FLOOR`/`HARD_RATIO_FLOOR` do the actual filtering.
const HARD_CREST_FLOOR: f32 = 1.2;

/// A candidate tap only gets confirmed (counted, logged) after surviving
/// this long unchallenged by the VAD. The classifier reacts to a single
/// chunk's sudden quiet->loud transition immediately, but the VAD needs a
/// bit more context before it's confident something is speech — so at the
/// very start of an utterance, the classifier "wins the race" and fires
/// before the VAD has caught up. Holding the decision open for a short
/// grace period lets a late-arriving VAD verdict retroactively cancel it.
/// Kept short so it doesn't eat into legitimate fast double/triple taps.
const TAP_CONFIRM_DELAY: Duration = Duration::from_millis(150);

/// How long the frontend should consider a finalized tap group "active", so
/// a quick poll-based UI can still catch a momentary flash.
const ACTIVE_WINDOW: Duration = Duration::from_millis(700);

/// This device's audio config sample rate (confirmed via `DJIMIC_DEBUG=1`
/// logging — `[mic_tap] opening ... rate=48000`), used to place the
/// Goertzel analysis bins in `spectral_features`. Hardcoded rather than
/// threaded through every call site since this specific receiver has only
/// ever been observed at this rate.
const SAMPLE_RATE_HZ: f32 = 48000.0;

// ---------------------------------------------------------------------
// Trained classifier — 1 hidden layer, tanh activation, softmax output
// over 3 classes, trained on real recorded data from this receiver (see
// test-tools/detect-test's `collect`/`train` subcommands). Replaces the
// hand-tuned peak/ratio thresholds ported from the macOS reference
// (guessed for different hardware, never fired on this mic).
// ---------------------------------------------------------------------

/// Classes: `0 = not a tap, 1 = fingernail tap, 2 = fingertip-pad tap`. The
/// nail/pad split is purely a training-data trick to give the model a
/// cleaner decision boundary — both count as "a tap" once classified (see
/// `class != 0` below), there's no separate nail/pad gesture exposed.
/// Feature order: `[ln(peak), ln(rms), ln(ratio), zcr, ln(crest),
/// centroid/1000, ln(high/low band ratio), ln(novelty)]`.
const N_FEATURES: usize = 8;
const N_HIDDEN: usize = 8;
const N_CLASSES: usize = 3;

const TAP_W1: [[f32; N_HIDDEN]; N_FEATURES] = [
    [0.135535, -0.076446, 0.004948, -0.180434, -0.095998, -0.385457, -0.022245, 0.054475],
    [0.105033, 0.028647, -0.252446, -0.179555, 0.018818, -0.714688, 0.069344, 0.010913],
    [-0.322777, -0.131798, 0.797818, 0.207440, -0.011429, 0.736934, -0.260370, 0.334038],
    [-0.193070, -0.233307, -0.248121, -0.247495, -0.139189, -0.420904, 0.065982, -0.067548],
    [0.322476, 0.073638, 0.093224, -0.209518, -0.147059, 0.053186, -0.253215, 0.105858],
    [-0.146326, -0.114495, -0.485083, -0.403872, -0.261615, 0.108842, 0.157607, 0.025234],
    [-0.274176, 0.176817, -0.383225, -0.228265, 0.347999, -0.196006, -0.456033, -0.172476],
    [0.026121, -0.094991, 0.253257, 0.274173, -0.321361, 0.050310, 0.256826, -0.188330],
];
const TAP_B1: [f32; N_HIDDEN] =
    [0.567922, 0.154472, -0.102371, 0.207297, -0.108343, -0.483559, 1.251333, -0.455840];
const TAP_W2: [[f32; N_CLASSES]; N_HIDDEN] = [
    [0.794097, -0.320796, -0.402413],
    [0.136090, 0.073055, -0.090941],
    [-0.916578, 0.598011, 0.592258],
    [0.030284, -0.725793, 0.337637],
    [-0.354197, 0.270782, -0.209570],
    [-0.562380, -0.892894, 1.114725],
    [1.339380, -0.600914, -0.769204],
    [-0.429989, 0.299540, 0.460390],
];
const TAP_B2: [f32; N_CLASSES] = [1.225891, -0.418108, -0.807783];
const TAP_MEAN: [f32; N_FEATURES] =
    [5.097567, 4.183465, 0.977682, 0.039682, 0.914110, 0.877099, -2.485912, -6.234169];
const TAP_STD: [f32; N_FEATURES] =
    [1.298908, 1.282411, 0.760981, 0.039219, 0.170054, 0.516419, 1.077319, 1.391290];
/// Raised from the model's natural 0.5 decision boundary — same reasoning
/// as the hard floors above, demand a confidently unambiguous "yes" rather
/// than a bare majority.
const TAP_CONFIDENCE_THRESHOLD: f32 = 0.55;

const TAP_CLASS_NAMES: [&str; N_CLASSES] = ["none", "指甲", "指腹"];

/// Goertzel algorithm: signal magnitude at one target frequency, without
/// needing a full FFT (or an FFT crate) for just a handful of bins.
fn goertzel_magnitude(samples: &[i16], target_hz: f32) -> f32 {
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
    (s_prev2 * s_prev2 + s_prev * s_prev - coeff * s_prev * s_prev2)
        .max(0.0)
        .sqrt()
}

/// Crude 4-band spectral snapshot (returns `(centroid_hz, high/low energy
/// ratio)`) — gives the classifier a sense of *timbre* instead of only
/// amplitude/dynamics: a hard knock on the mic shell is a broadband,
/// high-frequency-heavy transient, while voiced speech concentrates energy
/// in low harmonics even when it's loud.
fn spectral_features(samples: &[i16]) -> (f32, f32) {
    const BANDS: [f32; 4] = [300.0, 1000.0, 3000.0, 6000.0];
    let mags: [f32; 4] = std::array::from_fn(|i| goertzel_magnitude(samples, BANDS[i]));
    let total: f32 = mags.iter().sum::<f32>().max(1e-6);
    let centroid = BANDS.iter().zip(mags.iter()).map(|(f, m)| f * m).sum::<f32>() / total;
    let low = mags[0] + mags[1];
    let high = mags[2] + mags[3];
    let hl_ratio = high / low.max(1e-6);
    (centroid, hl_ratio)
}

/// Spectral-flux based onset novelty (`microdsp::sfnov`) — a mature MIR
/// technique for detecting percussive onsets, the same family of algorithm
/// behind tools like aubio/essentia's onset detectors. Measures
/// frame-to-frame *change* in the spectrum, targeting "how sudden is this
/// attack" more reliably than the crest-factor attempt above.
struct NoveltyState {
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
    fn update(&mut self, samples: &[i16]) -> f32 {
        let float_samples: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
        let mut latest = self.last_novelty;
        self.detector.process(&float_samples, |flux| {
            latest = flux.novelty();
        });
        self.last_novelty = latest;
        latest
    }
}

/// Turns raw per-chunk measurements into the model's input vector.
fn build_feature_vector(
    peak: f32,
    rms: f32,
    ratio: f32,
    zcr: f32,
    centroid: f32,
    hl_ratio: f32,
    novelty: f32,
) -> [f32; N_FEATURES] {
    let crest = (peak / rms.max(1.0)).max(1.0);
    [
        peak.max(1.0).ln(),
        rms.max(1.0).ln(),
        ratio.max(1.0).ln(),
        zcr,
        crest.ln(),
        centroid / 1000.0,
        hl_ratio.max(1e-3).ln(),
        novelty.max(1e-4).ln(),
    ]
}

fn softmax(logits: [f32; N_CLASSES]) -> [f32; N_CLASSES] {
    let max = logits.iter().cloned().fold(f32::MIN, f32::max);
    let exps: [f32; N_CLASSES] = std::array::from_fn(|k| (logits[k] - max).exp());
    let sum: f32 = exps.iter().sum::<f32>().max(1e-9);
    std::array::from_fn(|k| exps[k] / sum)
}

/// Class probabilities `[none, nail, pad]` for one audio chunk, per the
/// trained model.
fn tap_class_probabilities(features: [f32; N_FEATURES]) -> [f32; N_CLASSES] {
    let x: [f32; N_FEATURES] =
        std::array::from_fn(|i| (features[i] - TAP_MEAN[i]) / TAP_STD[i]);
    let hidden: [f32; N_HIDDEN] = std::array::from_fn(|j| {
        let mut sum = TAP_B1[j];
        for i in 0..N_FEATURES {
            sum += x[i] * TAP_W1[i][j];
        }
        sum.tanh()
    });
    let logits: [f32; N_CLASSES] = std::array::from_fn(|k| {
        let mut z = TAP_B2[k];
        for j in 0..N_HIDDEN {
            z += hidden[j] * TAP_W2[j][k];
        }
        z
    });
    softmax(logits)
}

/// The most likely class and its probability, per [`tap_class_probabilities`].
fn tap_predict(features: [f32; N_FEATURES]) -> (usize, f32) {
    let probs = tap_class_probabilities(features);
    let (idx, &p) = probs
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .unwrap();
    (idx, p)
}

// ---------------------------------------------------------------------
// Speech gating: the tap classifier alone still occasionally reads a sharp
// consonant or loud exclamation as a tap, since its features are single
// small-frame amplitude/spectral snapshots, not a real model of speech.
// Running a dedicated voice-activity detector alongside it and suppressing
// tap classification while it reports speech closes that gap.
// ---------------------------------------------------------------------

/// Silero VAD operates on 16kHz mono, 512-sample (32ms) frames; this device
/// captures at 48kHz, so every 3rd raw sample is kept (after a one-pole
/// low-pass to avoid aliasing high-frequency speech content into noise the
/// VAD can't read).
const VAD_DOWNSAMPLE_RATIO: usize = 3;
const VAD_FRAME_LEN: usize = 512;
/// alpha = 1 - exp(-2*pi*fc/fs) for fc=7000Hz, fs=48000Hz.
const VAD_LOWPASS_ALPHA: f32 = 0.6;
/// Leans below Silero's usual ~0.5 default: false-negatives here (real
/// speech scored as non-speech) let the tap classifier's own speech/tap
/// confusion through, which matters more in practice than occasionally
/// gating out a real tap.
const VAD_SCORE_THRESHOLD: f32 = 0.35;
/// How long tap detection stays suppressed after the last speech frame —
/// covers the brief pauses within a sentence so the gate doesn't flicker
/// open between words.
const VAD_HOLDOVER: Duration = Duration::from_millis(450);

struct VadState {
    detector: voice_activity_detector::VoiceActivityDetector,
    lp_state: f32,
    sample_counter: u64,
    frame_buf: Vec<i16>,
    speech_until: Option<Instant>,
    last_score: f32,
}

impl Default for VadState {
    fn default() -> Self {
        let detector = voice_activity_detector::VoiceActivityDetector::builder()
            .sample_rate(16_000)
            .chunk_size(VAD_FRAME_LEN)
            .build()
            .expect("failed to build Silero VAD detector");
        VadState {
            detector,
            lp_state: 0.0,
            sample_counter: 0,
            frame_buf: Vec::new(),
            speech_until: None,
            last_score: 0.0,
        }
    }
}

impl VadState {
    fn update(&mut self, samples: &[i16], now: Instant) {
        for &s in samples {
            self.lp_state += VAD_LOWPASS_ALPHA * (s as f32 - self.lp_state);
            self.sample_counter += 1;
            if self.sample_counter % VAD_DOWNSAMPLE_RATIO as u64 == 0 {
                self.frame_buf.push(self.lp_state.clamp(i16::MIN as f32, i16::MAX as f32) as i16);
            }
        }

        while self.frame_buf.len() >= VAD_FRAME_LEN {
            let frame: Vec<i16> = self.frame_buf[..VAD_FRAME_LEN].to_vec();
            self.last_score = self.detector.predict(frame);
            if self.last_score > VAD_SCORE_THRESHOLD {
                self.speech_until = Some(now + VAD_HOLDOVER);
            }
            self.frame_buf.drain(0..VAD_FRAME_LEN);
        }
    }

    fn is_speech(&self, now: Instant) -> bool {
        self.speech_until.is_some_and(|t| now < t)
    }
}

/// Matches the Windows audio input device name against the receiver. Its
/// exact device string varies (driver/localization dependent), so this
/// checks loosely rather than for one fixed name.
fn is_dji_mic_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("mic rx") || lower.contains("wireless mic") || lower.contains("dji")
}

pub struct MicTapWatcher {
    last_group_count: AtomicU8,
    last_group_millis: AtomicU64,
    device_found: AtomicBool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TapStatus {
    pub count: u8,
    pub active: bool,
    pub device_found: bool,
}

impl MicTapWatcher {
    pub fn status(&self) -> TapStatus {
        let count = self.last_group_count.load(Ordering::Relaxed);
        let last = self.last_group_millis.load(Ordering::Relaxed);
        let active = last != 0 && now_millis().saturating_sub(last) < ACTIVE_WINDOW.as_millis() as u64;
        TapStatus {
            count: if active { count } else { 0 },
            active,
            device_found: self.device_found.load(Ordering::Relaxed),
        }
    }

    /// Only single and double taps are reported — a burst of 3+ still
    /// counts as a double tap rather than becoming a third gesture.
    fn finalize_group(&self, count: u32) {
        let reported = count.min(2);
        self.last_group_count.store(reported as u8, Ordering::Relaxed);
        self.last_group_millis.store(now_millis(), Ordering::Relaxed);
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct PendingTap {
    detected_at: Instant,
    class: usize,
}

#[derive(Default)]
struct DetectionState {
    rms_window: VecDeque<f32>,
    tap_count: u32,
    last_tap: Option<Instant>,
    debounce_until: Option<Instant>,
    vad: VadState,
    novelty: NoveltyState,
    /// Candidate taps awaiting confirmation — see `TAP_CONFIRM_DELAY`. A
    /// `Vec` rather than a single slot: deliberate rapid double/triple taps
    /// routinely land less than one grace period apart, so the second (or
    /// third) real tap needs its own candidate slot instead of being
    /// silently dropped while the first is still pending.
    pending_taps: Vec<PendingTap>,
}

fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

fn peak(samples: &[i16]) -> f32 {
    samples
        .iter()
        .map(|&s| (s as i32).unsigned_abs())
        .max()
        .unwrap_or(0) as f32
}

fn median(values: &VecDeque<f32>) -> f32 {
    if values.is_empty() {
        return 1.0;
    }
    let mut sorted: Vec<f32> = values.iter().copied().collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn zero_crossing_rate(samples: &[i16]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    let crossings = samples.windows(2).filter(|w| (w[0] >= 0) != (w[1] >= 0)).count();
    crossings as f32 / (samples.len() - 1) as f32
}

/// Process one mono chunk of samples, updating detection state and
/// finalizing/reporting a tap group through `watcher` as needed.
/// How long after a detected pairing-button press to suppress tap
/// detection — the button's own mechanical click is picked up by the mic
/// and otherwise reads as a shell tap.
const BUTTON_TAP_SUPPRESS_WINDOW: Duration = Duration::from_millis(300);

fn process_chunk(watcher: &MicTapWatcher, state: &mut DetectionState, samples: &[i16]) {
    let now = Instant::now();
    state.vad.update(samples, now);
    let speech = state.vad.is_speech(now);
    let button_pressed = crate::pairing_button::recently_pressed(BUTTON_TAP_SUPPRESS_WINDOW);
    let suppressed = speech || button_pressed;

    let r = rms(samples);
    let p = peak(samples);
    let zcr = zero_crossing_rate(samples);

    state.rms_window.push_back(r);
    if state.rms_window.len() > RMS_WINDOW_LEN {
        state.rms_window.pop_front();
    }
    let baseline = median(&state.rms_window).max(1.0);
    let ratio = p / baseline;
    let (centroid, hl_ratio) = spectral_features(samples);
    let novelty = state.novelty.update(samples);
    let (class, confidence) =
        tap_predict(build_feature_vector(p, r, ratio, zcr, centroid, hl_ratio, novelty));

    // The moment VAD catches up and flags speech, or the pairing button was
    // just pressed, cancel every candidate still waiting to be confirmed —
    // those were speech onset or the button's own click, not real taps
    // (see `TAP_CONFIRM_DELAY`'s doc comment for why the classifier reacts
    // before the VAD does).
    if suppressed && !state.pending_taps.is_empty() {
        if debug_enabled() {
            for pending in &state.pending_taps {
                eprintln!(
                    "[mic_tap] cancelled pending {} — {}",
                    TAP_CLASS_NAMES[pending.class],
                    if button_pressed { "pairing button pressed".to_string() } else { format!("VAD caught up (score={:.2})", state.vad.last_score) }
                );
            }
        }
        state.pending_taps.clear();
    }

    let debounced = state.debounce_until.is_some_and(|t| now < t);
    // Before `rms_window` has filled up, `median()` falls back to 1.0
    // instead of a real ambient-noise baseline, so `ratio` comes out
    // wildly inflated for the first ~20 chunks after the stream opens
    // (right when the mic connects) — skip detection entirely until a
    // real baseline exists.
    let warmed_up = state.rms_window.len() >= RMS_WINDOW_LEN;
    let crest = p / r.max(1.0);
    let loud_enough =
        p > HARD_PEAK_FLOOR && ratio > HARD_RATIO_FLOOR && crest > HARD_CREST_FLOOR;

    // No `pending_taps.is_empty()` gate here on purpose — a deliberate fast
    // double/triple tap routinely lands a second (or third) real hit before
    // the first one's grace period is up, and each needs its own candidate
    // slot rather than being dropped because "one's already pending".
    if warmed_up
        && class != 0
        && confidence > TAP_CONFIDENCE_THRESHOLD
        && !debounced
        && !suppressed
        && loud_enough
    {
        if debug_enabled() {
            eprintln!(
                "[mic_tap] candidate {} conf={confidence:.2} peak={p:.0} ratio={ratio:.1} novelty={novelty:.3} — waiting {TAP_CONFIRM_DELAY:?} to confirm",
                TAP_CLASS_NAMES[class]
            );
        }
        state.pending_taps.push(PendingTap { detected_at: now, class });
        state.debounce_until = Some(now + DEBOUNCE);
    } else if debug_enabled()
        && warmed_up
        && class != 0
        && confidence > TAP_CONFIDENCE_THRESHOLD
        && !debounced
        && !suppressed
        && !loud_enough
    {
        eprintln!(
            "[mic_tap] too quiet/soft, ignored: class={} conf={confidence:.2} peak={p:.0} (need >{HARD_PEAK_FLOOR}) ratio={ratio:.1} (need >{HARD_RATIO_FLOOR}) crest={crest:.1} (need >{HARD_CREST_FLOOR}) novelty={novelty:.3}",
            TAP_CLASS_NAMES[class]
        );
    }

    // Confirm every pending candidate whose grace period has elapsed,
    // oldest first — usually at most one per call, but a burst of rapid
    // taps can confirm several in the same chunk.
    while state.pending_taps.first().is_some_and(|p| now.duration_since(p.detected_at) >= TAP_CONFIRM_DELAY) {
        let class = state.pending_taps.remove(0).class;
        if debug_enabled() {
            eprintln!("[mic_tap] confirmed {}", TAP_CLASS_NAMES[class]);
        }
        let within_window = state.last_tap.is_some_and(|t| now.duration_since(t) <= TAP_WINDOW);
        if within_window {
            state.tap_count += 1;
        } else {
            if state.tap_count > 0 {
                if debug_enabled() {
                    eprintln!("[mic_tap] group finalized: {} tap(s)", state.tap_count);
                }
                watcher.finalize_group(state.tap_count);
            }
            state.tap_count = 1;
        }
        state.last_tap = Some(now);
    }

    if state.tap_count > 0 && state.pending_taps.is_empty() {
        if state.last_tap.is_some_and(|t| now.duration_since(t) > TAP_WINDOW) {
            if debug_enabled() {
                eprintln!("[mic_tap] group finalized: {} tap(s)", state.tap_count);
            }
            watcher.finalize_group(state.tap_count);
            state.tap_count = 0;
        }
    }
}

#[cfg(windows)]
pub fn spawn() -> Arc<MicTapWatcher> {
    let watcher = Arc::new(MicTapWatcher {
        last_group_count: AtomicU8::new(0),
        last_group_millis: AtomicU64::new(0),
        device_found: AtomicBool::new(false),
    });
    let watcher_thread = watcher.clone();
    std::thread::spawn(move || win32::run(watcher_thread));
    watcher
}

#[cfg(not(windows))]
pub fn spawn() -> Arc<MicTapWatcher> {
    Arc::new(MicTapWatcher {
        last_group_count: AtomicU8::new(0),
        last_group_millis: AtomicU64::new(0),
        device_found: AtomicBool::new(false),
    })
}

#[tauri::command]
pub fn mic_tap_test_status(watcher: tauri::State<'_, Arc<MicTapWatcher>>) -> TapStatus {
    watcher.status()
}

#[cfg(windows)]
mod win32 {
    use super::{debug_enabled, is_dji_mic_name, process_chunk, DetectionState, MicTapWatcher};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    pub fn run(watcher: Arc<MicTapWatcher>) {
        let mut logged_devices = false;
        loop {
            if let Err(e) = try_open_and_run(&watcher) {
                watcher.device_found.store(false, Ordering::Relaxed);
                if debug_enabled() && !logged_devices {
                    eprintln!("[mic_tap] {e}");
                    log_all_input_devices();
                    logged_devices = true;
                }
            } else {
                logged_devices = false;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    fn log_all_input_devices() {
        let Ok(host_devices) = cpal::default_host().input_devices() else {
            eprintln!("[mic_tap] failed to enumerate input devices");
            return;
        };
        eprintln!("[mic_tap] available input devices:");
        for d in host_devices {
            eprintln!("  - {:?}", d.name());
        }
    }

    fn find_device() -> Option<cpal::Device> {
        let host = cpal::default_host();
        host.input_devices()
            .ok()?
            .find(|d| d.name().map(|n| is_dji_mic_name(&n)).unwrap_or(false))
    }

    fn try_open_and_run(watcher: &Arc<MicTapWatcher>) -> Result<(), String> {
        let device = find_device().ok_or("device not found")?;
        watcher.device_found.store(true, Ordering::Relaxed);

        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let sample_format = config.sample_format();
        let channels = config.channels() as usize;
        if debug_enabled() {
            eprintln!(
                "[mic_tap] opening {:?} format={sample_format:?} channels={channels} rate={}",
                device.name(),
                config.sample_rate().0
            );
        }
        let stream_config: cpal::StreamConfig = config.into();

        let state = Arc::new(Mutex::new(DetectionState::default()));
        let err_fn = |_e: cpal::StreamError| {};

        let stream = match sample_format {
            cpal::SampleFormat::I16 => {
                let watcher = watcher.clone();
                let state = state.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        let mono: Vec<i16> = data.chunks(channels.max(1)).map(|f| f[0]).collect();
                        process_chunk(&watcher, &mut state.lock().unwrap(), &mono);
                    },
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::F32 => {
                let watcher = watcher.clone();
                let state = state.clone();
                device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        let mono: Vec<i16> = data
                            .chunks(channels.max(1))
                            .map(|f| (f[0].clamp(-1.0, 1.0) * 32767.0) as i16)
                            .collect();
                        process_chunk(&watcher, &mut state.lock().unwrap(), &mono);
                    },
                    err_fn,
                    None,
                )
            }
            _ => return Err("unsupported sample format".into()),
        }
        .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;

        // cpal's callback runs on its own audio thread; this thread just
        // has to keep `stream` alive and periodically check the device is
        // still present so a disconnect gets picked up and retried.
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if find_device().is_none() {
                return Ok(());
            }
        }
    }
}
