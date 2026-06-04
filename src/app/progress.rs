use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressStyle};

pub(crate) struct ApplyProgress<'a> {
    pub(crate) file: &'a ProgressBar,
    pub(crate) started: Instant,
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
    progress.file.set_position(position);
    progress.file.set_message(format!(
        "{} ({})",
        step,
        format_duration(progress.started.elapsed())
    ));
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
