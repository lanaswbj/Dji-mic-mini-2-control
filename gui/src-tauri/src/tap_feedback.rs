//! Incremental training driven by user feedback on the mic-tap classifier
//! (see `mic_tap.rs`): "that wasn't a tap" (false positive) and "I tapped
//! and nothing happened" (false negative) buttons in the UI, wired through
//! `mic_tap_report_false_positive`/`mic_tap_report_false_negative` below.
//!
//! `mic_tap::process_chunk` unconditionally pushes every chunk's raw
//! measurements into a [`FeedbackRing`] (before any suppression/hard-floor
//! branching), so a tap the VAD gate suppressed or the hard floors rejected
//! is still findable when the user reports it as missed. Reporting either
//! kind of mistake:
//! 1. Finds the actual acoustic event in the ring buffer (the loudest
//!    recent chunk near the right instant) and appends it, correctly
//!    labeled, to a per-user CSV under the app's data dir — the same
//!    8-column schema `test-tools/detect-test` writes, so a future full
//!    retrain there can fold this back in.
//! 2. Kicks off a bounded, warm-started retrain
//!    ([`tap_model::continue_training`]) on a background thread: a handful
//!    of low-learning-rate epochs starting from the *live* model's weights,
//!    using the newly reported rows (up-weighted) plus a bulk "background"
//!    replay sample drawn from whatever's currently in the ring buffer
//!    (real, this-room, this-hardware ambient audio — a better-grounded
//!    negative-class replay than a static bundled dataset would be, and
//!    needs no extra shipped resource).
//! 3. Gates the result before ever touching the live model: candidate
//!    weights must be finite, and the candidate's accuracy on the replay
//!    sample's "none" rows must not regress past a small tolerance versus
//!    the model it started from (a cheap proxy for "did this just make
//!    false triggers on ordinary noise more likely"). A rejected candidate
//!    is simply discarded — its rows still made it into the CSV, so a
//!    later attempt (or a full offline retrain) benefits regardless.
//! 4. On acceptance, backs up the previous `tap_model.json` to
//!    `tap_model.json.bak` before atomically writing the new one, then
//!    hot-swaps it into the same `TapModelStore` `mic_tap`'s audio callback
//!    reads every chunk — see `mic_tap::spawn_model_poll` for the other
//!    (file-driven) way a model can hot-swap in.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::AppHandle;

use crate::mic_tap::MicTapWatcher;
use tap_model::features::{self, N_BANDS, N_FEATURES};

/// One audio chunk's raw measurements, timestamped — enough to rebuild the
/// exact feature vector `mic_tap::process_chunk` computed for it (via
/// `tap_model::features::build_feature_vector`), without storing the
/// derived vector itself.
#[derive(Clone, Copy)]
pub struct CapturedChunk {
    pub at: Instant,
    pub peak: f32,
    pub rms: f32,
    pub ratio: f32,
    pub zcr: f32,
    pub novelty: f32,
    pub bands: [f32; N_BANDS],
    pub attack_pos: f32,
    pub energy_skew: f32,
    pub delta_ratio: f32,
    pub delta_novelty: f32,
}

impl CapturedChunk {
    fn features(&self) -> [f32; N_FEATURES] {
        features::build_feature_vector(
            self.peak,
            self.rms,
            self.ratio,
            self.zcr,
            self.novelty,
            &self.bands,
            self.attack_pos,
            self.energy_skew,
            self.delta_ratio,
            self.delta_novelty,
        )
    }
}

/// How much history to keep — long enough to cover a slow human reaction
/// time to a missed tap (false-negative lookback) plus enough bulk to serve
/// as a "background" replay sample for incremental training.
const RING_MAX_AGE: Duration = Duration::from_secs(60);
/// Hard cap independent of age, so a very high chunk rate can't grow this
/// unboundedly — this is an in-memory ring buffer, never persisted.
const RING_MAX_LEN: usize = 8000;

pub struct FeedbackRing {
    buf: Mutex<VecDeque<CapturedChunk>>,
}

impl FeedbackRing {
    pub fn new() -> Self {
        FeedbackRing { buf: Mutex::new(VecDeque::new()) }
    }

    pub fn push(&self, chunk: CapturedChunk) {
        let mut buf = self.buf.lock().unwrap();
        buf.push_back(chunk);
        while buf.len() > RING_MAX_LEN {
            buf.pop_front();
        }
        if let Some(cutoff) = chunk.at.checked_sub(RING_MAX_AGE) {
            while buf.front().is_some_and(|c| c.at < cutoff) {
                buf.pop_front();
            }
        }
    }

    /// The single loudest chunk within `radius` of `center`, if any.
    fn loudest_near(&self, center: Instant, radius: Duration) -> Option<CapturedChunk> {
        let buf = self.buf.lock().unwrap();
        buf.iter()
            .filter(|c| {
                let dt = if c.at >= center { c.at - center } else { center - c.at };
                dt <= radius
            })
            .max_by(|a, b| a.peak.partial_cmp(&b.peak).unwrap())
            .copied()
    }

    /// The loudest chunk in the trailing `lookback` window ending at `now`.
    fn loudest_trailing(&self, now: Instant, lookback: Duration) -> Option<CapturedChunk> {
        let cutoff = now.checked_sub(lookback).unwrap_or(now);
        let buf = self.buf.lock().unwrap();
        buf.iter().filter(|c| c.at >= cutoff && c.at <= now).max_by(|a, b| a.peak.partial_cmp(&b.peak).unwrap()).copied()
    }

    /// Up to `max` chunks sampled from the whole current buffer, evenly
    /// spaced (not random — this buffer only holds ~1 minute of audio, so a
    /// stride keeps the sample spread across it instead of clustering).
    fn sample(&self, max: usize) -> Vec<CapturedChunk> {
        let buf = self.buf.lock().unwrap();
        if buf.len() <= max {
            return buf.iter().copied().collect();
        }
        let stride = buf.len() / max;
        buf.iter().step_by(stride.max(1)).take(max).copied().collect()
    }
}

/// The candidate's own confirm loop uses `TAP_CONFIRM_DELAY` between when a
/// candidate is *raised* (the acoustic event itself) and when it's
/// *confirmed* (pushed into `last_group_taps`) — so the loudest real chunk
/// sits *before* the confirm instant, not at it.
const CONFIRM_DELAY_ESTIMATE: Duration = Duration::from_millis(150);
/// Search window around the estimated acoustic-event instant.
const FALSE_POSITIVE_SEARCH_RADIUS: Duration = Duration::from_millis(250);
/// How far back a false-negative report looks for the tap the user says the
/// app missed — covers realistic reaction time to notice+click.
const FALSE_NEGATIVE_LOOKBACK: Duration = Duration::from_secs(3);
/// A false-negative candidate must clear this to be accepted as a real
/// acoustic event at all — well below the (already low) production
/// `HARD_PEAK_FLOOR`, just enough to reject "clicked the button but nothing
/// happened" mis-clicks.
const FALSE_NEGATIVE_MIN_PEAK: f32 = 250.0;

/// Raw (not derived) measurements only — centroid/hl_ratio/rolloff/flatness
/// are all recomputed from `bands` at feature-vector-build time, so they
/// don't need their own columns. Matches `test-tools/detect-test`'s
/// `samples.csv` schema exactly, so files from either source can be folded
/// together at training time.
const CSV_HEADER: &str = "label,peak,rms,ratio,zcr,novelty,band0,band1,band2,band3,band4,band5,band6,band7,band8,band9,band10,band11,attack_pos,energy_skew,delta_ratio,delta_novelty";
const CSV_COLUMNS: usize = 22;

fn feedback_csv_path(app: &AppHandle) -> PathBuf {
    use tauri::Manager;
    let dir = app.path().app_data_dir().ok().or_else(tap_model::app_data_dir).unwrap_or_default();
    dir.join("tap_feedback.csv")
}

fn append_row(path: &PathBuf, label: u8, chunk: &CapturedChunk) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let is_new = !path.exists();
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    if is_new {
        writeln!(file, "{CSV_HEADER}")?;
    }
    let bands: Vec<String> = chunk.bands.iter().map(|b| format!("{b:.1}")).collect();
    writeln!(
        file,
        "{label},{:.1},{:.1},{:.2},{:.4},{:.4},{},{:.3},{:.3},{:.3},{:.3}",
        chunk.peak,
        chunk.rms,
        chunk.ratio,
        chunk.zcr,
        chunk.novelty,
        bands.join(","),
        chunk.attack_pos,
        chunk.energy_skew,
        chunk.delta_ratio,
        chunk.delta_novelty
    )?;
    Ok(())
}

fn read_feedback_rows(path: &PathBuf) -> Vec<(Vec<f32>, usize)> {
    let Ok(content) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut rows = Vec::new();
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != CSV_COLUMNS {
            continue;
        }
        let Ok(values): Result<Vec<f32>, _> = parts.iter().map(|p| p.parse::<f32>()).collect() else {
            continue;
        };
        let label = values[0];
        let (peak, rms, ratio, zcr, novelty) = (values[1], values[2], values[3], values[4], values[5]);
        let bands: [f32; N_BANDS] = std::array::from_fn(|i| values[6 + i]);
        let attack_pos = values[6 + N_BANDS];
        let energy_skew = values[7 + N_BANDS];
        let delta_ratio = values[8 + N_BANDS];
        let delta_novelty = values[9 + N_BANDS];
        let class = if label as i32 != 0 { 1usize } else { 0usize };
        let features = features::build_feature_vector(
            peak, rms, ratio, zcr, novelty, &bands, attack_pos, energy_skew, delta_ratio, delta_novelty,
        );
        rows.push((features.to_vec(), class));
    }
    rows
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingStatus {
    /// `"idle"` or `"training"`.
    pub state: &'static str,
    pub model_source: String,
    pub model_trained_at_unix_ms: u64,
    pub model_training_rows: usize,
    pub model_confidence_threshold: f32,
    pub feedback_row_count: usize,
    pub can_rollback: bool,
    pub last_message: String,
}

struct TrainerInner {
    training: AtomicBool,
    last_trained: Mutex<Option<Instant>>,
    last_message: Mutex<String>,
}

pub struct TapTrainer {
    app: AppHandle,
    inner: Arc<TrainerInner>,
}

/// Minimum gap between two accepted incremental updates — repeated rapid
/// feedback clicks all still get their rows appended to the CSV, but only
/// the first triggers a retrain; the rest ride along on the next one.
const MIN_RETRAIN_INTERVAL: Duration = Duration::from_secs(20);

impl TapTrainer {
    pub fn spawn(app: AppHandle) -> Arc<TapTrainer> {
        Arc::new(TapTrainer {
            app,
            inner: Arc::new(TrainerInner {
                training: AtomicBool::new(false),
                last_trained: Mutex::new(None),
                last_message: Mutex::new("就绪".to_string()),
            }),
        })
    }

    pub fn report_false_positive(&self, watcher: &MicTapWatcher) -> Result<(), String> {
        let taps = watcher.last_group_taps.lock().unwrap().clone();
        if taps.is_empty() {
            return Err("最近没有可撤销的敲击".to_string());
        }
        let csv_path = feedback_csv_path(&self.app);
        let mut found = 0;
        for confirmed_at in taps {
            let estimated_event = confirmed_at.checked_sub(CONFIRM_DELAY_ESTIMATE).unwrap_or(confirmed_at);
            if let Some(chunk) = watcher.ring.loudest_near(estimated_event, FALSE_POSITIVE_SEARCH_RADIUS) {
                let _ = append_row(&csv_path, 0, &chunk);
                found += 1;
            }
        }
        if found == 0 {
            return Err("没能在最近的录音里找到对应的声音片段".to_string());
        }
        self.kick_off_training();
        Ok(())
    }

    pub fn report_false_negative(&self, watcher: &MicTapWatcher) -> Result<(), String> {
        let now = Instant::now();
        let Some(chunk) = watcher.ring.loudest_trailing(now, FALSE_NEGATIVE_LOOKBACK) else {
            return Err("最近没有捕捉到任何声音".to_string());
        };
        if chunk.peak < FALSE_NEGATIVE_MIN_PEAK {
            return Err("最近几秒内没有检测到明显的敲击声，请在敲击后尽快点击反馈".to_string());
        }
        let csv_path = feedback_csv_path(&self.app);
        let _ = append_row(&csv_path, 1, &chunk);
        self.kick_off_training();
        Ok(())
    }

    fn kick_off_training(&self) {
        if self.inner.training.swap(true, Ordering::SeqCst) {
            // Already training — this feedback's row is saved either way;
            // it'll be picked up next time.
            return;
        }
        let too_soon = self
            .inner
            .last_trained
            .lock()
            .unwrap()
            .is_some_and(|t| t.elapsed() < MIN_RETRAIN_INTERVAL);
        if too_soon {
            self.inner.training.store(false, Ordering::SeqCst);
            return;
        }

        let app = self.app.clone();
        let inner = self.inner.clone();
        // Needs the watcher (for the model store + ring buffer) but that's
        // only available as Tauri-managed state, not stored on `self` —
        // resolved fresh inside the thread via the app handle.
        std::thread::spawn(move || {
            use tauri::Manager;
            let result = app
                .try_state::<Arc<MicTapWatcher>>()
                .map(|w| run_incremental_update(&app, w.inner()))
                .unwrap_or_else(|| Err("mic_tap watcher not ready".to_string()));
            match result {
                Ok(msg) => {
                    *inner.last_message.lock().unwrap() = msg;
                    *inner.last_trained.lock().unwrap() = Some(Instant::now());
                }
                Err(msg) => {
                    *inner.last_message.lock().unwrap() = msg;
                }
            }
            inner.training.store(false, Ordering::SeqCst);
        });
    }

    pub fn status(&self, watcher: Option<&MicTapWatcher>) -> TrainingStatus {
        let model = watcher.map(|w| w.model.current());
        let feedback_row_count = read_feedback_rows(&feedback_csv_path(&self.app)).len();
        let bak_path = model_backup_path(&self.app);
        TrainingStatus {
            state: if self.inner.training.load(Ordering::SeqCst) { "training" } else { "idle" },
            model_source: model.as_ref().map(|m| format!("{:?}", m.source)).unwrap_or_default(),
            model_trained_at_unix_ms: model.as_ref().map(|m| m.trained_at_unix_ms).unwrap_or(0),
            model_training_rows: model.as_ref().map(|m| m.training_rows).unwrap_or(0),
            model_confidence_threshold: model.as_ref().map(|m| m.confidence_threshold).unwrap_or(0.0),
            feedback_row_count,
            can_rollback: bak_path.exists(),
            last_message: self.inner.last_message.lock().unwrap().clone(),
        }
    }

    pub fn rollback(&self, watcher: &MicTapWatcher) -> Result<(), String> {
        let bak_path = model_backup_path(&self.app);
        let restored = tap_model::TapModel::load_from_file(&bak_path)?;
        let live_path = crate::mic_tap::model_file_path(&self.app);
        restored.save_to_file(&live_path).map_err(|e| e.to_string())?;
        watcher.model.swap(restored);
        *self.inner.last_message.lock().unwrap() = "已回滚到上一个模型".to_string();
        Ok(())
    }

    pub fn restore_factory(&self, watcher: &MicTapWatcher) -> Result<(), String> {
        let live_path = crate::mic_tap::model_file_path(&self.app);
        let bak_path = model_backup_path(&self.app);
        if live_path.exists() {
            let _ = std::fs::copy(&live_path, &bak_path);
        }
        let factory = tap_model::TapModel::embedded_default();
        factory.save_to_file(&live_path).map_err(|e| e.to_string())?;
        watcher.model.swap(factory);
        *self.inner.last_message.lock().unwrap() = "已恢复出厂模型".to_string();
        Ok(())
    }
}

fn model_backup_path(app: &AppHandle) -> PathBuf {
    crate::mic_tap::model_file_path(app).with_extension("json.bak")
}

fn now_unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The actual bounded, warm-started retrain — runs entirely off the audio
/// thread. Returns a human-readable (Chinese) status message either way.
fn run_incremental_update(app: &AppHandle, watcher: &Arc<MicTapWatcher>) -> Result<String, String> {
    let csv_path = feedback_csv_path(app);
    let feedback_rows = read_feedback_rows(&csv_path);
    if feedback_rows.is_empty() {
        return Err("没有可用的反馈数据".to_string());
    }

    // Bulk "background" replay: whatever's currently in the ring buffer,
    // labeled `none` by default. This is real, this-room, this-hardware
    // ambient audio rather than a static bundled dataset, and needs no
    // extra shipped resource — the tradeoff is it can't include *rare*
    // hard negatives (blowing, button clicks) unless they happened to occur
    // in the last minute, which is why the safety gate below (not just
    // "did training converge") is what actually protects the live model.
    let replay: Vec<(Vec<f32>, usize)> =
        watcher.ring.sample(400).iter().map(|c| (c.features().to_vec(), 0usize)).collect();
    if replay.len() < 20 {
        return Err("最近的录音样本太少，暂不满足增量训练条件".to_string());
    }

    let current = watcher.model.current();
    let mut train_rows = replay.clone();
    // Up-weight the (few) explicit feedback rows by simple repetition,
    // matching detect-test's own augmentation-by-repetition approach rather
    // than a from-scratch class-weighting recompute (this is a small warm
    // start, not a full refit).
    const FEEDBACK_REPEAT: usize = 6;
    for _ in 0..FEEDBACK_REPEAT {
        train_rows.extend(feedback_rows.iter().cloned());
    }

    let cfg = tap_model::TrainConfig {
        n_hidden: current.n_hidden,
        n_classes: current.n_classes,
        epochs: 250,
        lr: 0.03,
        l2: 1e-4,
        class_names: current.class_names.clone(),
        confidence_threshold: current.confidence_threshold,
        seed: now_unix_ms(),
        // Ignored by `continue_training`, which always inherits `current`'s
        // own architecture (n_bands/conv_channels/conv_kernel) instead.
        n_bands: 0,
        conv_channels: 0,
        conv_kernel: 0,
    };
    let (candidate, _report) = tap_model::continue_training(&current, &train_rows, N_FEATURES, &cfg);

    if candidate.validate().is_err() {
        return Err("增量训练结果无效，已放弃这次更新（反馈已保存，下次会重新尝试）".to_string());
    }
    let finite = candidate.w1.iter().flatten().all(|v| v.is_finite())
        && candidate.w2.iter().flatten().all(|v| v.is_finite())
        && candidate.b1.iter().all(|v| v.is_finite())
        && candidate.b2.iter().all(|v| v.is_finite());
    if !finite {
        return Err("增量训练结果包含非法数值，已放弃这次更新".to_string());
    }

    // Safety gate: candidate's "none" accuracy on the same replay sample
    // must not regress past a small tolerance versus the model it started
    // from — a cheap proxy for "did this just make false triggers on
    // ordinary ambient noise more likely".
    const MAX_NONE_ACCURACY_DROP: f32 = 0.03;
    let none_accuracy = |m: &tap_model::TapModel| -> f32 {
        if replay.is_empty() {
            return 1.0;
        }
        let correct = replay.iter().filter(|(f, _)| m.predict(f).class == 0).count();
        correct as f32 / replay.len() as f32
    };
    let before = none_accuracy(&current);
    let after = none_accuracy(&candidate);
    if after + MAX_NONE_ACCURACY_DROP < before {
        return Err(format!(
            "增量训练结果可能增加误触发（背景准确率 {:.1}% → {:.1}%），已放弃这次更新（反馈已保存）",
            before * 100.0,
            after * 100.0
        ));
    }

    let live_path = crate::mic_tap::model_file_path(app);
    let bak_path = model_backup_path(app);
    if live_path.exists() {
        let _ = std::fs::copy(&live_path, &bak_path);
    }
    candidate.save_to_file(&live_path).map_err(|e| e.to_string())?;
    watcher.model.swap(candidate);

    Ok(format!(
        "增量训练完成：{} 条反馈样本，背景准确率 {:.1}% → {:.1}%",
        feedback_rows.len(),
        before * 100.0,
        after * 100.0
    ))
}

#[tauri::command]
pub fn mic_tap_report_false_positive(
    watcher: tauri::State<'_, Arc<MicTapWatcher>>,
    trainer: tauri::State<'_, Arc<TapTrainer>>,
) -> Result<(), String> {
    trainer.report_false_positive(&watcher)
}

#[tauri::command]
pub fn mic_tap_report_false_negative(
    watcher: tauri::State<'_, Arc<MicTapWatcher>>,
    trainer: tauri::State<'_, Arc<TapTrainer>>,
) -> Result<(), String> {
    trainer.report_false_negative(&watcher)
}

#[tauri::command]
pub fn mic_tap_training_status(
    watcher: tauri::State<'_, Arc<MicTapWatcher>>,
    trainer: tauri::State<'_, Arc<TapTrainer>>,
) -> TrainingStatus {
    trainer.status(Some(&watcher))
}

#[tauri::command]
pub fn mic_tap_rollback_model(
    watcher: tauri::State<'_, Arc<MicTapWatcher>>,
    trainer: tauri::State<'_, Arc<TapTrainer>>,
) -> Result<(), String> {
    trainer.rollback(&watcher)
}

#[tauri::command]
pub fn mic_tap_restore_factory_model(
    watcher: tauri::State<'_, Arc<MicTapWatcher>>,
    trainer: tauri::State<'_, Arc<TapTrainer>>,
) -> Result<(), String> {
    trainer.restore_factory(&watcher)
}
