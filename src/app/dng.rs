use std::{
    collections::HashSet,
    env, fs,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result, anyhow, bail};
use filetime::{FileTime, set_file_times};
use serde::{Deserialize, Deserializer};
use tempfile::Builder;

use crate::app::util::is_raw_input_file;

const ADOBE_DNG_CONVERTER_RELATIVE_PATHS: &[&str] = &[
    "drive_c/Program Files/Adobe/Adobe DNG Converter/Adobe DNG Converter.exe",
    "drive_c/Program Files (x86)/Adobe/Adobe DNG Converter/Adobe DNG Converter.exe",
];
const ADOBE_STANDARD_DCP_RELATIVE_PATH: &str =
    "drive_c/ProgramData/Adobe/CameraRaw/CameraProfiles/Adobe Standard";
const ADOBE_LCP_RELATIVE_PATH: &str = "drive_c/ProgramData/Adobe/CameraRaw/LensProfiles/1.0";
static GENERATED_DNGS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

#[derive(Clone, Debug)]
pub(crate) struct DngFallbackConfig {
    converter: Option<PathBuf>,
    wine: Option<PathBuf>,
    wine_prefix: Option<PathBuf>,
    exiftool: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreparedRawSource {
    requested: PathBuf,
    active: PathBuf,
    replaced: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawSourceReplacement {
    pub(crate) old_path: PathBuf,
    pub(crate) new_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct DngMetadata {
    #[serde(rename = "FileType")]
    file_type: Option<String>,
    #[serde(rename = "DNGVersion")]
    dng_version: Option<serde_json::Value>,
    #[serde(rename = "Compression")]
    compression: Option<u16>,
    #[serde(rename = "BitsPerSample")]
    bits_per_sample: Option<u16>,
    #[serde(rename = "NewRawImageDigest")]
    new_raw_image_digest: Option<String>,
    #[serde(rename = "OriginalRawFileName")]
    original_raw_file_name: Option<String>,
    #[serde(rename = "ImageWidth")]
    image_width: Option<u32>,
    #[serde(rename = "ImageHeight")]
    image_height: Option<u32>,
    #[serde(rename = "Make")]
    make: Option<String>,
    #[serde(rename = "Model")]
    model: Option<String>,
    #[serde(rename = "DateTimeOriginal")]
    date_time_original: Option<String>,
    #[serde(
        rename = "SubSecTimeOriginal",
        default,
        deserialize_with = "deserialize_optional_subsecond"
    )]
    subsec_time_original: Option<String>,
    #[serde(rename = "ShutterCount")]
    shutter_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct NikonCompressionMetadata {
    #[serde(rename = "NEFCompression")]
    nef_compression: Option<u16>,
}

fn deserialize_optional_subsecond<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(serde_json::Value::String(value))
            if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            Ok(Some(value))
        }
        Some(serde_json::Value::Number(value)) if value.as_u64().is_some() => {
            Ok(Some(value.to_string()))
        }
        Some(value) => Err(serde::de::Error::custom(format!(
            "expected capture subsecond as digits or a non-negative integer, got {value}"
        ))),
    }
}

impl Default for DngFallbackConfig {
    fn default() -> Self {
        Self::new(None, None, None)
    }
}

impl DngFallbackConfig {
    pub(crate) fn new(
        converter: Option<PathBuf>,
        wine: Option<PathBuf>,
        wine_prefix: Option<PathBuf>,
    ) -> Self {
        Self {
            converter,
            wine,
            wine_prefix,
            exiftool: PathBuf::from("exiftool"),
        }
    }

    pub(crate) fn adobe_standard_dcp_root(&self) -> Option<PathBuf> {
        let explicit_prefix = self
            .wine_prefix
            .clone()
            .or_else(|| nonempty_env_path("MINI_FILM_WINE_PREFIX"))
            .or_else(|| nonempty_env_path("WINEPREFIX"));
        if let Some(root) = explicit_prefix
            .as_deref()
            .and_then(adobe_standard_dcp_root_in_prefix)
        {
            return Some(root);
        }

        let explicit_converter = self
            .converter
            .clone()
            .or_else(|| nonempty_env_path("MINI_FILM_ADOBE_DNG_CONVERTER"));
        if let Some(root) = explicit_converter
            .as_deref()
            .and_then(wine_prefix_for_converter)
            .as_deref()
            .and_then(adobe_standard_dcp_root_in_prefix)
        {
            return Some(root);
        }

        default_wine_prefixes()
            .into_iter()
            .find_map(|prefix| adobe_standard_dcp_root_in_prefix(&prefix))
    }

    pub(crate) fn adobe_lcp_root(&self) -> Option<PathBuf> {
        if let Some(root) = nonempty_env_path("MINI_FILM_LCP_ROOT").filter(|root| root.is_dir()) {
            return Some(root);
        }

        let explicit_prefix = self
            .wine_prefix
            .clone()
            .or_else(|| nonempty_env_path("MINI_FILM_WINE_PREFIX"))
            .or_else(|| nonempty_env_path("WINEPREFIX"));
        if let Some(root) = explicit_prefix
            .as_deref()
            .and_then(adobe_lcp_root_in_prefix)
        {
            return Some(root);
        }

        let explicit_converter = self
            .converter
            .clone()
            .or_else(|| nonempty_env_path("MINI_FILM_ADOBE_DNG_CONVERTER"));
        if let Some(root) = explicit_converter
            .as_deref()
            .and_then(wine_prefix_for_converter)
            .as_deref()
            .and_then(adobe_lcp_root_in_prefix)
        {
            return Some(root);
        }

        default_wine_prefixes()
            .into_iter()
            .find_map(|prefix| adobe_lcp_root_in_prefix(&prefix))
    }

    #[cfg(test)]
    pub(crate) fn with_exiftool(mut self, exiftool: PathBuf) -> Self {
        self.exiftool = exiftool;
        self
    }

    pub(crate) fn prepare_known(&self, source: &Path) -> Result<PreparedRawSource> {
        if !is_raw_input_file(source) || is_dng(source) {
            return Ok(PreparedRawSource::unchanged(source));
        }

        if let Some(successor) = self.existing_successor(source)? {
            return Ok(successor);
        }

        if !source.is_file()
            || !is_nikon_raw(source)
            || !is_nikon_high_efficiency(&self.exiftool, source)?
        {
            return Ok(PreparedRawSource::unchanged(source));
        }

        self.convert(source)
    }

    pub(crate) fn existing_successor(&self, source: &Path) -> Result<Option<PreparedRawSource>> {
        validated_existing_successor(&self.exiftool, source)
            .map(|successor| successor.map(|path| PreparedRawSource::converted(source, path)))
    }

    pub(crate) fn generated_this_process(path: &Path) -> bool {
        GENERATED_DNGS
            .get()
            .and_then(|paths| paths.lock().ok())
            .is_some_and(|paths| paths.contains(path))
    }

    pub(crate) fn append_cli_args(&self, command: &mut Command) {
        if let Some(converter) = &self.converter {
            command.arg("--adobe-dng-converter").arg(converter);
        }
        if let Some(wine) = &self.wine {
            command.arg("--wine").arg(wine);
        }
        if let Some(wine_prefix) = &self.wine_prefix {
            command.arg("--wine-prefix").arg(wine_prefix);
        }
    }

    pub(crate) fn coalesce_existing_replacements(
        &self,
        inputs: Vec<PathBuf>,
    ) -> Result<Vec<PathBuf>> {
        let input_set = inputs.iter().cloned().collect::<HashSet<_>>();
        let mut replacement_dngs = HashSet::new();
        for source in &inputs {
            if is_raw_input_file(source)
                && !is_dng(source)
                && let Some(successor) = self.existing_successor(source)?
                && input_set.contains(successor.active())
            {
                replacement_dngs.insert(successor.active().to_path_buf());
            }
        }
        Ok(inputs
            .into_iter()
            .filter(|input| !replacement_dngs.contains(input))
            .collect())
    }

    pub(crate) fn prepare_after_decode_failure(
        &self,
        source: &PreparedRawSource,
    ) -> Result<PreparedRawSource> {
        if source.was_replaced() || is_dng(source.active()) {
            return Ok(source.clone());
        }
        self.convert(source.requested())
    }

    pub(crate) fn finish_successful_development(&self, source: &PreparedRawSource) -> Result<()> {
        let Some(original) = source.replaced.as_deref() else {
            return Ok(());
        };
        if !original.exists() {
            return Ok(());
        }
        validate_dng(&self.exiftool, source.active(), original)?;
        fs::remove_file(original)
            .with_context(|| format!("removing replaced RAW {}", original.display()))?;
        sync_parent_directory(original)?;
        eprintln!(
            "Adobe DNG fallback: replaced {} with {}",
            original.display(),
            source.active().display()
        );
        Ok(())
    }

    fn convert(&self, source: &Path) -> Result<PreparedRawSource> {
        if !source.is_file() {
            if let Some(successor) = validated_existing_successor(&self.exiftool, source)? {
                return Ok(PreparedRawSource::converted(source, successor));
            }
            bail!("RAW source does not exist: {}", source.display());
        }

        let target = dng_successor_path(source)?;
        let _lock = acquire_conversion_lock(&target)?;
        if target.is_file() {
            validate_dng(&self.exiftool, &target, source)?;
            return Ok(PreparedRawSource::converted(source, target));
        }

        let toolchain = self.resolve_toolchain().with_context(|| {
            format!(
                "Adobe DNG fallback is required for {}; install Adobe DNG Converter in Wine or configure --adobe-dng-converter, --wine, and --wine-prefix",
                source.display()
            )
        })?;
        eprintln!(
            "Adobe DNG fallback: converting {} with {}",
            source.display(),
            toolchain.converter.display()
        );
        let parent = source
            .parent()
            .ok_or_else(|| anyhow!("RAW source has no parent: {}", source.display()))?;
        let staging = Builder::new()
            .prefix(".mini-film-dng-")
            .tempdir_in(parent)
            .with_context(|| format!("creating DNG staging directory in {}", parent.display()))?;
        let windows_staging = wine_z_path(staging.path())?;
        let windows_source = wine_z_path(source)?;
        let output = Command::new(&toolchain.wine)
            .env("WINEPREFIX", &toolchain.wine_prefix)
            .env("WINEDEBUG", "-all")
            .arg(&toolchain.converter)
            .args(["-c", "-p0", "-d"])
            .arg(windows_staging)
            .arg(windows_source)
            .output()
            .with_context(|| {
                format!(
                    "running Adobe DNG Converter through {}",
                    toolchain.wine.display()
                )
            })?;
        if !output.status.success() {
            bail!(
                "Adobe DNG Converter failed for {} with status {}\nstdout:\n{}\nstderr:\n{}",
                source.display(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let staged = staging.path().join(
            target
                .file_name()
                .ok_or_else(|| anyhow!("DNG target has no file name: {}", target.display()))?,
        );
        preserve_source_filesystem_metadata(source, &staged)?;
        validate_dng(&self.exiftool, &staged, source)?;
        File::open(&staged)
            .and_then(|file| file.sync_all())
            .with_context(|| format!("syncing staged DNG {}", staged.display()))?;
        fs::rename(&staged, &target)
            .with_context(|| format!("publishing converted DNG {}", target.display()))?;
        GENERATED_DNGS
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .map_err(|_| anyhow!("DNG generation registry lock is poisoned"))?
            .insert(target.clone());
        sync_parent_directory(&target)?;
        Ok(PreparedRawSource::converted(source, target))
    }

    fn resolve_toolchain(&self) -> Result<DngToolchain> {
        let explicit_prefix = self
            .wine_prefix
            .clone()
            .or_else(|| nonempty_env_path("MINI_FILM_WINE_PREFIX"))
            .or_else(|| nonempty_env_path("WINEPREFIX"));
        let explicit_converter = self
            .converter
            .clone()
            .or_else(|| nonempty_env_path("MINI_FILM_ADOBE_DNG_CONVERTER"));
        let converter = match explicit_converter {
            Some(path) if path.is_file() => path,
            Some(path) => bail!("Adobe DNG Converter not found: {}", path.display()),
            None => discover_converter(explicit_prefix.as_deref())
                .ok_or_else(|| anyhow!("Adobe DNG Converter was not found in a Wine prefix"))?,
        };
        let wine_prefix = explicit_prefix
            .or_else(|| wine_prefix_for_converter(&converter))
            .ok_or_else(|| {
                anyhow!(
                    "cannot determine Wine prefix for Adobe DNG Converter {}",
                    converter.display()
                )
            })?;
        let explicit_wine = self
            .wine
            .clone()
            .or_else(|| nonempty_env_path("MINI_FILM_WINE"));
        let wine = match explicit_wine {
            Some(path) if executable_exists(&path) => path,
            Some(path) => bail!("Wine executable not found: {}", path.display()),
            None => find_in_path("wine-stable")
                .or_else(|| find_in_path("wine"))
                .ok_or_else(|| anyhow!("Wine executable was not found in PATH"))?,
        };

        Ok(DngToolchain {
            converter,
            wine,
            wine_prefix,
        })
    }
}

impl PreparedRawSource {
    pub(crate) fn unchanged(source: &Path) -> Self {
        Self {
            requested: source.to_path_buf(),
            active: source.to_path_buf(),
            replaced: None,
        }
    }

    fn converted(source: &Path, dng: PathBuf) -> Self {
        Self {
            requested: source.to_path_buf(),
            active: dng,
            replaced: Some(source.to_path_buf()),
        }
    }

    pub(crate) fn requested(&self) -> &Path {
        &self.requested
    }

    pub(crate) fn active(&self) -> &Path {
        &self.active
    }

    pub(crate) fn was_replaced(&self) -> bool {
        self.replaced.is_some()
    }

    pub(crate) fn replacement(&self) -> Option<RawSourceReplacement> {
        self.replaced.as_ref().map(|old_path| RawSourceReplacement {
            old_path: old_path.clone(),
            new_path: self.active.clone(),
        })
    }
}

#[derive(Clone, Debug)]
struct DngToolchain {
    converter: PathBuf,
    wine: PathBuf,
    wine_prefix: PathBuf,
}

fn is_nikon_high_efficiency(exiftool: &Path, source: &Path) -> Result<bool> {
    let output = Command::new(exiftool)
        .args(["-j", "-n", "-NEFCompression"])
        .arg(source)
        .output()
        .with_context(|| {
            format!(
                "reading Nikon RAW compression metadata from {}",
                source.display()
            )
        })?;
    if !output.status.success() {
        return Ok(false);
    }
    let rows: Vec<NikonCompressionMetadata> =
        serde_json::from_slice(&output.stdout).with_context(|| {
            format!(
                "parsing Nikon RAW compression metadata from {}",
                source.display()
            )
        })?;
    Ok(rows
        .first()
        .and_then(|row| row.nef_compression)
        .is_some_and(|compression| matches!(compression, 13 | 14)))
}

fn validate_dng(exiftool: &Path, dng: &Path, source: &Path) -> Result<()> {
    let metadata =
        fs::metadata(dng).with_context(|| format!("reading converted DNG {}", dng.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        bail!(
            "Adobe DNG Converter produced an empty file: {}",
            dng.display()
        );
    }
    let row = read_conversion_identity(exiftool, dng)?;
    if row.file_type.as_deref() != Some("DNG") || row.dng_version.is_none() {
        bail!("converted file is not a DNG: {}", dng.display());
    }
    if row.compression != Some(7)
        || row.bits_per_sample.unwrap_or_default() == 0
        || row
            .new_raw_image_digest
            .as_deref()
            .is_none_or(str::is_empty)
    {
        bail!(
            "converted DNG is not a lossless, checksummed RAW: {}",
            dng.display()
        );
    }
    if row.image_width.unwrap_or_default() == 0 || row.image_height.unwrap_or_default() == 0 {
        bail!("converted DNG has invalid dimensions: {}", dng.display());
    }
    let expected_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("RAW source name is not valid UTF-8: {}", source.display()))?;
    if !row
        .original_raw_file_name
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case(expected_name))
    {
        bail!(
            "DNG {} does not identify {} as its original RAW",
            dng.display(),
            source.display()
        );
    }
    if source.is_file() {
        validate_source_identity(exiftool, source, dng, &row)?;
    }
    Ok(())
}

fn read_conversion_identity(exiftool: &Path, path: &Path) -> Result<DngMetadata> {
    let output = Command::new(exiftool)
        .args([
            "-j",
            "-n",
            "-FileType",
            "-DNGVersion",
            "-Compression",
            "-BitsPerSample",
            "-NewRawImageDigest",
            "-OriginalRawFileName",
            "-ImageWidth",
            "-ImageHeight",
            "-Make",
            "-Model",
            "-DateTimeOriginal",
            "-SubSecTimeOriginal",
            "-ShutterCount",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("reading conversion identity from {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "ExifTool could not read conversion identity from {} with status {}",
            path.display(),
            output.status
        );
    }
    let rows: Vec<DngMetadata> = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("parsing conversion identity from {}", path.display()))?;
    rows.into_iter()
        .next()
        .ok_or_else(|| anyhow!("file has no conversion metadata: {}", path.display()))
}

fn validate_source_identity(
    exiftool: &Path,
    source: &Path,
    dng: &Path,
    dng_identity: &DngMetadata,
) -> Result<()> {
    let source_metadata = fs::metadata(source)
        .with_context(|| format!("reading metadata for {}", source.display()))?;
    let dng_metadata =
        fs::metadata(dng).with_context(|| format!("reading metadata for {}", dng.display()))?;
    let source_modified = FileTime::from_last_modification_time(&source_metadata);
    let dng_modified = FileTime::from_last_modification_time(&dng_metadata);
    if source_modified != dng_modified {
        bail!(
            "DNG {} does not match the modification time of {}; refusing to replace a possible different RAW",
            dng.display(),
            source.display()
        );
    }

    let source_identity = read_conversion_identity(exiftool, source)?;
    ensure_identity_match(
        "image width",
        source_identity.image_width,
        dng_identity.image_width,
        source,
        dng,
    )?;
    ensure_identity_match(
        "image height",
        source_identity.image_height,
        dng_identity.image_height,
        source,
        dng,
    )?;
    ensure_identity_match(
        "camera make",
        source_identity.make.as_deref(),
        dng_identity.make.as_deref(),
        source,
        dng,
    )?;
    ensure_identity_match(
        "camera model",
        source_identity.model.as_deref(),
        dng_identity.model.as_deref(),
        source,
        dng,
    )?;
    ensure_identity_match(
        "capture time",
        source_identity.date_time_original.as_deref(),
        dng_identity.date_time_original.as_deref(),
        source,
        dng,
    )?;
    ensure_identity_match(
        "capture subsecond",
        source_identity.subsec_time_original.as_deref(),
        dng_identity.subsec_time_original.as_deref(),
        source,
        dng,
    )?;
    ensure_identity_match(
        "shutter count",
        source_identity.shutter_count,
        dng_identity.shutter_count,
        source,
        dng,
    )?;
    Ok(())
}

fn ensure_identity_match<T: PartialEq>(
    field: &str,
    source_value: Option<T>,
    dng_value: Option<T>,
    source: &Path,
    dng: &Path,
) -> Result<()> {
    if let Some(source_value) = source_value
        && dng_value.as_ref() != Some(&source_value)
    {
        bail!(
            "DNG {} does not preserve {} from {}; refusing to replace the RAW",
            dng.display(),
            field,
            source.display()
        );
    }
    Ok(())
}

fn validated_existing_successor(exiftool: &Path, source: &Path) -> Result<Option<PathBuf>> {
    let target = dng_successor_path(source)?;
    if !target.is_file() {
        return Ok(None);
    }
    validate_dng(exiftool, &target, source)?;
    Ok(Some(target))
}

fn dng_successor_path(source: &Path) -> Result<PathBuf> {
    if source.file_stem().is_none() {
        bail!("RAW source has no file stem: {}", source.display());
    }
    Ok(source.with_extension("dng"))
}

fn is_dng(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dng"))
}

fn is_nikon_raw(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("nef") || extension.eq_ignore_ascii_case("nrw")
        })
}

fn conversion_lock_path(target: &Path) -> Result<PathBuf> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("DNG target name is not valid UTF-8: {}", target.display()))?;
    Ok(target.with_file_name(format!(".{file_name}.mini-film-dng.lock")))
}

fn acquire_conversion_lock(target: &Path) -> Result<File> {
    let lock_path = conversion_lock_path(target)?;
    // Keep the rendezvous path in place permanently. Removing a locked path can
    // let another worker open a different inode and bypass the active lock.
    // The kernel releases this advisory lock when the descriptor closes,
    // including when a conversion process is interrupted or exits unexpectedly.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("opening DNG conversion lock {}", lock_path.display()))?;
    loop {
        match file.lock() {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("locking DNG conversion target {}", target.display())
                });
            }
        }
    }
    Ok(file)
}

fn preserve_source_filesystem_metadata(source: &Path, output: &Path) -> Result<()> {
    let metadata = fs::metadata(source)
        .with_context(|| format!("reading metadata for {}", source.display()))?;
    fs::set_permissions(output, metadata.permissions())
        .with_context(|| format!("preserving permissions on {}", output.display()))?;
    let accessed = FileTime::from_last_access_time(&metadata);
    let modified = FileTime::from_last_modification_time(&metadata);
    set_file_times(output, accessed, modified)
        .with_context(|| format!("preserving timestamps on {}", output.display()))
}

fn sync_parent_directory(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", path.display()))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("syncing directory {}", parent.display()))
}

fn wine_z_path(path: &Path) -> Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .context("reading current directory for Wine path")?
            .join(path)
    };
    let path = absolute
        .to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8 for Wine: {}", absolute.display()))?;
    Ok(format!("Z:{}", path.replace('/', "\\")))
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn discover_converter(explicit_prefix: Option<&Path>) -> Option<PathBuf> {
    if let Some(converter) = explicit_prefix.and_then(converter_in_prefix) {
        return Some(converter);
    }
    let prefixes = default_wine_prefixes();
    prefixes
        .into_iter()
        .find_map(|prefix| converter_in_prefix(&prefix))
}

fn default_wine_prefixes() -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    if let Some(home) = nonempty_env_path("HOME") {
        prefixes.push(home.join(".wine-dng-mini-film"));
        prefixes.push(home.join(".wine"));
        if let Ok(entries) = fs::read_dir(&home) {
            let mut discovered = entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(".wine"))
                })
                .collect::<Vec<_>>();
            discovered.sort();
            prefixes.extend(discovered);
        }
    }
    let mut deduplicated = Vec::with_capacity(prefixes.len());
    for prefix in prefixes {
        if !deduplicated.contains(&prefix) {
            deduplicated.push(prefix);
        }
    }
    deduplicated
}

fn converter_in_prefix(prefix: &Path) -> Option<PathBuf> {
    ADOBE_DNG_CONVERTER_RELATIVE_PATHS
        .iter()
        .map(|relative| prefix.join(relative))
        .find(|candidate| candidate.is_file())
}

fn adobe_standard_dcp_root_in_prefix(prefix: &Path) -> Option<PathBuf> {
    let root = prefix.join(ADOBE_STANDARD_DCP_RELATIVE_PATH);
    root.is_dir().then_some(root)
}

fn adobe_lcp_root_in_prefix(prefix: &Path) -> Option<PathBuf> {
    let root = prefix.join(ADOBE_LCP_RELATIVE_PATH);
    root.is_dir().then_some(root)
}

fn wine_prefix_for_converter(converter: &Path) -> Option<PathBuf> {
    converter.ancestors().find_map(|ancestor| {
        (ancestor.file_name().and_then(|name| name.to_str()) == Some("drive_c"))
            .then(|| ancestor.parent().map(Path::to_path_buf))
            .flatten()
    })
}

fn executable_exists(path: &Path) -> bool {
    if path.components().count() > 1 {
        path.is_file()
    } else {
        find_in_path(path.to_string_lossy().as_ref()).is_some()
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::set_file_mtime;
    use std::{
        io::Write as _,
        os::unix::fs::PermissionsExt as _,
        process::Stdio,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    const LOCK_HOLDER_TARGET_ENV: &str = "MINI_FILM_DNG_LOCK_HOLDER_TARGET";
    const LOCK_HOLDER_READY_ENV: &str = "MINI_FILM_DNG_LOCK_HOLDER_READY";
    const LOCK_HOLDER_TEST_NAME: &str = "app::dng::tests::conversion_lock_holder_process";

    fn write_executable(path: &Path, text: &str) {
        let mut file = File::create(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
        file.sync_all().unwrap();
        let mut permissions = file.metadata().unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn conversion_lock_serializes_workers_and_keeps_rendezvous_path() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("frame.dng");
        let lock_path = conversion_lock_path(&target).unwrap();
        fs::write(&lock_path, b"orphaned legacy marker").unwrap();

        let first = acquire_conversion_lock(&target).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let contender_target = target.clone();
        let contender = thread::spawn(move || {
            started_tx.send(()).unwrap();
            let lock = acquire_conversion_lock(&contender_target).unwrap();
            acquired_tx.send(()).unwrap();
            lock
        });

        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(
            acquired_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(first);
        acquired_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(contender.join().unwrap());

        assert!(lock_path.is_file());
        drop(acquire_conversion_lock(&target).unwrap());
        assert!(lock_path.is_file());
    }

    #[test]
    fn conversion_lock_holder_process() {
        let Some(target) = env::var_os(LOCK_HOLDER_TARGET_ENV) else {
            return;
        };
        let ready = env::var_os(LOCK_HOLDER_READY_ENV).expect("lock holder ready path");
        let _lock = acquire_conversion_lock(Path::new(&target)).unwrap();
        fs::write(ready, b"ready").unwrap();
        loop {
            thread::sleep(Duration::from_secs(60));
        }
    }

    #[test]
    fn conversion_lock_is_released_when_holder_process_dies() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("frame.dng");
        let ready = temp.path().join("holder-ready");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg(LOCK_HOLDER_TEST_NAME)
            .arg("--exact")
            .arg("--nocapture")
            .env(LOCK_HOLDER_TARGET_ENV, &target)
            .env(LOCK_HOLDER_READY_ENV, &ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.is_file() {
            assert!(
                child.try_wait().unwrap().is_none(),
                "lock holder exited before acquiring the lock"
            );
            assert!(
                Instant::now() < deadline,
                "lock holder did not acquire the lock in time"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(conversion_lock_path(&target).unwrap())
            .unwrap();
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));

        child.kill().unwrap();
        child.wait().unwrap();
        drop(acquire_conversion_lock(&target).unwrap());
        assert!(conversion_lock_path(&target).unwrap().is_file());
    }

    #[test]
    fn wine_path_uses_the_default_z_drive() {
        assert_eq!(
            wine_z_path(Path::new("/tmp/folder with spaces/frame.nef")).unwrap(),
            r"Z:\tmp\folder with spaces\frame.nef"
        );
    }

    #[test]
    fn recognizes_adobe_lens_profiles_beside_converter_installation() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("wine-prefix");
        let root = prefix.join(ADOBE_LCP_RELATIVE_PATH);
        fs::create_dir_all(&root).unwrap();

        assert_eq!(
            adobe_lcp_root_in_prefix(&prefix).as_deref(),
            Some(root.as_path())
        );
    }

    #[test]
    fn conversion_identity_accepts_numeric_and_zero_padded_subseconds() {
        let numeric: DngMetadata = serde_json::from_str(r#"{"SubSecTimeOriginal":36}"#).unwrap();
        assert_eq!(numeric.subsec_time_original.as_deref(), Some("36"));

        let text: DngMetadata = serde_json::from_str(r#"{"SubSecTimeOriginal":"36"}"#).unwrap();
        assert_eq!(text.subsec_time_original, numeric.subsec_time_original);

        let zero_padded: DngMetadata =
            serde_json::from_str(r#"{"SubSecTimeOriginal":"06"}"#).unwrap();
        assert_eq!(zero_padded.subsec_time_original.as_deref(), Some("06"));

        let numeric_six: DngMetadata = serde_json::from_str(r#"{"SubSecTimeOriginal":6}"#).unwrap();
        assert_ne!(
            zero_padded.subsec_time_original,
            numeric_six.subsec_time_original
        );

        let missing: DngMetadata = serde_json::from_str("{}").unwrap();
        let null: DngMetadata = serde_json::from_str(r#"{"SubSecTimeOriginal":null}"#).unwrap();
        assert_eq!(missing.subsec_time_original, None);
        assert_eq!(null.subsec_time_original, None);
    }

    #[test]
    fn conversion_identity_rejects_invalid_subseconds() {
        for value in ["\"\"", r#""6a""#, "-1", "6.5", "true", r#"["06"]"#, "{}"] {
            let json = format!(r#"{{"SubSecTimeOriginal":{value}}}"#);
            let error = serde_json::from_str::<DngMetadata>(&json).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("expected capture subsecond as digits or a non-negative integer"),
                "unexpected error for {value}: {error}"
            );
        }
    }

    #[test]
    fn prepared_source_reports_replacement() {
        let prepared =
            PreparedRawSource::converted(Path::new("/in/frame.nef"), "/in/frame.dng".into());
        assert_eq!(prepared.requested(), Path::new("/in/frame.nef"));
        assert_eq!(prepared.active(), Path::new("/in/frame.dng"));
        assert_eq!(
            prepared.replacement(),
            Some(RawSourceReplacement {
                old_path: PathBuf::from("/in/frame.nef"),
                new_path: PathBuf::from("/in/frame.dng"),
            })
        );
    }

    #[test]
    fn rendered_input_does_not_claim_same_stem_dng() {
        let temp = tempfile::tempdir().unwrap();
        let jpeg = temp.path().join("frame.jpg");
        let dng = temp.path().join("frame.dng");
        fs::write(&jpeg, b"jpeg").unwrap();
        fs::write(&dng, b"unrelated raw").unwrap();

        let prepared = DngFallbackConfig::default().prepare_known(&jpeg).unwrap();
        assert_eq!(prepared.active(), jpeg);
        assert!(!prepared.was_replaced());
    }

    #[test]
    fn lossy_dng_is_rejected_before_replacing_raw() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("frame.nef");
        let dng = temp.path().join("frame.dng");
        fs::write(&source, b"raw").unwrap();
        fs::write(&dng, b"lossy dng").unwrap();
        let exiftool = temp.path().join("exiftool");
        write_executable(
            &exiftool,
            r#"#!/bin/sh
printf '%s\n' '[{"FileType":"DNG","DNGVersion":"1.7.1.0","Compression":8,"BitsPerSample":8,"NewRawImageDigest":"0123456789abcdef0123456789abcdef","OriginalRawFileName":"frame.nef","ImageWidth":8256,"ImageHeight":5504}]'
"#,
        );

        let error = validate_dng(&exiftool, &dng, &source).unwrap_err();
        assert!(error.to_string().contains("not a lossless"));
        assert!(source.is_file());
    }

    #[test]
    fn high_efficiency_nef_is_validated_before_original_is_removed() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("frame.nef");
        fs::write(&source, b"high efficiency raw").unwrap();
        let prefix = temp.path().join("wine-prefix");
        fs::create_dir_all(&prefix).unwrap();
        let converter = temp.path().join("Adobe DNG Converter.exe");
        fs::write(&converter, b"converter").unwrap();
        let exiftool = temp.path().join("exiftool");
        write_executable(
            &exiftool,
            r#"#!/bin/sh
case "$*" in
  *-NEFCompression*)
    printf '%s\n' '[{"NEFCompression":14}]'
    ;;
  *)
    printf '%s\n' '[{"FileType":"DNG","DNGVersion":"1.7.1.0","Compression":7,"BitsPerSample":16,"NewRawImageDigest":"0123456789abcdef0123456789abcdef","OriginalRawFileName":"frame.nef","ImageWidth":8256,"ImageHeight":5504,"SubSecTimeOriginal":"06"}]'
    ;;
esac
"#,
        );
        let wine = temp.path().join("wine");
        write_executable(
            &wine,
            r#"#!/bin/sh
set -eu
destination=$(printf '%s' "$5" | sed 's#^Z:##; s#\\#/#g')
source=$(printf '%s' "$6" | sed 's#^Z:##; s#\\#/#g')
name=$(basename "$source")
stem=${name%.*}
mkdir -p "$destination"
printf '%s\n' 'fake validated dng' > "$destination/$stem.dng"
"#,
        );
        let fallback = DngFallbackConfig::new(Some(converter), Some(wine), Some(prefix))
            .with_exiftool(exiftool);

        let prepared = fallback.prepare_known(&source).unwrap();
        assert_eq!(prepared.active(), temp.path().join("frame.dng"));
        assert!(prepared.active().is_file());
        assert!(source.is_file());
        assert!(DngFallbackConfig::generated_this_process(prepared.active()));

        let source_mtime = FileTime::from_last_modification_time(&fs::metadata(&source).unwrap());
        set_file_mtime(&source, FileTime::from_unix_time(1, 0)).unwrap();
        let error = fallback.prepare_known(&source).unwrap_err();
        assert!(error.to_string().contains("modification time"));
        set_file_mtime(&source, source_mtime).unwrap();

        fallback.finish_successful_development(&prepared).unwrap();
        assert!(!source.exists());
        assert!(prepared.active().is_file());
        assert_eq!(
            fallback.prepare_known(&source).unwrap().active(),
            prepared.active()
        );
    }
}
