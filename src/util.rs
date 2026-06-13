use std::{
    env, fs,
    path::{Path, PathBuf},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use rayon::ThreadPoolBuilder;

pub const SUPPORTED_RAW_EXTENSIONS: &[&str] = &[
    "arw", "cr2", "cr3", "crw", "dcr", "dng", "erf", "mrw", "nef", "nrw", "orf", "pef", "raf",
    "raw", "rwl", "rw2", "rwz", "r3d", "sr2", "srf", "srw", "x3f",
];

pub fn is_supported_raw_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            SUPPORTED_RAW_EXTENSIONS
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
        })
}

pub fn remove_temp_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("removing temporary {}", path.display())),
    }
}

pub fn configure_threads() {
    let _ = ThreadPoolBuilder::new()
        .num_threads(cpu_thread_count())
        .build_global();
}

pub fn time_of_day_seed() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds_in_day = now.as_secs() % 86_400;
    (seconds_in_day << 32) ^ now.subsec_nanos() as u64
}

pub fn cpu_thread_count() -> usize {
    thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1)
}

pub fn half_cpu_thread_count() -> usize {
    (cpu_thread_count() / 2).max(1)
}

pub fn default_hald_dir() -> PathBuf {
    default_mini_film_cache_dir().join("hald")
}

pub fn default_mini_film_cache_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache")
        .join("mini-film")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_raw_extension_matching_is_case_insensitive() {
        assert!(is_supported_raw_file(Path::new("foo.ARW")));
        assert!(is_supported_raw_file(Path::new("foo.Cr2")));
        assert!(is_supported_raw_file(Path::new("foo.NEF")));
        assert!(is_supported_raw_file(Path::new("foo.raf")));
    }

    #[test]
    fn unsupported_extensions_are_rejected() {
        assert!(!is_supported_raw_file(Path::new("foo.jpg")));
        assert!(!is_supported_raw_file(Path::new("foo.txt")));
    }
}
