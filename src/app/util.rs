use std::{
    fs,
    path::Path,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use rayon::ThreadPoolBuilder;

pub(crate) fn remove_temp_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("removing temporary {}", path.display())),
    }
}

pub(crate) fn configure_threads() {
    let _ = ThreadPoolBuilder::new()
        .num_threads(cpu_thread_count())
        .build_global();
}

pub(crate) fn time_of_day_seed() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds_in_day = now.as_secs() % 86_400;
    (seconds_in_day << 32) ^ now.subsec_nanos() as u64
}

pub(crate) fn cpu_thread_count() -> usize {
    thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
}
