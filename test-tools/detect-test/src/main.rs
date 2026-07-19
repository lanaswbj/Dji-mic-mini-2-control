//! Standalone test tool for the two DJI receiver detection paths that don't
//! go through the vendor control interface: the pairing button (HID Consumer
//! Control report on a separate USB interface) and mic-shell-tap detection
//! (an audio-domain transient detector on the receiver's microphone input).
//!
//! This exists to test both in isolation — fast `cargo run` iteration,
//! plain console output — without rebuilding/relaunching the full Tauri app.
//! It duplicates logic that also lives in `gui/src-tauri/src/pairing_button.rs`
//! and `gui/src-tauri/src/mic_tap.rs`; once both are confirmed working here,
//! that's the copy that matters for the real app.
//!
//! Run with `cargo run` from this directory. Set `DJIMIC_DEBUG=1` for
//! verbose logging (near-miss taps, chosen audio device details, all
//! enumerated input devices while the mic isn't found).

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn debug_enabled() -> bool {
    std::env::var_os("DJIMIC_DEBUG").is_some()
}

// ---------------------------------------------------------------------
// Deferred logging: `process_chunk` runs on cpal's real-time audio
// callback thread, where a synchronous `println!` can itself stall the
// callback long enough to cause WASAPI buffer glitches — corrupting the
// very audio VAD/tap detection are trying to read, and producing exactly
// the kind of erratic, seemingly-random misdetections that are otherwise
// hard to distinguish from "the model is just wrong". Every line
// detection code wants to print goes through this channel instead, so the
// audio thread only ever does a cheap non-blocking send; the actual
// console I/O happens on a dedicated thread.
// ---------------------------------------------------------------------

static LOG_SENDER: OnceLock<Sender<String>> = OnceLock::new();

fn init_logger() {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in rx {
            println!("{line}");
        }
    });
    let _ = LOG_SENDER.set(tx);
}

fn log_line(line: String) {
    if let Some(tx) = LOG_SENDER.get() {
        let _ = tx.send(line);
    }
}

// ---------------------------------------------------------------------
// Mic-tap detection (ported from gui/src-tauri/src/mic_tap.rs)
// ---------------------------------------------------------------------

// Prevents literally re-triggering on the same instant (adjacent/
// overlapping chunks of one continuous spike). Two ratio/peak-based
// "has it gone quiet since the last tap" release checks were tried before
// this and both ended up blocking legitimate fast double/triple taps more
// than they helped — the fixed-baseline version because the 20-chunk
// rolling median stays elevated through a rapid burst, and the absolute
// peak-floor version because ambient/resonance noise between real taps
// doesn't reliably dip low enough either. Same-tap-echo double-counting is
// now handled by the hard amplitude floors and the multi-candidate confirm
// system instead (a resonance decay is quieter than the original impact,
// so it tends to fail `HARD_PEAK_FLOOR`/`HARD_RATIO_FLOOR` on its own).
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
/// talking or moving around. Tune down (independently) if genuine firm taps
/// stop registering.
const HARD_PEAK_FLOOR: f32 = 600.0;
const HARD_RATIO_FLOOR: f32 = 10.0;
/// Crest factor (peak / rms *within this one chunk*, not peak/baseline
/// across chunks like `HARD_RATIO_FLOOR`) — turned out to be an unreliable
/// gate in practice: `DJIMIC_DEBUG=1` logs against genuine taps showed
/// confirmed real taps spanning crest 1.6 all the way to 9+, so any floor
/// worth using as a real filter also rejects a chunk of genuine taps. Left
/// in place but set near its mathematical floor (peak >= rms always, so
/// crest >= 1.0) so it's effectively a no-op — `HARD_PEAK_FLOOR` and
/// `HARD_RATIO_FLOOR` do the actual filtering.
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

/// This device's audio config sample rate (confirmed via `DJIMIC_DEBUG=1`
/// logging — `[mic] opening ... rate=48000`), used to place the Goertzel
/// analysis bins in `spectral_features`. Hardcoded rather than threaded
/// through every call site since this specific receiver has only ever been
/// observed at this rate; if `cargo run` ever logs a different rate, update
/// this to match.
const SAMPLE_RATE_HZ: f32 = 48000.0;

/// Small neural net (1 hidden layer, tanh activation, softmax output over
/// 3 classes) trained on real recorded data from this receiver via
/// `cargo run -- collect` + `cargo run -- train`, replacing the hand-tuned
/// peak/ratio thresholds ported from the macOS reference (guessed for
/// different hardware, never fired on this mic). A plain binary logistic
/// regression over amplitude-only features (peak/rms/ratio) was tried
/// first but confused loud speech for taps — this adds real spectral
/// features (see `spectral_features`) so the model can use *timbre*, not
/// just level, to tell a hard mechanical knock from a vocal transient, and
/// classifies taps into two kinds instead of one lumped "tap" class, since
/// a fingernail knock and a fingertip-pad tap sound noticeably different
/// (nail: sharper/higher-frequency click; pad: softer/duller thud). The 8th
/// feature is spectral-flux onset novelty (see `NoveltyState`) — a
/// mature, well-established MIR (music information retrieval) technique
/// for detecting percussive onsets (used in tools like aubio/essentia),
/// borrowed here instead of hand-rolling yet another ad hoc "how sudden is
/// this" heuristic like the crest-factor attempt that didn't pan out.
/// Classes: `0 = not a tap, 1 = fingernail tap, 2 = fingertip-pad tap`.
/// Feature order: `[ln(peak), ln(rms), ln(ratio), zcr, ln(crest),
/// centroid/1000, ln(high/low band ratio), ln(novelty)]`. Retrain and paste
/// in new values here (the `train` command prints ready-to-paste `const`
/// lines) whenever more data is collected.
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
/// (music information retrieval) technique for detecting percussive
/// onsets, the same family of algorithm behind tools like aubio/essentia's
/// onset detectors. Measures frame-to-frame *change* in the spectrum, not
/// just its static shape, so it targets exactly "how sudden is this attack"
/// — the thing `HARD_CREST_FLOOR` tried and failed to capture reliably.
/// Wrapped in its own state since the underlying detector needs continuity
/// across chunks (it compares each frame's spectrum to the previous one).
struct NoveltyState {
    detector: microdsp::sfnov::SpectralFluxNoveltyDetector<microdsp::sfnov::HardKneeCompression>,
    last_novelty: f32,
}

/// Onset detector's internal analysis window. Smaller = lower latency but
/// less frequency resolution; 512 samples at 48kHz is ~10.7ms, kept small
/// on purpose since low latency was an explicit goal.
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

/// Turns raw per-chunk measurements into the model's input vector. Shared
/// by inference (fed live values) and training (fed values parsed back out
/// of the collected CSV), so the two can never drift apart.
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

/// Runs the hidden layer only — shared by inference and training so the
/// forward pass can't drift between the two.
fn tap_hidden_layer(
    x: &[f32; N_FEATURES],
    w1: &[[f32; N_HIDDEN]; N_FEATURES],
    b1: &[f32; N_HIDDEN],
) -> [f32; N_HIDDEN] {
    std::array::from_fn(|j| {
        let mut sum = b1[j];
        for i in 0..N_FEATURES {
            sum += x[i] * w1[i][j];
        }
        sum.tanh()
    })
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
    let hidden = tap_hidden_layer(&x, &TAP_W1, &TAP_B1);
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

/// Matches the Windows audio input device name against the receiver. Its
/// exact device string varies (driver/localization dependent), so this
/// checks loosely rather than for one fixed name.
fn is_dji_mic_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("mic rx") || lower.contains("wireless mic") || lower.contains("dji")
}

// ---------------------------------------------------------------------
// Speech gating: the tap classifier alone still occasionally reads a sharp
// consonant or loud exclamation as a tap, since its features are single
// small-frame amplitude/spectral snapshots, not a real model of speech.
// Running a dedicated (and much better-trained) voice-activity detector
// alongside it and suppressing tap classification while it reports speech
// closes that gap without needing to chase it via more tap-model tuning.
// ---------------------------------------------------------------------

/// Silero VAD (the neural-net model, not the classical WebRTC-style
/// `earshot` this replaced) operates on 16kHz mono, 512-sample (32ms)
/// frames; our device captures at 48kHz, so every 3rd raw sample is kept.
/// The `voice_activity_detector` crate's `ort` backend downloads its own
/// ONNX Runtime binary as part of `cargo build` (no manual DLL placement
/// needed, unlike `silero-vad-rust`, which was tried first and dropped —
/// it has a broken internal `ndarray` version conflict as published).
const VAD_DOWNSAMPLE_RATIO: usize = 3;
const VAD_FRAME_LEN: usize = 512;
/// One-pole low-pass cutoff (~7kHz) applied before dropping samples down to
/// 16kHz, so the 48kHz->16kHz decimation doesn't alias high-frequency
/// speech content (sibilants especially) into noise the VAD can't read.
/// alpha = 1 - exp(-2*pi*fc/fs) for fc=7000Hz, fs=48000Hz.
const VAD_LOWPASS_ALPHA: f32 = 0.6;
/// Silero's own scores are much better-calibrated than earshot's, but this
/// still leans below its usual ~0.5 default: false-negatives here (real
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
    /// Most recent frame's score, purely for `DJIMIC_DEBUG=1` diagnostics.
    last_score: f32,
    /// When `DJIMIC_DEBUG=1`, every 16kHz sample actually fed to the VAD
    /// gets written here too — lets a human listen to exactly what the
    /// model sees, to check whether the 48kHz->16kHz downsampling itself
    /// is producing clean audio or garbage before suspecting the model.
    debug_wav: Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>,
}

impl Default for VadState {
    fn default() -> Self {
        let detector = voice_activity_detector::VoiceActivityDetector::builder()
            .sample_rate(16_000)
            .chunk_size(VAD_FRAME_LEN)
            .build()
            .expect("failed to build Silero VAD detector");
        let debug_wav = debug_enabled().then(|| {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vad_debug.wav");
            let writer = hound::WavWriter::create(&path, spec)
                .expect("failed to create vad_debug.wav");
            println!("[vad] dumping what the VAD hears to {}", path.display());
            writer
        });
        VadState {
            detector,
            lp_state: 0.0,
            sample_counter: 0,
            frame_buf: Vec::new(),
            speech_until: None,
            last_score: 0.0,
            debug_wav,
        }
    }
}

impl VadState {
    fn update(&mut self, samples: &[i16], now: Instant) {
        for &s in samples {
            self.lp_state += VAD_LOWPASS_ALPHA * (s as f32 - self.lp_state);
            self.sample_counter += 1;
            if self.sample_counter % VAD_DOWNSAMPLE_RATIO as u64 == 0 {
                let down = self.lp_state.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                self.frame_buf.push(down);
                if let Some(w) = &mut self.debug_wav {
                    let _ = w.write_sample(down);
                }
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

        // Ctrl+C kills the process without running destructors, so anything
        // still sitting in the WavWriter's internal BufWriter never reaches
        // disk unless it's flushed along the way — without this the debug
        // WAV comes out completely empty regardless of what audio was
        // actually captured.
        if let Some(w) = &mut self.debug_wav {
            let _ = w.flush();
        }
    }

    fn is_speech(&self, now: Instant) -> bool {
        self.speech_until.is_some_and(|t| now < t)
    }
}

#[derive(Default)]
struct DetectionState {
    rms_window: VecDeque<f32>,
    tap_count: u32,
    /// Class index (per `TAP_CLASS_NAMES`) of each tap in the current burst,
    /// so the group-finalize line can show what kind each one was.
    tap_classes: Vec<usize>,
    last_tap: Option<Instant>,
    debounce_until: Option<Instant>,
    vad: VadState,
    novelty: NoveltyState,
    /// Candidate taps awaiting confirmation — see `TAP_CONFIRM_DELAY`. A
    /// `Vec` rather than a single slot: deliberate rapid double/triple taps
    /// routinely land less than one grace period apart, so the second (or
    /// third) real tap needs its own candidate slot instead of being
    /// silently dropped while the first is still pending. Oldest-first;
    /// confirmed/cancelled together but timed independently.
    pending_taps: Vec<PendingTap>,
}

struct PendingTap {
    detected_at: Instant,
    class: usize,
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

fn process_chunk(state: &mut DetectionState, samples: &[i16]) {
    let now = Instant::now();
    state.vad.update(samples, now);
    let speech = state.vad.is_speech(now);

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

    // The moment VAD catches up and flags speech, cancel every candidate
    // still waiting to be confirmed — those were speech onset, not real
    // taps (see `TAP_CONFIRM_DELAY`'s doc comment for why the classifier
    // reacts before the VAD does).
    if speech && !state.pending_taps.is_empty() {
        if debug_enabled() {
            for pending in &state.pending_taps {
                log_line(format!(
                    "[tap] cancelled pending {} — VAD caught up (score={:.2})",
                    TAP_CLASS_NAMES[pending.class], state.vad.last_score
                ));
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
        && !speech
        && loud_enough
    {
        if debug_enabled() {
            log_line(format!(
                "[tap] candidate {} conf={confidence:.2} peak={p:.0} ratio={ratio:.1} novelty={novelty:.3} — waiting {TAP_CONFIRM_DELAY:?} to confirm",
                TAP_CLASS_NAMES[class]
            ));
        }
        state.pending_taps.push(PendingTap { detected_at: now, class });
        state.debounce_until = Some(now + DEBOUNCE);
    } else if debug_enabled()
        && warmed_up
        && class != 0
        && confidence > TAP_CONFIDENCE_THRESHOLD
        && !debounced
        && !speech
        && !loud_enough
    {
        log_line(format!(
            "[tap] too quiet/soft, ignored: class={} conf={confidence:.2} peak={p:.0} (need >{HARD_PEAK_FLOOR}) ratio={ratio:.1} (need >{HARD_RATIO_FLOOR}) crest={crest:.1} (need >{HARD_CREST_FLOOR}) novelty={novelty:.3}",
            TAP_CLASS_NAMES[class]
        ));
    }

    // Confirm every pending candidate whose grace period has elapsed,
    // oldest first — usually at most one per call, but a burst of rapid
    // taps can confirm several in the same chunk.
    while state.pending_taps.first().is_some_and(|p| now.duration_since(p.detected_at) >= TAP_CONFIRM_DELAY) {
        let class = state.pending_taps.remove(0).class;
        if debug_enabled() {
            log_line(format!("[tap] confirmed {}", TAP_CLASS_NAMES[class]));
        }
        let within_window = state.last_tap.is_some_and(|t| now.duration_since(t) <= TAP_WINDOW);
        if within_window {
            state.tap_count += 1;
        } else {
            if state.tap_count > 0 {
                log_line(format!(
                    ">>> [TAP] {} tap(s) detected ({})",
                    state.tap_count,
                    state.tap_classes.iter().map(|&c| TAP_CLASS_NAMES[c]).collect::<Vec<_>>().join(", ")
                ));
                state.tap_classes.clear();
            }
            state.tap_count = 1;
        }
        state.tap_classes.push(class);
        state.last_tap = Some(now);
    }

    if state.tap_count > 0 && state.pending_taps.is_empty() {
        if state.last_tap.is_some_and(|t| now.duration_since(t) > TAP_WINDOW) {
            log_line(format!(
                ">>> [TAP] {} tap(s) detected ({})",
                state.tap_count,
                state.tap_classes.iter().map(|&c| TAP_CLASS_NAMES[c]).collect::<Vec<_>>().join(", ")
            ));
            state.tap_count = 0;
            state.tap_classes.clear();
        }
    }
}

fn log_all_input_devices() {
    let Ok(host_devices) = cpal::default_host().input_devices() else {
        println!("[mic] failed to enumerate input devices");
        return;
    };
    println!("[mic] available input devices:");
    for d in host_devices {
        println!("  - {:?}", d.name());
    }
}

fn find_device() -> Option<cpal::Device> {
    let host = cpal::default_host();
    host.input_devices()
        .ok()?
        .find(|d| d.name().map(|n| is_dji_mic_name(&n)).unwrap_or(false))
}

fn try_open_and_run() -> Result<(), String> {
    let device = find_device().ok_or("device not found")?;

    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let sample_format = config.sample_format();
    let channels = config.channels() as usize;
    println!(
        "[mic] opening {:?} format={sample_format:?} channels={channels} rate={}",
        device.name(),
        config.sample_rate().0
    );
    let stream_config: cpal::StreamConfig = config.into();

    let state = Arc::new(Mutex::new(DetectionState::default()));
    let err_fn = |e: cpal::StreamError| println!("[mic] stream error: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::I16 => {
            let state = state.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    let mono: Vec<i16> = data.chunks(channels.max(1)).map(|f| f[0]).collect();
                    process_chunk(&mut state.lock().unwrap(), &mono);
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::F32 => {
            let state = state.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    let mono: Vec<i16> = data
                        .chunks(channels.max(1))
                        .map(|f| (f[0].clamp(-1.0, 1.0) * 32767.0) as i16)
                        .collect();
                    process_chunk(&mut state.lock().unwrap(), &mono);
                },
                err_fn,
                None,
            )
        }
        _ => return Err("unsupported sample format".into()),
    }
    .map_err(|e| e.to_string())?;

    stream.play().map_err(|e| e.to_string())?;

    loop {
        std::thread::sleep(Duration::from_millis(500));
        if find_device().is_none() {
            println!("[mic] device disappeared, will retry");
            return Ok(());
        }
    }
}

fn run_mic_tap() {
    let mut logged_devices = false;
    loop {
        if let Err(e) = try_open_and_run() {
            if !logged_devices {
                println!("[mic] {e}");
                log_all_input_devices();
                logged_devices = true;
            }
        } else {
            logged_devices = false;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------------
// Data collection (`cargo run -- collect`): records labeled audio-feature
// rows to a CSV so a classifier can be trained on real taps from this
// specific hardware/system, instead of hand-tuned thresholds guessed from
// a different reference implementation on different hardware.
// ---------------------------------------------------------------------

/// Written by the main thread during the passive (non-tap) recording
/// phases — quiet / speech / loud speech / other sounds — read by the audio
/// callback so every feature row gets labeled with whatever phase was
/// active when it was captured. Always 0 (not a tap) during those phases;
/// tap phases use the capture buffer below instead, since a tap needs
/// smarter labeling than "everything in this window is a tap".
static CURRENT_LABEL: AtomicU8 = AtomicU8::new(0);

/// When set, `write_feature_row` buffers chunks instead of writing them
/// immediately — see `start_capture`/`finish_capture`.
static CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// The tap class (1=指甲, 2=指腹) the current capture window is recording.
static CAPTURE_LABEL: AtomicU8 = AtomicU8::new(0);
/// Buffered `(peak, rms, ratio, zcr, centroid, hl_ratio, novelty)` rows for
/// the current capture window, relabeled and flushed by `finish_capture`.
static CAPTURE_BUFFER: Mutex<Vec<(f32, f32, f32, f32, f32, f32, f32)>> = Mutex::new(Vec::new());

fn zero_crossing_rate(samples: &[i16]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    let crossings = samples.windows(2).filter(|w| (w[0] >= 0) != (w[1] >= 0)).count();
    crossings as f32 / (samples.len() - 1) as f32
}

fn data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data")
}

/// The CSV header this build writes/expects. Bumping the feature set (like
/// adding the spectral columns) changes this — `run_collect` checks it
/// against any existing file and starts a fresh one rather than silently
/// mixing schemas if it doesn't match.
const CSV_HEADER: &str = "label,peak,rms,ratio,zcr,centroid,hl_ratio,novelty";

fn write_feature_row(
    writer: &Arc<Mutex<std::io::BufWriter<std::fs::File>>>,
    rms_window: &Arc<Mutex<VecDeque<f32>>>,
    novelty_state: &Arc<Mutex<NoveltyState>>,
    samples: &[i16],
) {
    let r = rms(samples);
    let p = peak(samples);
    let zcr = zero_crossing_rate(samples);
    let (centroid, hl_ratio) = spectral_features(samples);
    let novelty = novelty_state.lock().unwrap().update(samples);

    let baseline = {
        let mut window = rms_window.lock().unwrap();
        window.push_back(r);
        if window.len() > RMS_WINDOW_LEN {
            window.pop_front();
        }
        median(&window).max(1.0)
    };
    let ratio = p / baseline;

    if CAPTURE_ACTIVE.load(Ordering::Relaxed) {
        CAPTURE_BUFFER.lock().unwrap().push((p, r, ratio, zcr, centroid, hl_ratio, novelty));
        return;
    }

    let label = CURRENT_LABEL.load(Ordering::Relaxed);
    let mut w = writer.lock().unwrap();
    let _ = writeln!(w, "{label},{p:.1},{r:.1},{ratio:.2},{zcr:.4},{centroid:.1},{hl_ratio:.3},{novelty:.4}");
    let _ = w.flush();
}

/// Starts buffering incoming chunks instead of writing them immediately, so
/// the tap's actual acoustic peak (wherever it lands inside the capture
/// window — reaction time after pressing Enter varies) can be found before
/// any labels get assigned.
fn start_capture(class: u8) {
    CAPTURE_BUFFER.lock().unwrap().clear();
    CAPTURE_LABEL.store(class, Ordering::Relaxed);
    CAPTURE_ACTIVE.store(true, Ordering::Relaxed);
}

/// Stops buffering and flushes the window to the CSV. Only the loudest
/// chunk (plus its immediate neighbors, to cover the short decay) gets the
/// target tap label; everything else in the window is background. Labeling
/// the *entire* window as the tap class — what this replaced — mislabels
/// most of it, since the actual click only lasts a couple of chunks and its
/// position inside the window isn't fixed.
fn finish_capture(writer: &Arc<Mutex<std::io::BufWriter<std::fs::File>>>) {
    CAPTURE_ACTIVE.store(false, Ordering::Relaxed);
    let class = CAPTURE_LABEL.load(Ordering::Relaxed);
    let buffer = std::mem::take(&mut *CAPTURE_BUFFER.lock().unwrap());
    let Some((peak_idx, _)) =
        buffer.iter().enumerate().max_by(|a, b| a.1 .0.partial_cmp(&b.1 .0).unwrap())
    else {
        return;
    };

    let mut w = writer.lock().unwrap();
    for (i, (p, r, ratio, zcr, centroid, hl_ratio, novelty)) in buffer.iter().enumerate() {
        let label = if i.abs_diff(peak_idx) <= 1 { class } else { 0 };
        let _ = writeln!(
            w,
            "{label},{p:.1},{r:.1},{ratio:.2},{zcr:.4},{centroid:.1},{hl_ratio:.3},{novelty:.4}"
        );
    }
    let _ = w.flush();
}

fn run_collect() {
    let Some(device) = find_device() else {
        println!("[collect] 没找到麦克风设备，请先确认普通模式下（cargo run，不带参数）能看到 [mic] opening ...");
        return;
    };

    let config = device.default_input_config().expect("default_input_config failed");
    let sample_format = config.sample_format();
    let channels = config.channels() as usize;
    println!(
        "[collect] opening {:?} format={sample_format:?} channels={channels} rate={}",
        device.name(),
        config.sample_rate().0
    );
    let stream_config: cpal::StreamConfig = config.into();

    let dir = data_dir();
    std::fs::create_dir_all(&dir).expect("create data dir");
    let csv_path = dir.join("samples.csv");

    // If a samples.csv from an older feature set is sitting there, its rows
    // have the wrong column count and `run_train` would just silently skip
    // them — rename it aside instead of quietly discarding old data.
    if let Ok(existing) = std::fs::read_to_string(&csv_path) {
        if existing.lines().next().is_some_and(|h| h != CSV_HEADER) {
            let backup = dir.join(format!(
                "samples.csv.bak-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            ));
            println!(
                "[collect] 发现旧格式的采集数据，已备份到 {}（这次改了特征列，需要重新采集）",
                backup.display()
            );
            std::fs::rename(&csv_path, &backup).ok();
        }
    }

    let is_new = !csv_path.exists();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&csv_path)
        .expect("open csv for append");
    let writer = Arc::new(Mutex::new(std::io::BufWriter::new(file)));
    if is_new {
        writeln!(writer.lock().unwrap(), "{CSV_HEADER}").unwrap();
    }

    let rms_window: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    let novelty_state: Arc<Mutex<NoveltyState>> = Arc::new(Mutex::new(NoveltyState::default()));
    let err_fn = |e: cpal::StreamError| println!("[collect] stream error: {e}");

    let stream = {
        let writer = writer.clone();
        let rms_window = rms_window.clone();
        let novelty_state = novelty_state.clone();
        match sample_format {
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    let mono: Vec<i16> = data.chunks(channels.max(1)).map(|f| f[0]).collect();
                    write_feature_row(&writer, &rms_window, &novelty_state, &mono);
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    let mono: Vec<i16> = data
                        .chunks(channels.max(1))
                        .map(|f| (f[0].clamp(-1.0, 1.0) * 32767.0) as i16)
                        .collect();
                    write_feature_row(&writer, &rms_window, &novelty_state, &mono);
                },
                err_fn,
                None,
            ),
            _ => {
                println!("[collect] unsupported sample format");
                return;
            }
        }
    }
    .expect("build_input_stream failed");
    stream.play().expect("stream.play failed");

    let stdin = std::io::stdin();
    let wait_for_enter = |prompt: &str| {
        print!("{prompt}");
        std::io::stdout().flush().ok();
        let mut l = String::new();
        stdin.read_line(&mut l).ok();
    };
    // Counts down out loud so there's time to get into position (walk over
    // to wherever, pick up an object, etc.) before a phase actually starts
    // recording — the phase's own label only applies once this ends.
    let prepare = |seconds: u64| {
        println!("准备中，{seconds} 秒后开始...");
        for remaining in (1..=seconds).rev() {
            print!("{remaining}...");
            std::io::stdout().flush().ok();
            std::thread::sleep(Duration::from_secs(1));
        }
        println!("开始！");
    };

    println!("\n=== 第 1 阶段：安静 ===");
    println!("接下来 8 秒请保持安静，不要说话、不要碰麦克风。");
    prepare(10);
    std::thread::sleep(Duration::from_secs(8));

    println!("\n=== 第 2 阶段：正常说话 ===");
    println!("接下来 10 秒请正常语速说话、聊天都行（不要敲麦克风）。");
    prepare(10);
    std::thread::sleep(Duration::from_secs(10));

    println!("\n=== 第 3 阶段：大声/夸张说话（重点负样本）===");
    println!("接下来 10 秒请大声说、喊、感叹词都来一点，比如“啊！”“哈！”“不行！”“太好了！”，");
    println!("这些容易被误判成敲击，多录一些能让模型学会分辨。");
    prepare(10);
    std::thread::sleep(Duration::from_secs(10));

    println!("\n=== 第 4 阶段：其他碰撞/环境声（不要碰麦克风）===");
    println!("接下来 60 秒，可以到处走动，做各种动作（都不要碰麦克风本体），比如：");
    println!("  - 敲一敲电脑机箱/键盘面板");
    println!("  - 整理/摩擦衣服，动一动身体");
    println!("  - 敲桌子（近一点、远一点各来几下）、敲门、敲墙");
    println!("  - 挪动键盘鼠标、翻书、咳嗽、拍手（离麦克风远一点）、走动的脚步声");
    prepare(10);
    std::thread::sleep(Duration::from_secs(60));

    println!("\n=== 第 5 阶段：指甲敲击 ===");
    println!("用指甲敲麦克风外壳（不是指腹）。每次按回车后立刻敲一下，共 30 次。");
    prepare(10);
    for i in 1..=30 {
        wait_for_enter(&format!("指甲 第 {i}/30 次，按回车后敲击: "));
        start_capture(1);
        std::thread::sleep(Duration::from_millis(600));
        finish_capture(&writer);
        std::thread::sleep(Duration::from_millis(300));
    }

    println!("\n=== 第 6 阶段：指腹敲击 ===");
    println!("用手指指腹（软的那一面）敲麦克风外壳。每次按回车后立刻敲一下，共 30 次。");
    prepare(10);
    for i in 1..=30 {
        wait_for_enter(&format!("指腹 第 {i}/30 次，按回车后敲击: "));
        start_capture(2);
        std::thread::sleep(Duration::from_millis(600));
        finish_capture(&writer);
        std::thread::sleep(Duration::from_millis(300));
    }

    drop(stream);
    println!("\n[collect] 采集完成，数据已追加写入 {}", csv_path.display());
    println!("[collect] 可以多跑几次 `cargo run -- collect` 累积更多数据（会追加，不会覆盖）。");
}

// ---------------------------------------------------------------------
// Offline training (`cargo run -- train`): hand-rolled batch-gradient-
// descent over the CSV `collect` produces, for the 1-hidden-layer/
// softmax-output net defined above. Deliberately not pulling in an ML
// crate (linfa etc.) — 7 features, 3 classes, and a few thousand rows
// train fine with plain Rust, and it keeps this tool dependency-light
// like the rest of the project.
// ---------------------------------------------------------------------

/// Tiny deterministic PRNG (xorshift64) for weight init — avoids pulling in
/// the `rand` crate for what's just a handful of small random numbers.
struct Rng(u64);
impl Rng {
    fn next_unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 as f64 / u64::MAX as f64) as f32 * 2.0 - 1.0
    }
}

fn fmt_array(values: &[f32]) -> String {
    values.iter().map(|v| format!("{v:.6}")).collect::<Vec<_>>().join(", ")
}

fn run_train() {
    let csv_path = data_dir().join("samples.csv");
    let content = match std::fs::read_to_string(&csv_path) {
        Ok(c) => c,
        Err(e) => {
            println!("[train] 读取 {} 失败: {e}", csv_path.display());
            println!("[train] 先运行 `cargo run -- collect` 采集数据。");
            return;
        }
    };

    let mut rows: Vec<([f32; N_FEATURES], usize)> = Vec::new();
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 8 {
            continue;
        }
        let (Ok(label), Ok(p), Ok(r), Ok(ratio), Ok(zcr), Ok(centroid), Ok(hl_ratio), Ok(novelty)) = (
            parts[0].parse::<f32>(),
            parts[1].parse::<f32>(),
            parts[2].parse::<f32>(),
            parts[3].parse::<f32>(),
            parts[4].parse::<f32>(),
            parts[5].parse::<f32>(),
            parts[6].parse::<f32>(),
            parts[7].parse::<f32>(),
        ) else {
            continue;
        };
        let class = (label.round() as usize).min(N_CLASSES - 1);
        rows.push((build_feature_vector(p, r, ratio, zcr, centroid, hl_ratio, novelty), class));
    }

    if rows.is_empty() {
        println!("[train] {} 里没有可用数据", csv_path.display());
        return;
    }
    let mut counts = [0usize; N_CLASSES];
    for (_, c) in &rows {
        counts[*c] += 1;
    }
    println!(
        "[train] loaded {} rows (none={}, 指甲={}, 指腹={})",
        rows.len(),
        counts[0],
        counts[1],
        counts[2]
    );

    let n = rows.len() as f32;
    let mut mean = [0.0f32; N_FEATURES];
    for (f, _) in &rows {
        for i in 0..N_FEATURES {
            mean[i] += f[i];
        }
    }
    for m in &mut mean {
        *m /= n;
    }
    let mut std_dev = [0.0f32; N_FEATURES];
    for (f, _) in &rows {
        for i in 0..N_FEATURES {
            std_dev[i] += (f[i] - mean[i]).powi(2);
        }
    }
    for s in &mut std_dev {
        *s = (*s / n).sqrt().max(1e-6);
    }

    let normalized: Vec<([f32; N_FEATURES], usize)> = rows
        .iter()
        .map(|(f, c)| (std::array::from_fn(|i| (f[i] - mean[i]) / std_dev[i]), *c))
        .collect();

    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut w1 = [[0.0f32; N_HIDDEN]; N_FEATURES];
    for row in &mut w1 {
        for w in row.iter_mut() {
            *w = rng.next_unit() * 0.3;
        }
    }
    let mut b1 = [0.0f32; N_HIDDEN];
    let mut w2 = [[0.0f32; N_CLASSES]; N_HIDDEN];
    for row in &mut w2 {
        for w in row.iter_mut() {
            *w = rng.next_unit() * 0.3;
        }
    }
    let mut b2 = [0.0f32; N_CLASSES];

    const LR: f32 = 0.1;
    const EPOCHS: u32 = 4000;
    const EPS: f32 = 1e-7;

    // "none" outnumbers each tap class roughly 17:1 in a typical recording
    // session (a few taps vs. many seconds of quiet/speech/noise), so plain
    // unweighted cross-entropy converges to mostly predicting "none" and
    // barely ever fires on real taps. Full inverse-frequency weighting
    // (total / (classes * count)) overcorrects badly at this ratio — tried
    // it, and "none" recall dropped to ~73% (27% false-positive rate on
    // ordinary speech/silence). Using its square root instead compresses
    // the weight range (~17x becomes ~4x) for a milder correction: enough
    // for the rare tap classes to matter during training without drowning
    // out the plentiful, easy "none" examples.
    let class_weight: [f32; N_CLASSES] = std::array::from_fn(|k| {
        if counts[k] > 0 {
            (n / (N_CLASSES as f32 * counts[k] as f32)).sqrt()
        } else {
            0.0
        }
    });
    println!("[train] class weights (imbalance correction): {class_weight:?}");

    for epoch in 0..EPOCHS {
        let mut grad_w1 = [[0.0f32; N_HIDDEN]; N_FEATURES];
        let mut grad_b1 = [0.0f32; N_HIDDEN];
        let mut grad_w2 = [[0.0f32; N_CLASSES]; N_HIDDEN];
        let mut grad_b2 = [0.0f32; N_CLASSES];
        let mut loss = 0.0f32;

        for (x, class) in &normalized {
            let hidden = tap_hidden_layer(x, &w1, &b1);
            let logits: [f32; N_CLASSES] = std::array::from_fn(|k| {
                let mut z = b2[k];
                for j in 0..N_HIDDEN {
                    z += hidden[j] * w2[j][k];
                }
                z
            });
            let probs = softmax(logits);
            let weight = class_weight[*class];
            loss -= weight * (probs[*class] + EPS).ln();

            // Softmax + cross-entropy gradient wrt logits is simply
            // (predicted - one_hot(true_class)) per class, scaled by this
            // sample's class weight to match the weighted loss above.
            let dz: [f32; N_CLASSES] = std::array::from_fn(|k| {
                weight * (probs[k] - if k == *class { 1.0 } else { 0.0 })
            });
            for j in 0..N_HIDDEN {
                for k in 0..N_CLASSES {
                    grad_w2[j][k] += dz[k] * hidden[j];
                }
            }
            for k in 0..N_CLASSES {
                grad_b2[k] += dz[k];
            }
            for j in 0..N_HIDDEN {
                let dh: f32 = (0..N_CLASSES).map(|k| dz[k] * w2[j][k]).sum();
                let dh_raw = dh * (1.0 - hidden[j] * hidden[j]);
                grad_b1[j] += dh_raw;
                for i in 0..N_FEATURES {
                    grad_w1[i][j] += dh_raw * x[i];
                }
            }
        }

        for i in 0..N_FEATURES {
            for j in 0..N_HIDDEN {
                w1[i][j] -= LR * grad_w1[i][j] / n;
            }
        }
        for j in 0..N_HIDDEN {
            b1[j] -= LR * grad_b1[j] / n;
            for k in 0..N_CLASSES {
                w2[j][k] -= LR * grad_w2[j][k] / n;
            }
        }
        for k in 0..N_CLASSES {
            b2[k] -= LR * grad_b2[k] / n;
        }

        if epoch % 500 == 0 || epoch == EPOCHS - 1 {
            println!("[train] epoch {epoch} loss={:.4}", loss / n);
        }
    }

    // Sanity-check accuracy on the training set itself (not a held-out
    // split — good enough as a smoke test at this scale, not a rigorous
    // evaluation). Reports a confusion matrix so it's obvious which classes
    // get mixed up, not just an overall percentage.
    let mut confusion = [[0usize; N_CLASSES]; N_CLASSES]; // [actual][predicted]
    for (x, class) in &normalized {
        let hidden = tap_hidden_layer(x, &w1, &b1);
        let logits: [f32; N_CLASSES] = std::array::from_fn(|k| {
            let mut z = b2[k];
            for j in 0..N_HIDDEN {
                z += hidden[j] * w2[j][k];
            }
            z
        });
        let probs = softmax(logits);
        let predicted = probs.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).unwrap().0;
        confusion[*class][predicted] += 1;
    }
    let correct: usize = (0..N_CLASSES).map(|k| confusion[k][k]).sum();
    println!(
        "\n[train] 训练集准确率 {:.1}%（共 {} 条）",
        100.0 * correct as f32 / normalized.len() as f32,
        normalized.len()
    );
    println!("[train] 混淆矩阵（行=真实, 列=预测, 顺序 none/指甲/指腹）：");
    for row in &confusion {
        println!("  {row:?}");
    }

    println!("\n[train] 训练完成，把下面这些常量整体贴进 main.rs 替换掉旧的 TAP_* 常量：\n");
    print!("const TAP_W1: [[f32; {N_HIDDEN}]; {N_FEATURES}] = [");
    for row in &w1 {
        print!("[{}], ", fmt_array(row));
    }
    println!("];");
    println!("const TAP_B1: [f32; {N_HIDDEN}] = [{}];", fmt_array(&b1));
    print!("const TAP_W2: [[f32; {N_CLASSES}]; {N_HIDDEN}] = [");
    for row in &w2 {
        print!("[{}], ", fmt_array(row));
    }
    println!("];");
    println!("const TAP_B2: [f32; {N_CLASSES}] = [{}];", fmt_array(&b2));
    println!("const TAP_MEAN: [f32; {N_FEATURES}] = [{}];", fmt_array(&mean));
    println!("const TAP_STD: [f32; {N_FEATURES}] = [{}];", fmt_array(&std_dev));
}

// ---------------------------------------------------------------------
// Pairing-button detection (ported from gui/src-tauri/src/pairing_button.rs)
// ---------------------------------------------------------------------

#[cfg(windows)]
mod pairing {
    use windows::core::w;
    use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::{
        GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT, RAWINPUT,
        RAWINPUTDEVICE, RAWINPUTHEADER, RID_DEVICE_INFO, RIDEV_INPUTSINK, RIDI_DEVICEINFO,
        RID_INPUT, RIM_TYPEHID,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
        RegisterClassExW, TranslateMessage, CW_USEDEFAULT, HWND_MESSAGE, MSG, WM_DESTROY,
        WM_INPUT, WNDCLASSEXW, WNDCLASS_STYLES,
    };

    const VID: u32 = 0x2ca3;
    const PID: u32 = 0x4011;
    const USAGE_PAGE_CONSUMER: u16 = 0x0c;
    const USAGE_CONSUMER_CONTROL: u16 = 0x01;

    /// Blocks forever pumping the message loop. Prints a line to stdout on
    /// every detected pairing-button press.
    pub fn run() {
        unsafe {
            let Ok(hinstance) = GetModuleHandleW(None) else {
                println!("[pairing] GetModuleHandleW failed");
                return;
            };
            let class_name = w!("DjiMicDetectTestWnd");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: WNDCLASS_STYLES::default(),
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
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
                println!("[pairing] CreateWindowExW failed");
                return;
            };

            // RIDEV_NOLEGACY is only valid for the Generic Desktop usage page
            // (mouse/keyboard, page 0x01) — using it here on the Consumer
            // Control page (0x0C) makes RegisterRawInputDevices fail outright
            // with E_INVALIDARG, so this omits it. That means Windows' own
            // default handling (the volume change) isn't suppressed by this
            // registration; that's a separate problem to solve once
            // detection itself is confirmed working.
            let device = RAWINPUTDEVICE {
                usUsagePage: USAGE_PAGE_CONSUMER,
                usUsage: USAGE_CONSUMER_CONTROL,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            };
            if let Err(e) =
                RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32)
            {
                println!("[pairing] RegisterRawInputDevices failed: {e}");
                return;
            }
            println!("[pairing] listening for pairing-button presses (Consumer Control usage)");

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
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
            WM_INPUT => {
                handle_raw_input(lparam);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn handle_raw_input(lparam: LPARAM) {
        let handle = HRAWINPUT(lparam.0 as *mut _);

        let mut size: u32 = 0;
        GetRawInputData(
            handle,
            RID_INPUT,
            None,
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        );
        if size == 0 {
            return;
        }

        let mut buf = vec![0u8; size as usize];
        let copied = GetRawInputData(
            handle,
            RID_INPUT,
            Some(buf.as_mut_ptr() as *mut _),
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        );
        if copied == u32::MAX || (copied as usize) < std::mem::size_of::<RAWINPUTHEADER>() {
            return;
        }

        let raw = &*(buf.as_ptr() as *const RAWINPUT);
        if raw.header.dwType != RIM_TYPEHID.0 {
            return;
        }
        if !from_dji_device(raw.header.hDevice) {
            return;
        }

        let hid = &raw.data.hid;
        let report_size = hid.dwSizeHid as usize;
        if report_size < 2 {
            return;
        }
        let report = std::slice::from_raw_parts(hid.bRawData.as_ptr(), report_size);
        if super::debug_enabled() {
            println!("[pairing] raw report: {report:?}");
        }
        if report[1] != 0 {
            println!(">>> [PAIRING] button press detected");
        }
    }

    unsafe fn from_dji_device(hdevice: HANDLE) -> bool {
        let mut info = RID_DEVICE_INFO {
            cbSize: std::mem::size_of::<RID_DEVICE_INFO>() as u32,
            ..Default::default()
        };
        let mut size = info.cbSize;
        let ok = GetRawInputDeviceInfoW(
            Some(hdevice),
            RIDI_DEVICEINFO,
            Some(&mut info as *mut _ as *mut _),
            &mut size,
        );
        if ok == u32::MAX {
            return false;
        }
        let hid = info.Anonymous.hid;
        hid.dwVendorId == VID && hid.dwProductId == PID
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("collect") => {
            run_collect();
            return;
        }
        Some("train") => {
            run_train();
            return;
        }
        _ => {}
    }

    init_logger();

    println!("DJI Mic detect-test — Ctrl+C to exit");
    println!("Set DJIMIC_DEBUG=1 for verbose logs (near-miss taps, raw HID reports, device list)");
    println!("Other modes: `cargo run -- collect` to record labeled tap data, `cargo run -- train` to fit a classifier\n");

    std::thread::spawn(run_mic_tap);

    #[cfg(windows)]
    pairing::run();

    #[cfg(not(windows))]
    {
        println!("[pairing] pairing-button detection is Windows-only");
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
}
