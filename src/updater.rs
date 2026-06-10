#[cfg(feature = "github-update")]
use self_update::cargo_crate_version;

#[cfg(feature = "github-update")]
use flate2::read::GzDecoder;
#[cfg(feature = "github-update")]
use std::collections::HashSet;
#[cfg(feature = "github-update")]
use std::fs;
#[cfg(feature = "github-update")]
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
#[cfg(feature = "github-update")]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

#[cfg(feature = "github-update")]
use self_update::Download;
#[cfg(feature = "github-update")]
use tar::Archive;
#[cfg(feature = "github-update")]
use walkdir::WalkDir;

#[cfg(feature = "github-update")]
const LENSFUN_ARCHIVE_URL: &str = "https://codeload.github.com/lensfun/lensfun/tar.gz/master";

#[cfg(feature = "github-update")]
const LENSFUN_DB_SUBDIR: &str = "data/db";

/// Run a timed GitHub release update check and install the latest compatible
/// binary when available.
///
/// Returns `"up-to-date"` when no newer release exists, or a status message on
/// update.
#[cfg(feature = "github-update")]
pub(crate) fn run_binary_update(timeout: Duration) -> anyhow::Result<String> {
    if !api_host_is_reachable("api.github.com", 443, timeout) {
        anyhow::bail!("github host unavailable: api.github.com:443");
    }

    let status = check_for_update()?;
    if let self_update::Status::UpToDate(version) = status {
        return Ok(format!("binary already up-to-date (v{version})"));
    }

    Ok(status.to_string())
}

#[cfg(not(feature = "github-update"))]
pub(crate) fn run_binary_update(_timeout: Duration) -> anyhow::Result<String> {
    anyhow::bail!(
        "mini-film binary updater disabled in this build (build with --features github-update)"
    )
}

#[cfg(feature = "github-update")]
fn check_for_update() -> anyhow::Result<self_update::Status> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("alfanick")
        .repo_name("mini-film")
        .bin_name("mini-film")
        .target(asset_target())
        .no_confirm(true)
        .show_download_progress(false)
        .current_version(cargo_crate_version!())
        .build()?
        .update()?;

    Ok(status)
}

#[cfg(feature = "github-update")]
fn api_host_is_reachable(host: &str, port: u16, timeout: Duration) -> bool {
    let Some(target) = (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| {
            addresses.find(|address| matches!(address, SocketAddr::V4(_) | SocketAddr::V6(_)))
        })
    else {
        return false;
    };

    TcpStream::connect_timeout(&target, timeout).is_ok()
}

#[cfg(feature = "github-update")]
fn asset_target() -> &'static str {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else {
        self_update::get_target()
    }
}

#[cfg(feature = "github-update")]
pub(crate) fn run_lensfun_update() -> anyhow::Result<LensfunUpdateReport> {
    let download_dir = tempfile::tempdir()?;
    let archive_path = download_dir.path().join("lensfun.tar.gz");
    let mut archive_file = fs::File::create(&archive_path)?;
    Download::from_url(LENSFUN_ARCHIVE_URL).download_to(&mut archive_file)?;

    let mut archive = Archive::new(GzDecoder::new(fs::File::open(&archive_path)?));
    let extracted = download_dir.path().join("source");
    archive.unpack(&extracted)?;

    let source_db = locate_source_db_dir(&extracted)?;
    let target_cache = lensfun_cache_dir();
    let copied = copy_directory_contents(&source_db, &target_cache)?;

    let synced = mirror_to_default_lensfun_data_root(&target_cache)?;
    Ok(LensfunUpdateReport {
        cache_dir: target_cache,
        synced_dir: synced,
        copied_files: copied,
    })
}

#[cfg(not(feature = "github-update"))]
pub(crate) fn run_lensfun_update() -> anyhow::Result<LensfunUpdateReport> {
    Err(anyhow::anyhow!(
        "lensfun update is disabled in this build (build with --features github-update)"
    ))
}

#[cfg(feature = "github-update")]
fn locate_source_db_dir(extracted_root: &Path) -> anyhow::Result<PathBuf> {
    let top = extracted_root
        .read_dir()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("lensfun archive missing extracted root entries"))?
        .map_err(|err| anyhow::anyhow!(err))?
        .path();

    let source = top.join(LENSFUN_DB_SUBDIR);
    if !source.is_dir() {
        anyhow::bail!("lensfun archive does not contain {LENSFUN_DB_SUBDIR}");
    }
    Ok(source)
}

#[cfg(feature = "github-update")]
fn copy_directory_contents(source: &Path, destination: &Path) -> anyhow::Result<usize> {
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    fs::create_dir_all(destination)?;

    let mut copied = 0usize;
    for entry in WalkDir::new(source).into_iter().filter_map(Result::ok) {
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), &target)?;
        copied = copied.saturating_add(1);
    }

    Ok(copied)
}

#[cfg(feature = "github-update")]
pub(crate) fn lensfun_cache_dir() -> PathBuf {
    home_dir().join(".cache").join("mini-film").join("lensfun")
}

#[cfg(feature = "github-update")]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(feature = "github-update")]
fn mirror_to_default_lensfun_data_root(cache: &Path) -> anyhow::Result<PathBuf> {
    let data_root = lensfun_default_data_root();
    let mut copied_dirs = HashSet::new();

    if data_root.exists() {
        if data_root == cache {
            return Ok(data_root);
        }

        fs::remove_dir_all(&data_root)?;
    }

    fs::create_dir_all(data_root.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "lensfun data root has no parent directory: {}",
            data_root.display()
        )
    })?)?;

    for entry in WalkDir::new(cache).into_iter().filter_map(Result::ok) {
        let rel = entry.path().strip_prefix(cache)?;
        let target = data_root.join(rel);

        if entry.file_type().is_dir() {
            copied_dirs.insert(
                target
                    .parent()
                    .unwrap_or_else(|| data_root.as_path())
                    .to_path_buf(),
            );
            fs::create_dir_all(&target)?;
            continue;
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(entry.path(), &target)?;
    }

    if copied_dirs.is_empty() {
        fs::create_dir_all(&data_root)?;
    }

    Ok(data_root)
}

#[cfg(feature = "github-update")]
fn lensfun_default_data_root() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".local/share"))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("lensfun")
    }

    #[cfg(target_os = "macos")]
    {
        home_dir()
            .join("Library")
            .join("Application Support")
            .join("lensfun")
    }

    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(PathBuf::from)
            .or_else(|| Some(home_dir().join("AppData").join("Local")))
            .unwrap();
        base.join("lensfun")
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        home_dir().join(".local").join("share").join("lensfun")
    }
}

#[derive(Debug)]
pub(crate) struct LensfunUpdateReport {
    pub(crate) cache_dir: PathBuf,
    pub(crate) synced_dir: PathBuf,
    pub(crate) copied_files: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "github-update")]
    #[test]
    fn lensfun_default_data_root_is_deterministic() {
        let _ = lensfun_default_data_root();
    }

    #[test]
    fn lensfun_report_fields_are_readable() {
        let report = LensfunUpdateReport {
            cache_dir: PathBuf::from("cache"),
            synced_dir: PathBuf::from("synced"),
            copied_files: 7,
        };

        assert_eq!(report.copied_files, 7);
        assert_eq!(report.cache_dir, PathBuf::from("cache"));
        assert_eq!(report.synced_dir, PathBuf::from("synced"));
    }
}
