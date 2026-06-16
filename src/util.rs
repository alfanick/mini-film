use std::{
    collections::HashSet,
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

pub const SUPPORTED_COMPRESSED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "heic", "heif"];

#[derive(Clone, Copy, Debug, Default)]
pub enum InputFileFilter {
    #[default]
    All,
    JpgOnly,
    RawOnly,
}

pub fn is_supported_raw_file(path: &Path) -> bool {
    is_raw_input_file(path)
}

pub fn is_raw_input_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            SUPPORTED_RAW_EXTENSIONS
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
        })
}

pub fn is_jpeg_input_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            SUPPORTED_COMPRESSED_EXTENSIONS
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
        })
}

pub fn is_supported_input_file(path: &Path, filter: InputFileFilter) -> bool {
    match filter {
        InputFileFilter::All => is_raw_input_file(path) || is_jpeg_input_file(path),
        InputFileFilter::RawOnly => is_raw_input_file(path),
        InputFileFilter::JpgOnly => is_jpeg_input_file(path),
    }
}

pub fn coalesce_input_sidecars(mut inputs: Vec<PathBuf>, filter: InputFileFilter) -> Vec<PathBuf> {
    inputs.sort();
    inputs.dedup();
    if !matches!(filter, InputFileFilter::All) {
        return inputs;
    }
    inputs
        .into_iter()
        .filter(|path| !is_jpeg_input_file(path) || matching_raw_for_sidecar(path).is_none())
        .collect()
}

pub fn coalesce_due_input_sidecars(inputs: Vec<PathBuf>, filter: InputFileFilter) -> Vec<PathBuf> {
    if !matches!(filter, InputFileFilter::All) {
        return coalesce_input_sidecars(inputs, filter);
    }
    let mut seen = HashSet::new();
    let mut coalesced = inputs
        .into_iter()
        .filter_map(|path| {
            if is_jpeg_input_file(&path) {
                matching_raw_for_sidecar(&path).or(Some(path))
            } else {
                Some(path)
            }
        })
        .filter(|path| seen.insert(path.clone()))
        .collect::<Vec<_>>();
    coalesced.sort();
    coalesced
}

pub fn matching_sidecar_for_raw(raw: &Path) -> Option<PathBuf> {
    matching_sibling_with_kind(raw, is_jpeg_input_file)
}

pub fn matching_raw_for_sidecar(sidecar: &Path) -> Option<PathBuf> {
    matching_sibling_with_kind(sidecar, is_raw_input_file)
}

fn matching_sibling_with_kind(path: &Path, accept: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let mut matches = fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|candidate| accept(candidate))
        .filter(|candidate| {
            candidate
                .file_stem()
                .and_then(|candidate_stem| candidate_stem.to_str())
                .is_some_and(|candidate_stem| candidate_stem.eq_ignore_ascii_case(stem))
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches.into_iter().next()
}

#[allow(dead_code)]
pub(crate) fn input_filter_name(filter: InputFileFilter) -> &'static str {
    match filter {
        InputFileFilter::All => "RAW/JPEG/HEIC",
        InputFileFilter::JpgOnly => "JPG/JPEG/HEIC/HEIF",
        InputFileFilter::RawOnly => "RAW",
    }
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
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache")
        .join("mini-film")
        .join("hald")
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

    #[test]
    fn compressed_detection_for_jpg_like_extensions() {
        assert!(is_jpeg_input_file(Path::new("foo.jpg")));
        assert!(is_jpeg_input_file(Path::new("foo.JPEG")));
        assert!(is_jpeg_input_file(Path::new("foo.heic")));
        assert!(is_jpeg_input_file(Path::new("foo.HEIF")));
        assert!(!is_jpeg_input_file(Path::new("foo.nef")));
        assert!(!is_jpeg_input_file(Path::new("foo.txt")));
    }

    #[test]
    fn input_filter_controls_candidate_sources() {
        assert!(is_supported_input_file(
            Path::new("foo.NEF"),
            InputFileFilter::All
        ));
        assert!(is_supported_input_file(
            Path::new("foo.jpg"),
            InputFileFilter::All
        ));
        assert!(!is_supported_input_file(
            Path::new("foo.txt"),
            InputFileFilter::All
        ));

        assert!(is_supported_input_file(
            Path::new("foo.NEF"),
            InputFileFilter::RawOnly
        ));
        assert!(!is_supported_input_file(
            Path::new("foo.jpg"),
            InputFileFilter::RawOnly
        ));

        assert!(!is_supported_input_file(
            Path::new("foo.NEF"),
            InputFileFilter::JpgOnly
        ));
        assert!(is_supported_input_file(
            Path::new("foo.jpg"),
            InputFileFilter::JpgOnly
        ));
    }
}
