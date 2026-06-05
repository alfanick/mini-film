use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use indicatif::{ProgressBar, ProgressStyle};

pub(crate) struct ApplyProgress<'a> {
    pub(crate) file: &'a ProgressBar,
    pub(crate) started: Instant,
    pub(crate) estimates: Option<Arc<StageEstimates>>,
}

pub(crate) const FILE_PROGRESS_STEPS: u64 = 5;
const STEP_UNITS: u64 = 100;
const TICK_INTERVAL: Duration = Duration::from_millis(150);
const ESTIMATE_ALPHA: f64 = 0.35;

#[derive(Default)]
pub(crate) struct StageEstimates {
    durations: Mutex<HashMap<&'static str, Duration>>,
}

impl StageEstimates {
    pub(crate) fn estimate(&self, stage: &'static str, fallback: Duration) -> Duration {
        self.durations
            .lock()
            .ok()
            .and_then(|durations| durations.get(stage).copied())
            .unwrap_or(fallback)
    }

    fn record(&self, stage: &'static str, actual: Duration) {
        let Ok(mut durations) = self.durations.lock() else {
            return;
        };
        durations
            .entry(stage)
            .and_modify(|estimate| {
                let blended = estimate.as_secs_f64() * (1.0 - ESTIMATE_ALPHA)
                    + actual.as_secs_f64() * ESTIMATE_ALPHA;
                *estimate = Duration::from_secs_f64(blended.max(TICK_INTERVAL.as_secs_f64()));
            })
            .or_insert(actual.max(TICK_INTERVAL));
    }
}

pub(crate) fn batch_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} batch [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} {msg}",
    )
    .unwrap()
    .progress_chars("#>-")
}

pub(crate) fn file_progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.green} file  [{elapsed_precise}] [{wide_bar:.magenta/blue}] {pos}/{len} {msg}",
    )
    .unwrap()
    .progress_chars("#>-")
}

pub(crate) fn progress_step(progress: Option<&ApplyProgress<'_>>, position: u64, step: &str) {
    let Some(progress) = progress else {
        return;
    };
    set_progress(
        progress.file,
        progress.started,
        progress_position(position),
        step,
    );
}

pub(crate) fn progress_length() -> u64 {
    progress_position(FILE_PROGRESS_STEPS)
}

pub(crate) fn progress_position(step: u64) -> u64 {
    step.saturating_mul(STEP_UNITS)
}

pub(crate) fn set_progress(file: &ProgressBar, started: Instant, position: u64, step: &str) {
    file.set_position(position);
    file.set_message(format!("{} ({})", step, format_duration(started.elapsed())));
}

pub(crate) fn progress_stage(
    progress: Option<&ApplyProgress<'_>>,
    start_step: u64,
    end_step: u64,
    step: &'static str,
    estimate: Duration,
) -> StageProgress {
    let Some(progress) = progress else {
        return StageProgress::inactive();
    };
    StageProgress::start(
        progress.file.clone(),
        progress.started,
        None,
        progress_position(start_step),
        progress_position(end_step),
        step,
        estimate,
    )
}

pub(crate) fn progress_stage_adaptive(
    progress: Option<&ApplyProgress<'_>>,
    start_step: u64,
    end_step: u64,
    stage_key: &'static str,
    step: &'static str,
    fallback: Duration,
) -> StageProgress {
    let Some(progress) = progress else {
        return StageProgress::inactive();
    };
    let estimate = progress
        .estimates
        .as_ref()
        .map(|estimates| estimates.estimate(stage_key, fallback))
        .unwrap_or(fallback);
    StageProgress::start(
        progress.file.clone(),
        progress.started,
        progress
            .estimates
            .clone()
            .map(|estimates| (estimates, stage_key)),
        progress_position(start_step),
        progress_position(end_step),
        step,
        estimate,
    )
}

pub(crate) struct StageProgress {
    file: Option<ProgressBar>,
    started: Instant,
    stage_started: Instant,
    end: u64,
    step: &'static str,
    estimate_sink: Option<(Arc<StageEstimates>, &'static str)>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl StageProgress {
    fn inactive() -> Self {
        Self {
            file: None,
            started: Instant::now(),
            stage_started: Instant::now(),
            end: 0,
            step: "",
            estimate_sink: None,
            stop: Arc::new(AtomicBool::new(true)),
            handle: None,
        }
    }

    fn start(
        file: ProgressBar,
        started: Instant,
        estimate_sink: Option<(Arc<StageEstimates>, &'static str)>,
        start: u64,
        end: u64,
        step: &'static str,
        estimate: Duration,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_file = file.clone();
        let span = end.saturating_sub(start).max(1);
        let estimate = estimate.max(TICK_INTERVAL);
        set_progress(&file, started, start, step);
        let stage_started = Instant::now();

        let handle = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                let elapsed = stage_started.elapsed();
                let fraction = (elapsed.as_secs_f64() / estimate.as_secs_f64()).min(0.98);
                let position = start + ((span as f64 * fraction) as u64).min(span - 1);
                worker_file.set_position(position);
                worker_file.set_message(format!(
                    "{} {:>3}% ({})",
                    step,
                    (fraction * 100.0) as u64,
                    format_duration(started.elapsed())
                ));
                thread::sleep(TICK_INTERVAL);
            }
        });

        Self {
            file: Some(file),
            started,
            stage_started,
            end,
            step,
            estimate_sink,
            stop,
            handle: Some(handle),
        }
    }

    pub(crate) fn finish(mut self) {
        self.stop();
        if let Some((estimates, key)) = &self.estimate_sink {
            estimates.record(key, self.stage_started.elapsed());
        }
        if let Some(file) = &self.file {
            set_progress(file, self.started, self.end, self.step);
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for StageProgress {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let millis = duration.subsec_millis();
    if seconds >= 60 {
        format!("{}m{:02}.{:03}s", seconds / 60, seconds % 60, millis)
    } else {
        format!("{seconds}.{millis:03}s")
    }
}
