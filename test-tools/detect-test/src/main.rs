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
//!
//! `cargo run -- collect` records the original six phases (quiet, speech,
//! loud speech, other environmental noise, nail taps, pad taps).
//! `cargo run -- collect-extra` appends two more hard-negative phases
//! (pairing-button press, blowing on the mic), and `cargo run --
//! collect-friction` appends one more (finger sliding/rubbing against the
//! mic shell, the kind of incidental contact a real button press involves
//! beyond just the button's own click) — none of these touch or redo
//! anything `collect` already recorded. All write to the same
//! `data/samples.csv` (append-only), then `cargo run -- train` retrains on
//! the combined file.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tap_model::features::{self, NoveltyState, VadState, N_BANDS, N_FEATURES};

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
/// Supplementary hard floors on top of the ML classifier — a nominal safety
/// net against literal silence/near-zero readings, not the primary filter.
/// Used to be set much more aggressively (peak>600, ratio>10) to reject
/// quiet/soft incidental sounds the model might slip on, but retraining on
/// *cleaned* labels (see `clean_peak_min` in `run_train`) plus a higher
/// `confidence_threshold` gave the classifier itself enough of that job that
/// the old floors were mostly rejecting genuine soft-but-real taps instead —
/// see PROTOCOL/CLAUDE.md history for the offline held-out sweep that found
/// this. Tune down further (independently) if genuine firm taps still don't
/// register; tune back up if false triggers from ambient noise become a
/// problem in practice.
const HARD_PEAK_FLOOR: f32 = 150.0;
const HARD_RATIO_FLOOR: f32 = 1.5;
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

/// A candidate only confirms if `ratio` dropped to at most this fraction of
/// its own peak (recorded when the candidate was raised) at some point
/// before `TAP_CONFIRM_DELAY` elapses — see `PendingTap::seen_decay`.
/// Targets continuous noise a single audio-chunk snapshot can otherwise
/// mistake for a tap (blowing on the mic, rubbing/scrubbing it): both stay
/// loud for hundreds of milliseconds, while a real mechanical tap's
/// shell-resonance ring-down dies out fast. Deliberately self-relative to
/// each candidate's own peak rather than the slow 20-chunk rolling baseline
/// `HARD_RATIO_FLOOR` compares against — an earlier, baseline-relative
/// version of this idea ("has it gone quiet since the last tap") was tried
/// and removed for blocking legitimate rapid double/triple taps, since that
/// baseline stays elevated through a fast burst. This version doesn't have
/// that problem: each candidate's decay is judged only against its own
/// starting peak, so a second real tap re-elevating the signal shortly after
/// doesn't retroactively undo the first candidate's own already-observed
/// decay.
const SUSTAIN_DECAY_FRACTION: f32 = 0.5;

/// Small neural net (1 hidden layer, tanh activation, optionally with a
/// small 1D-conv path over the spectral-band sequence, softmax output),
/// trained on real recorded data from this receiver via `cargo run --
/// collect` + `cargo run -- train`, replacing the hand-tuned peak/ratio
/// thresholds ported from the macOS reference (guessed for different
/// hardware, never fired on this mic). A plain binary logistic regression
/// over amplitude-only features (peak/rms/ratio) was tried first but
/// confused loud speech for taps — this adds real spectral features (see
/// `tap_model::features::spectral_bands`) so the model can use *timbre*,
/// not just level, to tell a hard mechanical knock from a vocal transient.
///
/// The weights, forward pass, training, *and* the feature-extraction/VAD
/// math itself all now live in the shared `tap-model` crate
/// (`../../crates/tap-model`) instead of being duplicated here and in
/// `gui/src-tauri/src/mic_tap.rs` — see that crate's doc comment and
/// `tap_model::features`'. `run_train` fits a new `tap_model::TapModel` and
/// writes it to `%APPDATA%\org.djimic.control\tap_model.json`, which the
/// real app picks up via its hot-swap poll with no rebuild needed. Only
/// classes `0 = not a tap` / `1 = tap` are modeled — nail/pad sub-classes
/// were dropped (see `tap_model::features`' doc comment for the offline
/// evidence: recall ~45%→~99.7% through the exact same runtime inference
/// gate) since nothing downstream ever branched on which anyway.
fn tap_predict(model: &tap_model::TapModel, features: [f32; N_FEATURES]) -> (usize, f32) {
    let result = model.predict(&features);
    (result.class, result.confidence)
}

/// Matches the Windows audio input device name against the receiver. Its
/// exact device string varies (driver/localization dependent), so this
/// checks loosely rather than for one fixed name.
fn is_dji_mic_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("mic rx") || lower.contains("wireless mic") || lower.contains("dji")
}

#[derive(Default)]
struct DetectionState {
    rms_window: VecDeque<f32>,
    tap_count: u32,
    /// Class index (per the live model's `class_names`) of each tap in the
    /// current burst, so the group-finalize line can show what kind each one was.
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
    /// Previous chunk's `ratio`/`ln(novelty)` — see
    /// `tap_model::features::build_feature_vector`'s delta features.
    prev_ratio: Option<f32>,
    prev_ln_novelty: Option<f32>,
}

struct PendingTap {
    detected_at: Instant,
    class: usize,
    /// `ratio` at the moment this candidate was raised — the yardstick
    /// `seen_decay` compares later chunks against.
    peak_ratio: f32,
    /// Set once `ratio` has dropped to `SUSTAIN_DECAY_FRACTION` of
    /// `peak_ratio` at some point since this candidate started. A real
    /// mechanical tap's ring-down dies out within a few chunks even during a
    /// fast multi-tap burst (the next tap's own impact re-elevates the
    /// signal, but only after this has already flipped true); continuous
    /// noise like blowing on the mic or rubbing/scrubbing it stays elevated
    /// the whole confirm window and never flips it. See the confirm loop in
    /// `process_chunk`, which rejects candidates that reach confirm time
    /// still `false`.
    seen_decay: bool,
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

fn process_chunk(model: &tap_model::TapModel, state: &mut DetectionState, samples: &[i16]) {
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
    let bands = features::spectral_bands(samples);
    let novelty = state.novelty.update(samples);
    let (attack_pos, energy_skew) = features::attack_shape(samples);
    let ln_novelty = novelty.max(1e-4).ln();
    let delta_ratio = ratio.max(1.0).ln() - state.prev_ratio.unwrap_or(ratio).max(1.0).ln();
    let delta_novelty = ln_novelty - state.prev_ln_novelty.unwrap_or(ln_novelty);
    state.prev_ratio = Some(ratio);
    state.prev_ln_novelty = Some(ln_novelty);

    let feature_vector =
        features::build_feature_vector(p, r, ratio, zcr, novelty, &bands, attack_pos, energy_skew, delta_ratio, delta_novelty);
    let (class, confidence) = tap_predict(model, feature_vector);
    let class_name = |c: usize| model.class_names.get(c).map(String::as_str).unwrap_or("?");

    // Every pending candidate watches every subsequent chunk for its own
    // decay, regardless of what else is happening this chunk — see
    // `PendingTap::seen_decay`.
    for pending in &mut state.pending_taps {
        if !pending.seen_decay && ratio < pending.peak_ratio * SUSTAIN_DECAY_FRACTION {
            pending.seen_decay = true;
        }
    }

    // The moment VAD catches up and flags speech, cancel every candidate
    // still waiting to be confirmed — those were speech onset, not real
    // taps (see `TAP_CONFIRM_DELAY`'s doc comment for why the classifier
    // reacts before the VAD does).
    if speech && !state.pending_taps.is_empty() {
        if debug_enabled() {
            for pending in &state.pending_taps {
                log_line(format!(
                    "[tap] cancelled pending {} — VAD caught up (score={:.2})",
                    class_name(pending.class), state.vad.last_score
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
        && confidence > model.confidence_threshold
        && !debounced
        && !speech
        && loud_enough
    {
        if debug_enabled() {
            log_line(format!(
                "[tap] candidate {} conf={confidence:.2} peak={p:.0} ratio={ratio:.1} novelty={novelty:.3} — waiting {TAP_CONFIRM_DELAY:?} to confirm",
                class_name(class)
            ));
        }
        state.pending_taps.push(PendingTap {
            detected_at: now,
            class,
            peak_ratio: ratio,
            seen_decay: false,
        });
        state.debounce_until = Some(now + DEBOUNCE);
    } else if debug_enabled()
        && warmed_up
        && class != 0
        && confidence > model.confidence_threshold
        && !debounced
        && !speech
        && !loud_enough
    {
        log_line(format!(
            "[tap] too quiet/soft, ignored: class={} conf={confidence:.2} peak={p:.0} (need >{HARD_PEAK_FLOOR}) ratio={ratio:.1} (need >{HARD_RATIO_FLOOR}) crest={crest:.1} (need >{HARD_CREST_FLOOR}) novelty={novelty:.3}",
            class_name(class)
        ));
    }

    // Confirm every pending candidate whose grace period has elapsed,
    // oldest first — usually at most one per call, but a burst of rapid
    // taps can confirm several in the same chunk.
    while state.pending_taps.first().is_some_and(|p| now.duration_since(p.detected_at) >= TAP_CONFIRM_DELAY) {
        let pending = state.pending_taps.remove(0);
        if !pending.seen_decay {
            if debug_enabled() {
                log_line(format!(
                    "[tap] rejected {} — never decayed, looks like sustained noise (blow/rub) not a tap",
                    class_name(pending.class)
                ));
            }
            continue;
        }
        let class = pending.class;
        if debug_enabled() {
            log_line(format!("[tap] confirmed {}", class_name(class)));
        }
        let within_window = state.last_tap.is_some_and(|t| now.duration_since(t) <= TAP_WINDOW);
        if within_window {
            state.tap_count += 1;
        } else {
            if state.tap_count > 0 {
                log_line(format!(
                    ">>> [TAP] {} tap(s) detected ({})",
                    state.tap_count,
                    state.tap_classes.iter().map(|&c| class_name(c)).collect::<Vec<_>>().join(", ")
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
                state.tap_classes.iter().map(|&c| class_name(c)).collect::<Vec<_>>().join(", ")
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

fn try_open_and_run(model: &Arc<tap_model::TapModel>) -> Result<(), String> {
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
            let model = model.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    let mono: Vec<i16> = data.chunks(channels.max(1)).map(|f| f[0]).collect();
                    process_chunk(&model, &mut state.lock().unwrap(), &mono);
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::F32 => {
            let state = state.clone();
            let model = model.clone();
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    let mono: Vec<i16> = data
                        .chunks(channels.max(1))
                        .map(|f| (f[0].clamp(-1.0, 1.0) * 32767.0) as i16)
                        .collect();
                    process_chunk(&model, &mut state.lock().unwrap(), &mono);
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
    // Loaded once at startup — this standalone test tool doesn't need the
    // real app's hot-swap poll (see gui/src-tauri/src/mic_tap.rs), just a
    // model consistent with whatever's on disk (or the embedded baseline).
    let path = tap_model::model_file_path().unwrap_or_default();
    let model = Arc::new(tap_model::TapModel::load_or_default(&path));
    println!(
        "[mic] loaded model: source={:?} trained_at={} rows={} confidence_threshold={:.2}",
        model.source, model.trained_at_unix_ms, model.training_rows, model.confidence_threshold
    );

    let mut logged_devices = false;
    loop {
        if let Err(e) = try_open_and_run(&model) {
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
/// Buffered rows for the current capture window, relabeled and flushed by
/// `finish_capture`.
static CAPTURE_BUFFER: Mutex<Vec<RawRow>> = Mutex::new(Vec::new());

/// One chunk's raw (not-yet-labeled) measurements — everything
/// `tap_model::features::build_feature_vector` needs, plus `peak` on its
/// own for the neighbor-labeling/cleaning floors. `centroid`/`hl_ratio`/
/// `rolloff`/`flatness` aren't stored separately since they're all derived
/// from `bands` at feature-vector-build time.
#[derive(Clone, Copy)]
struct RawRow {
    peak: f32,
    rms: f32,
    ratio: f32,
    zcr: f32,
    novelty: f32,
    bands: [f32; N_BANDS],
    attack_pos: f32,
    energy_skew: f32,
    delta_ratio: f32,
    delta_novelty: f32,
}

impl RawRow {
    fn write_csv(&self, w: &mut impl Write, label: u8) {
        let bands: Vec<String> = self.bands.iter().map(|b| format!("{b:.1}")).collect();
        let _ = writeln!(
            w,
            "{label},{:.1},{:.1},{:.2},{:.4},{:.4},{},{:.3},{:.3},{:.3},{:.3}",
            self.peak,
            self.rms,
            self.ratio,
            self.zcr,
            self.novelty,
            bands.join(","),
            self.attack_pos,
            self.energy_skew,
            self.delta_ratio,
            self.delta_novelty
        );
    }
}

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
/// mixing schemas if it doesn't match. Raw (not derived) measurements only
/// — matches `gui/src-tauri/src/tap_feedback.rs`'s feedback CSV schema
/// exactly, so files from either source can be folded together at
/// training time.
const CSV_HEADER: &str = "label,peak,rms,ratio,zcr,novelty,band0,band1,band2,band3,band4,band5,band6,band7,band8,band9,band10,band11,attack_pos,energy_skew,delta_ratio,delta_novelty";
const CSV_COLUMNS: usize = 22;

/// Per-stream state `write_feature_row` needs across calls — bundled into
/// one struct (rather than separate `Arc<Mutex<_>>`s per field, as an
/// earlier version had for just `rms_window`/`novelty_state`) now that the
/// delta features need their own carried-forward state too.
#[derive(Default)]
struct CollectState {
    rms_window: VecDeque<f32>,
    novelty: NoveltyState,
    prev_ratio: Option<f32>,
    prev_ln_novelty: Option<f32>,
}

fn write_feature_row(
    writer: &Arc<Mutex<std::io::BufWriter<std::fs::File>>>,
    state: &Arc<Mutex<CollectState>>,
    samples: &[i16],
) {
    let r = rms(samples);
    let p = peak(samples);
    let zcr = zero_crossing_rate(samples);
    let bands = features::spectral_bands(samples);
    let (attack_pos, energy_skew) = features::attack_shape(samples);

    let (ratio, novelty, delta_ratio, delta_novelty) = {
        let mut st = state.lock().unwrap();
        let novelty = st.novelty.update(samples);
        st.rms_window.push_back(r);
        if st.rms_window.len() > RMS_WINDOW_LEN {
            st.rms_window.pop_front();
        }
        let baseline = median(&st.rms_window).max(1.0);
        let ratio = p / baseline;
        let ln_novelty = novelty.max(1e-4).ln();
        let delta_ratio = ratio.max(1.0).ln() - st.prev_ratio.unwrap_or(ratio).max(1.0).ln();
        let delta_novelty = ln_novelty - st.prev_ln_novelty.unwrap_or(ln_novelty);
        st.prev_ratio = Some(ratio);
        st.prev_ln_novelty = Some(ln_novelty);
        (ratio, novelty, delta_ratio, delta_novelty)
    };

    let row = RawRow { peak: p, rms: r, ratio, zcr, novelty, bands, attack_pos, energy_skew, delta_ratio, delta_novelty };

    if CAPTURE_ACTIVE.load(Ordering::Relaxed) {
        CAPTURE_BUFFER.lock().unwrap().push(row);
        return;
    }

    let label = CURRENT_LABEL.load(Ordering::Relaxed);
    let mut w = writer.lock().unwrap();
    row.write_csv(&mut *w, label);
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

/// A capture-window neighbor of the loudest chunk only inherits the tap
/// label if it's still this loud — otherwise it's almost always the quiet
/// tail of the ring-down or a reaction-time gap, not the impact itself. An
/// offline analysis of the full recorded dataset found a substantial chunk
/// of "tap"-labeled rows sitting at near-zero peak amplitude (25th
/// percentile ~96, versus a median tap peak of ~1741) purely because of this
/// unconditional neighbor-labeling — training on those mislabeled-as-tap
/// silent frames was a real contributor to the classifier's poor recall.
/// Matches the `clean_peak_min` cleaning threshold `run_train` also applies
/// (belt-and-suspenders: this prevents new mislabels at collection time,
/// that one cleans up rows collected before this fix existed).
const CAPTURE_NEIGHBOR_MIN_PEAK: f32 = 1200.0;

/// Stops buffering and flushes the window to the CSV. Only the loudest
/// chunk gets the target tap label unconditionally; its immediate neighbors
/// (to cover the short decay) get it too, but only if they're still loud
/// enough to plausibly be part of the same impact rather than its quiet
/// tail — see `CAPTURE_NEIGHBOR_MIN_PEAK`. Labeling the *entire* window as
/// the tap class — what this replaced — mislabels most of it, since the
/// actual click only lasts a couple of chunks and its position inside the
/// window isn't fixed.
fn finish_capture(writer: &Arc<Mutex<std::io::BufWriter<std::fs::File>>>) {
    CAPTURE_ACTIVE.store(false, Ordering::Relaxed);
    let class = CAPTURE_LABEL.load(Ordering::Relaxed);
    let buffer = std::mem::take(&mut *CAPTURE_BUFFER.lock().unwrap());
    let Some((peak_idx, _)) =
        buffer.iter().enumerate().max_by(|a, b| a.1.peak.partial_cmp(&b.1.peak).unwrap())
    else {
        return;
    };

    let mut w = writer.lock().unwrap();
    for (i, row) in buffer.iter().enumerate() {
        let label = if i == peak_idx {
            class
        } else if i.abs_diff(peak_idx) <= 1 && row.peak >= CAPTURE_NEIGHBOR_MIN_PEAK {
            class
        } else {
            0
        };
        row.write_csv(&mut *w, label);
    }
    let _ = w.flush();
}

/// Like `finish_capture`, but labels *every* row in the window as background
/// (0), including its loudest moment — for hard-negative phases (pairing
/// button click, blowing on the mic) where the whole point is that the loud
/// part isn't a tap and the model needs to see it labeled that way, not just
/// the quiet edges around it.
fn finish_capture_as_background(writer: &Arc<Mutex<std::io::BufWriter<std::fs::File>>>) {
    CAPTURE_ACTIVE.store(false, Ordering::Relaxed);
    let buffer = std::mem::take(&mut *CAPTURE_BUFFER.lock().unwrap());
    let mut w = writer.lock().unwrap();
    for row in buffer.iter() {
        row.write_csv(&mut *w, 0);
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

    let collect_state: Arc<Mutex<CollectState>> = Arc::new(Mutex::new(CollectState::default()));
    let err_fn = |e: cpal::StreamError| println!("[collect] stream error: {e}");

    let stream = {
        let writer = writer.clone();
        let collect_state = collect_state.clone();
        match sample_format {
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    let mono: Vec<i16> = data.chunks(channels.max(1)).map(|f| f[0]).collect();
                    write_feature_row(&writer, &collect_state, &mono);
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
                    write_feature_row(&writer, &collect_state, &mono);
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

/// Two extra hard-negative phases (pairing-button press, blowing on the
/// mic), kept as a separate command from `run_collect` on purpose: both are
/// deliberately labeled 0 (background) in their entirety via
/// `finish_capture_as_background` rather than picking out a peak like a real
/// tap phase would, and running this doesn't require redoing (or risk
/// duplicating a lot of) the original six phases — it only appends these two
/// new ones to the same `samples.csv`, leaving everything already collected
/// untouched.
fn run_collect_extra() {
    let Some(device) = find_device() else {
        println!("[collect-extra] 没找到麦克风设备，请先确认普通模式下（cargo run，不带参数）能看到 [mic] opening ...");
        return;
    };

    let config = device.default_input_config().expect("default_input_config failed");
    let sample_format = config.sample_format();
    let channels = config.channels() as usize;
    println!(
        "[collect-extra] opening {:?} format={sample_format:?} channels={channels} rate={}",
        device.name(),
        config.sample_rate().0
    );
    let stream_config: cpal::StreamConfig = config.into();

    let dir = data_dir();
    std::fs::create_dir_all(&dir).expect("create data dir");
    let csv_path = dir.join("samples.csv");

    if let Ok(existing) = std::fs::read_to_string(&csv_path) {
        if existing.lines().next().is_some_and(|h| h != CSV_HEADER) {
            println!(
                "[collect-extra] {} 的表头和当前特征列不匹配，先运行一次 `cargo run -- collect` 让它按新格式重建，再来跑这个。",
                csv_path.display()
            );
            return;
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

    let collect_state: Arc<Mutex<CollectState>> = Arc::new(Mutex::new(CollectState::default()));
    let err_fn = |e: cpal::StreamError| println!("[collect-extra] stream error: {e}");

    let stream = {
        let writer = writer.clone();
        let collect_state = collect_state.clone();
        match sample_format {
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    let mono: Vec<i16> = data.chunks(channels.max(1)).map(|f| f[0]).collect();
                    write_feature_row(&writer, &collect_state, &mono);
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
                    write_feature_row(&writer, &collect_state, &mono);
                },
                err_fn,
                None,
            ),
            _ => {
                println!("[collect-extra] unsupported sample format");
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
    let prepare = |seconds: u64| {
        println!("准备中，{seconds} 秒后开始...");
        for remaining in (1..=seconds).rev() {
            print!("{remaining}...");
            std::io::stdout().flush().ok();
            std::thread::sleep(Duration::from_secs(1));
        }
        println!("开始！");
    };

    println!("\n=== 附加阶段 A：接收器配对键按下（硬负样本）===");
    println!("请按一下接收器上的配对键（短按后松开即可）。每次按回车后立刻按一下，共 30 次。");
    println!("这些全部记为“不是敲击”，让模型学会分辨按键的咔哒声和敲击麦克风外壳的区别。");
    prepare(10);
    for i in 1..=30 {
        wait_for_enter(&format!("按键 第 {i}/30 次，按回车后按下配对键: "));
        start_capture(0);
        std::thread::sleep(Duration::from_millis(700));
        finish_capture_as_background(&writer);
        std::thread::sleep(Duration::from_millis(300));
    }

    println!("\n=== 附加阶段 B：对着麦克风吹气（硬负样本）===");
    println!("请对着麦克风轻轻吹一口气（力度可以有变化）。每次按回车后立刻吹一下，共 30 次。");
    prepare(10);
    for i in 1..=30 {
        wait_for_enter(&format!("吹气 第 {i}/30 次，按回车后吹气: "));
        start_capture(0);
        std::thread::sleep(Duration::from_millis(700));
        finish_capture_as_background(&writer);
        std::thread::sleep(Duration::from_millis(300));
    }

    drop(stream);
    println!("\n[collect-extra] 采集完成，数据已追加写入 {}", csv_path.display());
    println!("[collect-extra] 原有数据没有变动，可以直接运行 `cargo run -- train` 重新训练。");
}

/// A third hard-negative phase, kept separate from `run_collect_extra` (its
/// own subcommand) so it doesn't force re-running the button-press/blow-air
/// phases just to add this one:手指划过/摩擦麦克风外壳的声音, specifically
/// because a real pairing-button press isn't just the button's own click —
/// reaching a finger over to it naturally brushes against the mic shell too,
/// and that friction sound is inconsistent enough in timing relative to the
/// HID press event (sometimes right before it, sometimes overlapping) that
/// suppressing purely off the press/release timestamps in
/// `pairing_button.rs` can't reliably catch it. Teaching the classifier to
/// recognize the sound itself, rather than only gating off button state,
/// covers it regardless of timing.
fn run_collect_friction() {
    let Some(device) = find_device() else {
        println!("[collect-friction] 没找到麦克风设备，请先确认普通模式下（cargo run，不带参数）能看到 [mic] opening ...");
        return;
    };

    let config = device.default_input_config().expect("default_input_config failed");
    let sample_format = config.sample_format();
    let channels = config.channels() as usize;
    println!(
        "[collect-friction] opening {:?} format={sample_format:?} channels={channels} rate={}",
        device.name(),
        config.sample_rate().0
    );
    let stream_config: cpal::StreamConfig = config.into();

    let dir = data_dir();
    std::fs::create_dir_all(&dir).expect("create data dir");
    let csv_path = dir.join("samples.csv");

    if let Ok(existing) = std::fs::read_to_string(&csv_path) {
        if existing.lines().next().is_some_and(|h| h != CSV_HEADER) {
            println!(
                "[collect-friction] {} 的表头和当前特征列不匹配，先运行一次 `cargo run -- collect` 让它按新格式重建，再来跑这个。",
                csv_path.display()
            );
            return;
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

    let collect_state: Arc<Mutex<CollectState>> = Arc::new(Mutex::new(CollectState::default()));
    let err_fn = |e: cpal::StreamError| println!("[collect-friction] stream error: {e}");

    let stream = {
        let writer = writer.clone();
        let collect_state = collect_state.clone();
        match sample_format {
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    let mono: Vec<i16> = data.chunks(channels.max(1)).map(|f| f[0]).collect();
                    write_feature_row(&writer, &collect_state, &mono);
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
                    write_feature_row(&writer, &collect_state, &mono);
                },
                err_fn,
                None,
            ),
            _ => {
                println!("[collect-friction] unsupported sample format");
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
    let prepare = |seconds: u64| {
        println!("准备中，{seconds} 秒后开始...");
        for remaining in (1..=seconds).rev() {
            print!("{remaining}...");
            std::io::stdout().flush().ok();
            std::thread::sleep(Duration::from_secs(1));
        }
        println!("开始！");
    };

    println!("\n=== 附加阶段 C：手指划过/摩擦麦克风外壳（硬负样本）===");
    println!("模拟你去按配对键时手指划过外壳的感觉：手指从外面移过来，轻轻蹭一下麦克风外壳，");
    println!("不用真的按到配对键。力度、角度、快慢都可以变化着来。每次按回车后立刻做一次，共 30 次。");
    prepare(10);
    for i in 1..=30 {
        wait_for_enter(&format!("摩擦 第 {i}/30 次，按回车后蹭一下外壳: "));
        start_capture(0);
        std::thread::sleep(Duration::from_millis(800));
        finish_capture_as_background(&writer);
        std::thread::sleep(Duration::from_millis(300));
    }

    drop(stream);
    println!("\n[collect-friction] 采集完成，数据已追加写入 {}", csv_path.display());
    println!("[collect-friction] 原有数据没有变动，可以直接运行 `cargo run -- train` 重新训练。");
}

// ---------------------------------------------------------------------
// Offline training (`cargo run -- train`): builds a `tap_model::TapModel`
// from the CSV `collect` produces. The forward pass, the batch-gradient-
// descent loop, and the model format all live in the shared `tap-model`
// crate now (see its doc comment) — this function's job is just the
// data-prep steps specific to *this* dataset: label cleaning, the
// nail/pad -> binary tap-vs-none collapse, and augmenting the still-rare
// positive class.
// ---------------------------------------------------------------------

/// A recorded tap row whose peak amplitude is below this is almost always a
/// mislabeled neighbor frame from the old (pre-`CAPTURE_NEIGHBOR_MIN_PEAK`)
/// collection code, not a genuine impact — relabeling it back to "none"
/// before training is what an offline held-out sweep over the full 128k-row
/// dataset found gave the single biggest recall improvement (far more than
/// any feature or architecture change), since a real fingernail/fingertip
/// tap on this hardware is essentially never this quiet. Matches
/// `CAPTURE_NEIGHBOR_MIN_PEAK` in spirit but tuned independently (empirical
/// sweep landed noticeably higher, at 1200, than what collection-time
/// filtering alone needs).
const TRAIN_CLEAN_PEAK_MIN: f32 = 1200.0;
/// Each surviving tap row is repeated this many times with small Gaussian
/// jitter (see `augment_row`) — the positive class is still only ~0.5% of
/// all rows even after cleaning, and this gave a measurable recall gain in
/// the same offline sweep without needing more real hardware recordings.
const TRAIN_AUGMENT_FACTOR: usize = 15;
/// Jitter magnitude as a fraction of the *tap class's own* per-feature
/// std-dev (not the whole dataset's, which is dominated by "none").
const TRAIN_AUGMENT_SIGMA: f32 = 0.25;

fn augment_row(rng: &mut tap_model::Rng, feat: &[f32; N_FEATURES], tap_std: &[f32; N_FEATURES]) -> [f32; N_FEATURES] {
    std::array::from_fn(|i| feat[i] + rng.next_gaussian() * TRAIN_AUGMENT_SIGMA * tap_std[i])
}

fn run_train(bake_default: bool) {
    let csv_path = data_dir().join("samples.csv");
    let content = match std::fs::read_to_string(&csv_path) {
        Ok(c) => c,
        Err(e) => {
            println!("[train] 读取 {} 失败: {e}", csv_path.display());
            println!("[train] 先运行 `cargo run -- collect` 采集数据。");
            return;
        }
    };

    // raw_label is 0/none, 1/nail, or 2/pad exactly as recorded — kept
    // around only to decide which rows are "quiet enough to relabel" below;
    // the model itself is trained on a plain binary tap-vs-none target,
    // since nail-vs-pad was never anything but a training-data trick (see
    // the doc comment above `tap_predict`) and merging them recalls far more
    // real taps.
    let mut raw: Vec<(f32, [f32; N_FEATURES])> = Vec::new();
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != CSV_COLUMNS {
            continue;
        }
        let Ok(values): Result<Vec<f32>, _> = parts.iter().map(|p| p.parse::<f32>()).collect() else {
            continue;
        };
        let label = values[0];
        let (p, r, ratio, zcr, novelty) = (values[1], values[2], values[3], values[4], values[5]);
        let bands: [f32; N_BANDS] = std::array::from_fn(|i| values[6 + i]);
        let attack_pos = values[6 + N_BANDS];
        let energy_skew = values[7 + N_BANDS];
        let delta_ratio = values[8 + N_BANDS];
        let delta_novelty = values[9 + N_BANDS];
        raw.push((
            label,
            features::build_feature_vector(p, r, ratio, zcr, novelty, &bands, attack_pos, energy_skew, delta_ratio, delta_novelty),
        ));
    }

    if raw.is_empty() {
        println!("[train] {} 里没有可用数据", csv_path.display());
        return;
    }

    // peak (feature index 0 is ln(peak)) — recover it to apply the cleaning
    // floor in the same units `collect` recorded it in.
    let peak_of = |feat: &[f32; N_FEATURES]| feat[0].exp();

    let mut relabeled = 0usize;
    let mut rows: Vec<(Vec<f32>, usize)> = Vec::new();
    let mut tap_feats_for_std: Vec<[f32; N_FEATURES]> = Vec::new();
    for (label, feat) in &raw {
        let is_tap_raw = *label != 0.0;
        let clean_tap = is_tap_raw && peak_of(feat) >= TRAIN_CLEAN_PEAK_MIN;
        if is_tap_raw && !clean_tap {
            relabeled += 1;
        }
        let class = if clean_tap { 1usize } else { 0usize };
        if class == 1 {
            tap_feats_for_std.push(*feat);
        }
        rows.push((feat.to_vec(), class));
    }

    let mut none_count = rows.iter().filter(|(_, c)| *c == 0).count();
    let tap_count = rows.len() - none_count;
    println!(
        "[train] loaded {} rows (none={none_count}, tap={tap_count}, relabeled {relabeled} quiet tap rows back to none)",
        rows.len()
    );
    if tap_count == 0 {
        println!("[train] 没有可用的 tap 样本，先采集数据。");
        return;
    }

    // Per-feature std of the (cleaned) tap rows only — used to scale
    // augmentation jitter to the positive class's own spread rather than
    // the whole dataset's (which is dominated by "none").
    let tap_mean: [f32; N_FEATURES] = std::array::from_fn(|i| {
        tap_feats_for_std.iter().map(|f| f[i]).sum::<f32>() / tap_feats_for_std.len() as f32
    });
    let tap_std: [f32; N_FEATURES] = std::array::from_fn(|i| {
        (tap_feats_for_std.iter().map(|f| (f[i] - tap_mean[i]).powi(2)).sum::<f32>()
            / tap_feats_for_std.len() as f32)
            .sqrt()
            .max(1e-3)
    });

    let mut rng = tap_model::Rng::new(0x9E3779B97F4A7C15);
    let mut augmented = 0usize;
    for (feat, class) in rows.clone() {
        if class == 0 {
            continue;
        }
        let feat_arr: [f32; N_FEATURES] = std::array::from_fn(|i| feat[i]);
        for _ in 0..TRAIN_AUGMENT_FACTOR {
            rows.push((augment_row(&mut rng, &feat_arr, &tap_std).to_vec(), 1));
            augmented += 1;
        }
    }
    none_count = rows.iter().filter(|(_, c)| *c == 0).count();
    println!(
        "[train] augmented tap rows x{TRAIN_AUGMENT_FACTOR} (+{augmented}) -> {} total rows (none={none_count}, tap={})",
        rows.len(),
        rows.len() - none_count
    );

    // Hidden width and confidence threshold (0.65) came out of the held-out
    // sweep documented in this project's history — a stricter bar than the
    // model's natural 0.5 boundary is affordable now that the binary
    // target's decision boundary is much cleaner. `n_bands`/`conv_channels`/
    // `conv_kernel` enable `TapModel`'s small 1D-conv path over the 12-band
    // spectral profile (see that struct's doc comment) instead of feeding
    // the whole 26-dim vector through one plain dense layer — hidden width
    // scaled up accordingly (conv pools to 6 channels + 14 scalars = 20
    // dense inputs, vs. the original 8-feature/8-hidden dense-only model).
    let cfg = tap_model::TrainConfig {
        n_hidden: 24,
        n_classes: 2,
        epochs: 3000,
        lr: 0.1,
        l2: 1e-4,
        class_names: vec!["none".to_string(), "tap".to_string()],
        confidence_threshold: 0.65,
        seed: 0x9E3779B97F4A7C15,
        n_bands: N_BANDS,
        conv_channels: 6,
        conv_kernel: 5,
    };
    let (model, report) = tap_model::train(&rows, N_FEATURES, &cfg);

    println!("[train] class counts (post-clean, post-augment): {:?}", report.class_counts);
    println!(
        "\n[train] 训练集准确率 {:.1}%（共 {} 条，含增强样本）",
        100.0 * report.train_accuracy,
        report.rows
    );
    println!("[train] 混淆矩阵（行=真实, 列=预测, 顺序 none/tap）：");
    for row in &report.confusion {
        println!("  {row:?}");
    }

    let live_path = match tap_model::model_file_path() {
        Some(p) => p,
        None => {
            println!("[train] 无法解析 %APPDATA%，跳过写入模型文件");
            return;
        }
    };
    match model.save_to_file(&live_path) {
        Ok(()) => println!("\n[train] 已写入 {}（正在运行的 app 会自动热加载）", live_path.display()),
        Err(e) => println!("\n[train] 写入 {} 失败: {e}", live_path.display()),
    }

    if bake_default {
        let baseline_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/tap-model/default_model.json");
        match model.save_to_file(&baseline_path) {
            Ok(()) => println!(
                "[train] --bake-default: 已更新 {}（下次编译会把它当作新装安装的默认模型）",
                baseline_path.display()
            ),
            Err(e) => println!("[train] --bake-default: 写入 {} 失败: {e}", baseline_path.display()),
        }
    }
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
        Some("collect-extra") => {
            run_collect_extra();
            return;
        }
        Some("collect-friction") => {
            run_collect_friction();
            return;
        }
        Some("train") => {
            let bake_default = args.iter().any(|a| a == "--bake-default");
            run_train(bake_default);
            return;
        }
        _ => {}
    }

    init_logger();

    println!("DJI Mic detect-test — Ctrl+C to exit");
    println!("Set DJIMIC_DEBUG=1 for verbose logs (near-miss taps, raw HID reports, device list)");
    println!("Other modes: `cargo run -- collect` to record labeled tap data, `cargo run -- collect-extra` for extra hard-negative phases (button press, blowing on the mic), `cargo run -- collect-friction` for finger-vs-shell friction, `cargo run -- train` to fit a classifier (add `--bake-default` to also refresh crates/tap-model/default_model.json's checked-in baseline)\n");

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
