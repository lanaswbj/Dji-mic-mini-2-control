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
//! - A 12-band Goertzel spectral profile plus rolloff/flatness/attack-shape/
//!   delta features (see `tap_model::features`) — real spectral *shape*,
//!   not just the two collapsed centroid/hl_ratio summaries the original
//!   4-band version produced, so loud speech doesn't look like a tap just
//!   because it's loud and the classifier can tell "this frequency profile
//!   is broadband and front-loaded" from "this one trails into harmonics".
//! - `earshot` (pure-Rust neural-net VAD) — gates tap classification off
//!   while speech is active; loud/sharp speech can slip past the
//!   classifier's own features otherwise. Replaced Silero
//!   (`voice_activity_detector`, which pulled in `ort` + a build-time-
//!   downloaded ONNX Runtime purely for this) — see
//!   `tap_model::features::VadState`'s doc comment for the version-history
//!   caveat.
//! - Spectral-flux onset novelty (`microdsp`) — a mature MIR (music
//!   information retrieval) technique for detecting percussive onsets,
//!   used as an extra feature rather than another hand-tuned "how sudden
//!   is this" threshold.
//! - All of the above feed a small trained neural net (1 hidden layer,
//!   optionally with a small 1D-conv path over the band sequence — see
//!   `tap_model::TapModel`'s doc comment — softmax, binary tap-vs-none)
//!   instead of fixed thresholds — the original macOS-reference thresholds
//!   never fired on this hardware at all. The weights/forward-pass/
//!   training now live in the shared `tap-model` crate as runtime-
//!   loadable, hot-swappable data instead of compile-time consts — see
//!   that crate's doc comment, and `tap_feedback` for the incremental-
//!   training/user-feedback loop built on top of it.
//! - Hard amplitude/transient floors on top of the model, and a short
//!   confirm-delay that lets a late-arriving VAD verdict retroactively
//!   cancel a candidate (the classifier reacts to speech onset before the
//!   VAD catches up) — see `TAP_CONFIRM_DELAY`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tap_model::features::{self, NoveltyState, VadState};

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

/// Supplementary hard floors on top of the ML classifier — a nominal safety
/// net against literal silence/near-zero readings, not the primary filter.
/// Used to be much more aggressive (peak>600, ratio>10) to reject quiet/soft
/// incidental sounds the model might slip on, but an offline held-out sweep
/// over the full recorded dataset found that retraining on *cleaned* labels
/// (see `tap-model`/`test-tools/detect-test`'s `TRAIN_CLEAN_PEAK_MIN`) plus a
/// higher confidence threshold gave the classifier itself that job just as
/// well, and the old floors were mostly rejecting genuine soft-but-real taps
/// instead. Tune down further (independently) if genuine firm taps still
/// don't register; tune back up if false triggers from ambient noise become
/// a problem in practice.
const HARD_PEAK_FLOOR: f32 = 150.0;
const HARD_RATIO_FLOOR: f32 = 1.5;
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

/// How long the frontend should consider a finalized tap group "active", so
/// a quick poll-based UI can still catch a momentary flash.
const ACTIVE_WINDOW: Duration = Duration::from_millis(700);

// ---------------------------------------------------------------------
// Trained classifier — weights/forward-pass/training live in the shared
// `tap-model` crate now (see its doc comment) as runtime-loadable, hot-
// swappable data instead of compile-time consts. `class != 0` is "a tap";
// an earlier version split taps into fingernail/fingertip-pad sub-classes
// purely as a training-data trick, but nothing downstream ever branched on
// which — merging them into a true binary target measurably improved real
// recall (see test-tools/detect-test's `run_train` doc comment). Feature
// extraction (Goertzel bands, novelty, VAD) also now lives in
// `tap_model::features`, shared with `test-tools/detect-test` — see that
// module's doc comment.
// ---------------------------------------------------------------------

/// Thin wrapper over `tap_model::TapModel::predict` — kept so call sites
/// read the same as before the model moved out to the shared crate.
fn tap_predict(model: &tap_model::TapModel, features: [f32; features::N_FEATURES]) -> (usize, f32) {
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

pub struct MicTapWatcher {
    last_group_count: AtomicU8,
    last_group_millis: AtomicU64,
    device_found: AtomicBool,
    /// Lets a finalized tap group drive the pie menu (see `finalize_group`)
    /// — open it on a double-tap while closed, or move the highlight
    /// right/left on a single/double tap while it's already open.
    app: tauri::AppHandle,
    /// Hot-swappable live model — `process_chunk` calls `.current()` once
    /// per chunk; a background poll thread (see `spawn`) and
    /// `tap_feedback`'s incremental trainer both call `.swap()` from
    /// entirely different threads with no locking on the read path.
    pub(crate) model: Arc<tap_model::TapModelStore>,
    /// Every chunk's raw measurements, unconditionally — including chunks
    /// that never fired a candidate at all (VAD-suppressed, below the hard
    /// floors, etc.) — so `tap_feedback`'s false-negative reporting can
    /// still find a real acoustic event even when the pipeline itself never
    /// raised one.
    pub(crate) ring: Arc<crate::tap_feedback::FeedbackRing>,
    /// Confirm instants of the most recently finalized tap group, for
    /// `tap_feedback::report_false_positive` to target — cleared/replaced
    /// wholesale on every new `finalize_group` call, not accumulated.
    pub(crate) last_group_taps: Mutex<Vec<Instant>>,
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
    ///
    /// Also drives the pie menu: while it's closed, a double tap opens it.
    /// While it's already open, a single tap moves the highlight right
    /// (wrapping back to the first slot past the last one — see
    /// `PieMenu.svelte`'s `move`); double tap no longer moves it left. A
    /// double tap that misfires as a single tap while navigating is common
    /// with this classifier in practice, so relying on it to distinguish
    /// left from right while open was unreliable — the single-tap-with-
    /// wraparound scheme reaches every slot without ever needing a double
    /// tap to be recognized correctly mid-navigation; double tap's only
    /// remaining job is the (reliable in practice) closed -> open trigger.
    fn finalize_group(&self, count: u32, taps: Vec<Instant>) {
        let reported = count.min(2);
        self.last_group_count.store(reported as u8, Ordering::Relaxed);
        self.last_group_millis.store(now_millis(), Ordering::Relaxed);
        *self.last_group_taps.lock().unwrap() = taps;

        if crate::pie_menu::is_showing(&self.app) {
            if reported == 1 {
                crate::pie_menu::navigate(&self.app, 1);
            }
        } else if reported == 2 {
            crate::pie_menu::open_if_closed(&self.app);
        }
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
    /// Confirm instants of every tap in the current burst, so
    /// `finalize_group` can pass the whole group to
    /// `MicTapWatcher::last_group_taps` for `tap_feedback`'s false-positive
    /// reporting to target.
    group_taps: Vec<Instant>,
    /// Previous chunk's `ratio`/`ln(novelty)`, for the delta-features in
    /// `tap_model::features::build_feature_vector` — `None` on the very
    /// first chunk, treated as "no change" (delta 0) rather than a spurious
    /// jump from an arbitrary baseline.
    prev_ratio: Option<f32>,
    prev_ln_novelty: Option<f32>,
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
/// How long after the press/release *edges* to suppress tap detection, on
/// top of suppressing for the entire held-down span (`pairing_button::is_held`)
/// — the button's own mechanical click is picked up by the mic and
/// otherwise reads as a shell tap, on both the press click and the release
/// click, neither of which is covered by `is_held` alone (press hasn't
/// registered as held yet on the very first chunk; release has already
/// flipped `is_held` back to false).
const BUTTON_TAP_SUPPRESS_WINDOW: Duration = Duration::from_millis(300);

fn process_chunk(watcher: &MicTapWatcher, state: &mut DetectionState, samples: &[i16]) {
    let now = Instant::now();
    state.vad.update(samples, now);
    let speech = state.vad.is_speech(now);
    let button_pressed = crate::pairing_button::is_held()
        || crate::pairing_button::recently_pressed(BUTTON_TAP_SUPPRESS_WINDOW);
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
    let bands = features::spectral_bands(samples);
    let novelty = state.novelty.update(samples);
    let (attack_pos, energy_skew) = features::attack_shape(samples);
    let ln_novelty = novelty.max(1e-4).ln();
    let delta_ratio = ratio.max(1.0).ln() - state.prev_ratio.unwrap_or(ratio).max(1.0).ln();
    let delta_novelty = ln_novelty - state.prev_ln_novelty.unwrap_or(ln_novelty);
    state.prev_ratio = Some(ratio);
    state.prev_ln_novelty = Some(ln_novelty);

    // Recorded unconditionally, before any suppression/floor branching below
    // — a tap the VAD gate suppressed or the hard floors rejected is exactly
    // the case `tap_feedback`'s false-negative reporting needs to still find
    // a real acoustic event for.
    watcher.ring.push(crate::tap_feedback::CapturedChunk {
        at: now,
        peak: p,
        rms: r,
        ratio,
        zcr,
        novelty,
        bands,
        attack_pos,
        energy_skew,
        delta_ratio,
        delta_novelty,
    });

    let model = watcher.model.current();
    let feature_vector =
        features::build_feature_vector(p, r, ratio, zcr, novelty, &bands, attack_pos, energy_skew, delta_ratio, delta_novelty);
    let (class, confidence) = tap_predict(&model, feature_vector);
    let class_name = |c: usize| model.class_names.get(c).map(String::as_str).unwrap_or("?");

    // Every pending candidate watches every subsequent chunk for its own
    // decay, regardless of what else is happening this chunk — see
    // `PendingTap::seen_decay`.
    for pending in &mut state.pending_taps {
        if !pending.seen_decay && ratio < pending.peak_ratio * SUSTAIN_DECAY_FRACTION {
            pending.seen_decay = true;
        }
    }

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
                    class_name(pending.class),
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
        && confidence > model.confidence_threshold
        && !debounced
        && !suppressed
        && loud_enough
    {
        if debug_enabled() {
            eprintln!(
                "[mic_tap] candidate {} conf={confidence:.2} peak={p:.0} ratio={ratio:.1} novelty={novelty:.3} — waiting {TAP_CONFIRM_DELAY:?} to confirm",
                class_name(class)
            );
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
        && !suppressed
        && !loud_enough
    {
        eprintln!(
            "[mic_tap] too quiet/soft, ignored: class={} conf={confidence:.2} peak={p:.0} (need >{HARD_PEAK_FLOOR}) ratio={ratio:.1} (need >{HARD_RATIO_FLOOR}) crest={crest:.1} (need >{HARD_CREST_FLOOR}) novelty={novelty:.3}",
            class_name(class)
        );
    }

    // Confirm every pending candidate whose grace period has elapsed,
    // oldest first — usually at most one per call, but a burst of rapid
    // taps can confirm several in the same chunk.
    while state.pending_taps.first().is_some_and(|p| now.duration_since(p.detected_at) >= TAP_CONFIRM_DELAY) {
        let pending = state.pending_taps.remove(0);
        if !pending.seen_decay {
            if debug_enabled() {
                eprintln!(
                    "[mic_tap] rejected {} — never decayed, looks like sustained noise (blow/rub) not a tap",
                    class_name(pending.class)
                );
            }
            continue;
        }
        let class = pending.class;
        if debug_enabled() {
            eprintln!("[mic_tap] confirmed {}", class_name(class));
        }
        let within_window = state.last_tap.is_some_and(|t| now.duration_since(t) <= TAP_WINDOW);
        if within_window {
            state.tap_count += 1;
        } else {
            if state.tap_count > 0 {
                if debug_enabled() {
                    eprintln!("[mic_tap] group finalized: {} tap(s)", state.tap_count);
                }
                watcher.finalize_group(state.tap_count, std::mem::take(&mut state.group_taps));
            }
            state.tap_count = 1;
        }
        state.group_taps.push(now);
        state.last_tap = Some(now);
    }

    if state.tap_count > 0 && state.pending_taps.is_empty() {
        if state.last_tap.is_some_and(|t| now.duration_since(t) > TAP_WINDOW) {
            if debug_enabled() {
                eprintln!("[mic_tap] group finalized: {} tap(s)", state.tap_count);
            }
            watcher.finalize_group(state.tap_count, std::mem::take(&mut state.group_taps));
            state.tap_count = 0;
        }
    }
}

/// Resolves to the same physical folder `tap-model`'s own
/// `app_data_dir()` fallback would (used by `test-tools/detect-test`, which
/// has no `AppHandle`) — prefers Tauri's own path resolver since it's the
/// canonical, futureproof API, falling back to the hand-rolled `%APPDATA%`
/// lookup only if that ever fails.
pub(crate) fn model_file_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .ok()
        .or_else(tap_model::app_data_dir)
        .unwrap_or_default();
    dir.join("tap_model.json")
}

/// How often the background thread checks whether `tap_model.json` changed
/// on disk (a full retrain via `detect-test`, or `tap_feedback`'s own
/// incremental update) — matches the codebase's existing 1-2s poll cadences
/// elsewhere rather than pulling in a filesystem-watcher crate for
/// something this infrequent.
const MODEL_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Watches `tap_model.json`'s mtime and hot-swaps the live model in when it
/// changes — no restart, no dropped audio frames. A bad/corrupt/schema-
/// mismatched file is just ignored (validated inside `load_from_file`),
/// leaving whatever's currently live untouched; see `tap_model`'s own doc
/// comment for the full edge-case list.
fn spawn_model_poll(model: Arc<tap_model::TapModelStore>, path: std::path::PathBuf) {
    std::thread::spawn(move || {
        let mut last_modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        loop {
            std::thread::sleep(MODEL_POLL_INTERVAL);
            let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) else {
                continue;
            };
            if last_modified.is_some_and(|prev| prev == modified) {
                continue;
            }
            last_modified = Some(modified);
            match tap_model::TapModel::load_from_file(&path) {
                Ok(new_model) => {
                    if debug_enabled() {
                        eprintln!(
                            "[mic_tap] hot-swapped model from {} (source={:?}, rows={})",
                            path.display(),
                            new_model.source,
                            new_model.training_rows
                        );
                    }
                    model.swap(new_model);
                }
                Err(e) => {
                    if debug_enabled() {
                        eprintln!("[mic_tap] ignoring invalid {}: {e}", path.display());
                    }
                }
            }
        }
    });
}

#[cfg(windows)]
pub fn spawn(app: tauri::AppHandle) -> Arc<MicTapWatcher> {
    let path = model_file_path(&app);
    let model = Arc::new(tap_model::TapModelStore::new(tap_model::TapModel::load_or_default(&path)));
    spawn_model_poll(model.clone(), path);

    let watcher = Arc::new(MicTapWatcher {
        last_group_count: AtomicU8::new(0),
        last_group_millis: AtomicU64::new(0),
        device_found: AtomicBool::new(false),
        app,
        model,
        ring: Arc::new(crate::tap_feedback::FeedbackRing::new()),
        last_group_taps: Mutex::new(Vec::new()),
    });
    let watcher_thread = watcher.clone();
    std::thread::spawn(move || win32::run(watcher_thread));
    watcher
}

#[cfg(not(windows))]
pub fn spawn(app: tauri::AppHandle) -> Arc<MicTapWatcher> {
    let path = model_file_path(&app);
    let model = Arc::new(tap_model::TapModelStore::new(tap_model::TapModel::load_or_default(&path)));
    spawn_model_poll(model.clone(), path);

    Arc::new(MicTapWatcher {
        last_group_count: AtomicU8::new(0),
        last_group_millis: AtomicU64::new(0),
        device_found: AtomicBool::new(false),
        app,
        model,
        ring: Arc::new(crate::tap_feedback::FeedbackRing::new()),
        last_group_taps: Mutex::new(Vec::new()),
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
