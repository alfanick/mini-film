use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::SystemTime,
};

use exif::{Context as ExifContext, Reader, Tag, Value};
use sha1::{Digest, Sha1};

use crate::app::dng::DngFallbackConfig;
use crate::app::util::is_raw_input_file;

const UNIQUE_CAMERA_MODEL_TAG: u16 = 0xc614;
const PROFILE_NAME_TAG: u16 = 0xc6f8;
const UNIQUE_CAMERA_MODEL: Tag = Tag(ExifContext::Tiff, UNIQUE_CAMERA_MODEL_TAG);
const NO_DCP_CACHE_IDENTITY: &str = "dcp-none";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DcpProfile {
    pub(crate) path: PathBuf,
    pub(crate) filename: String,
    pub(crate) fingerprint: String,
}

impl DcpProfile {
    pub(crate) fn cache_identity(&self) -> String {
        format!("dcp-{}", self.fingerprint)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DcpCatalogEntry {
    path: PathBuf,
    camera_model: String,
    version: u32,
}

#[derive(Clone)]
struct CachedCatalog {
    modified: Option<SystemTime>,
    entries: Arc<Vec<DcpCatalogEntry>>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CameraIdentity {
    unique_camera_model: Option<String>,
    make: Option<String>,
    model: Option<String>,
}

static CATALOG_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedCatalog>>> = OnceLock::new();

pub(crate) fn resolve_dcp_profile(
    input: &Path,
    dng_fallback: &DngFallbackConfig,
) -> Option<DcpProfile> {
    if !is_raw_input_file(input) {
        return None;
    }
    let root = dng_fallback.adobe_standard_dcp_root()?;
    let identity = read_camera_identity(input)?;
    let entries = catalog_entries(&root);
    let entry = select_catalog_entry(&entries, &identity)?;
    let fingerprint = sha1_file(&entry.path)?;
    let filename = entry.path.file_name()?.to_str()?.to_string();
    Some(DcpProfile {
        path: entry.path.clone(),
        filename,
        fingerprint,
    })
}

pub(crate) fn dcp_cache_identity(input: &Path, dng_fallback: &DngFallbackConfig) -> String {
    resolve_dcp_profile(input, dng_fallback)
        .map(|profile| profile.cache_identity())
        .unwrap_or_else(|| NO_DCP_CACHE_IDENTITY.to_string())
}

fn catalog_entries(root: &Path) -> Arc<Vec<DcpCatalogEntry>> {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let modified = fs::metadata(&root)
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    let cache = CATALOG_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock()
        && let Some(cached) = cache.get(&root)
        && cached.modified == modified
    {
        return Arc::clone(&cached.entries);
    }

    let mut paths = fs::read_dir(&root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dcp"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    let entries = Arc::new(
        paths
            .into_iter()
            .filter_map(|path| read_catalog_entry(&path))
            .collect(),
    );
    if let Ok(mut cache) = cache.lock() {
        cache.insert(
            root,
            CachedCatalog {
                modified,
                entries: Arc::clone(&entries),
            },
        );
    }
    entries
}

fn read_catalog_entry(path: &Path) -> Option<DcpCatalogEntry> {
    let bytes = fs::read(path).ok()?;
    let camera_model = normalize_camera_identity(&tiff_ascii_tag(&bytes, UNIQUE_CAMERA_MODEL_TAG)?);
    let version = adobe_standard_version(&tiff_ascii_tag(&bytes, PROFILE_NAME_TAG)?)?;
    (!camera_model.is_empty()).then(|| DcpCatalogEntry {
        path: path.to_path_buf(),
        camera_model,
        version,
    })
}

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

fn tiff_ascii_tag(bytes: &[u8], wanted_tag: u16) -> Option<String> {
    let byte_order = match bytes.get(..2)? {
        b"II" => ByteOrder::Little,
        b"MM" => ByteOrder::Big,
        _ => return None,
    };
    let magic = read_u16(bytes, 2, byte_order)?;
    if magic != 42 && magic != 0x4352 {
        return None;
    }
    let mut ifd_offset = usize::try_from(read_u32(bytes, 4, byte_order)?).ok()?;
    let mut remaining_ifds = 16;
    while ifd_offset != 0 && remaining_ifds > 0 {
        remaining_ifds -= 1;
        let entry_count = usize::from(read_u16(bytes, ifd_offset, byte_order)?);
        let entries_start = ifd_offset.checked_add(2)?;
        for index in 0..entry_count {
            let entry = entries_start.checked_add(index.checked_mul(12)?)?;
            let tag = read_u16(bytes, entry, byte_order)?;
            if tag != wanted_tag {
                continue;
            }
            let field_type = read_u16(bytes, entry.checked_add(2)?, byte_order)?;
            if field_type != 2 {
                return None;
            }
            let count =
                usize::try_from(read_u32(bytes, entry.checked_add(4)?, byte_order)?).ok()?;
            let value_start = if count <= 4 {
                entry.checked_add(8)?
            } else {
                usize::try_from(read_u32(bytes, entry.checked_add(8)?, byte_order)?).ok()?
            };
            let value = bytes.get(value_start..value_start.checked_add(count)?)?;
            return Some(
                String::from_utf8_lossy(value)
                    .trim_matches(char::from(0))
                    .trim()
                    .to_string(),
            )
            .filter(|value| !value.is_empty());
        }
        let next_ifd = entries_start.checked_add(entry_count.checked_mul(12)?)?;
        ifd_offset = usize::try_from(read_u32(bytes, next_ifd, byte_order)?).ok()?;
    }
    None
}

fn read_u16(bytes: &[u8], offset: usize, byte_order: ByteOrder) -> Option<u16> {
    let value: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(match byte_order {
        ByteOrder::Little => u16::from_le_bytes(value),
        ByteOrder::Big => u16::from_be_bytes(value),
    })
}

fn read_u32(bytes: &[u8], offset: usize, byte_order: ByteOrder) -> Option<u32> {
    let value: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(match byte_order {
        ByteOrder::Little => u32::from_le_bytes(value),
        ByteOrder::Big => u32::from_be_bytes(value),
    })
}

fn read_camera_identity(path: &Path) -> Option<CameraIdentity> {
    let exif = read_exif(path)?;
    let identity = CameraIdentity {
        unique_camera_model: field_ascii(&exif, UNIQUE_CAMERA_MODEL),
        make: field_ascii(&exif, Tag::Make),
        model: field_ascii(&exif, Tag::Model),
    };
    (identity.unique_camera_model.is_some() || identity.model.is_some()).then_some(identity)
}

fn read_exif(path: &Path) -> Option<exif::Exif> {
    let file = File::open(path).ok()?;
    Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()
}

fn field_ascii(exif: &exif::Exif, tag: Tag) -> Option<String> {
    exif.fields()
        .find(|field| field.tag == tag)
        .and_then(|field| match &field.value {
            Value::Ascii(values) => values.first(),
            _ => None,
        })
        .map(|value| {
            String::from_utf8_lossy(value)
                .trim_matches(char::from(0))
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

fn adobe_standard_version(profile_name: &str) -> Option<u32> {
    let normalized = normalize_camera_identity(profile_name);
    if normalized == "adobe standard" {
        return Some(1);
    }
    normalized
        .strip_prefix("adobe standard v")
        .and_then(|version| version.parse::<u32>().ok())
        .filter(|version| *version >= 2)
}

fn select_catalog_entry<'a>(
    entries: &'a [DcpCatalogEntry],
    identity: &CameraIdentity,
) -> Option<&'a DcpCatalogEntry> {
    if let Some(unique) = identity.unique_camera_model.as_deref() {
        let normalized = normalize_camera_identity(unique);
        return select_identity(entries, &normalized);
    }

    let model = normalize_camera_identity(identity.model.as_deref()?);
    if let Some(entry) = select_identity(entries, &model) {
        return Some(entry);
    }

    if let Some(make) = identity.make.as_deref() {
        let make_model =
            normalize_camera_identity(&format!("{make} {}", identity.model.as_deref()?));
        if let Some(entry) = select_identity(entries, &make_model) {
            return Some(entry);
        }
    }

    let suffix = format!(" {model}");
    let matching_identities = entries
        .iter()
        .filter(|entry| entry.camera_model.ends_with(&suffix))
        .map(|entry| entry.camera_model.as_str())
        .collect::<std::collections::HashSet<_>>();
    if matching_identities.len() != 1 {
        return None;
    }
    select_identity(entries, matching_identities.into_iter().next()?)
}

fn select_identity<'a>(
    entries: &'a [DcpCatalogEntry],
    camera_model: &str,
) -> Option<&'a DcpCatalogEntry> {
    let mut matches = entries
        .iter()
        .filter(|entry| entry.camera_model == camera_model)
        .collect::<Vec<_>>();
    let highest = matches.iter().map(|entry| entry.version).max()?;
    matches.retain(|entry| entry.version == highest);
    (matches.len() == 1).then_some(matches[0])
}

pub(crate) fn normalize_camera_identity(value: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    for character in value.chars() {
        if character == '+' {
            if !normalized.is_empty() && !normalized.ends_with(' ') {
                normalized.push(' ');
            }
            normalized.push_str("plus");
            pending_space = true;
        } else if character.is_alphanumeric() {
            if pending_space && !normalized.is_empty() && !normalized.ends_with(' ') {
                normalized.push(' ');
            }
            for lowercase in character.to_lowercase() {
                normalized.push(lowercase);
            }
            pending_space = false;
        } else {
            pending_space = true;
        }
    }
    normalized.trim().to_string()
}

fn sha1_file(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(model: &str, version: u32, filename: &str) -> DcpCatalogEntry {
        DcpCatalogEntry {
            path: PathBuf::from(filename),
            camera_model: normalize_camera_identity(model),
            version,
        }
    }

    #[test]
    fn normalizes_nikon_z7ii_raw_model_to_adobe_identity() {
        assert_eq!(
            normalize_camera_identity("NIKON Z 7_2"),
            normalize_camera_identity("Nikon Z 7 2")
        );
    }

    #[test]
    fn preserves_plus_as_a_semantic_camera_token() {
        assert_eq!(normalize_camera_identity("Galaxy S24+"), "galaxy s24 plus");
        assert_ne!(
            normalize_camera_identity("Galaxy S24+"),
            normalize_camera_identity("Galaxy S24")
        );
    }

    #[test]
    fn selects_latest_adobe_standard_for_exact_camera() {
        let entries = vec![
            entry("Nikon Z 6", 1, "z6-v1.dcp"),
            entry("Nikon Z 6", 2, "z6-v2.dcp"),
            entry("Nikon Z 7 2", 1, "z7ii.dcp"),
        ];
        let identity = CameraIdentity {
            model: Some("NIKON Z 6".to_string()),
            ..CameraIdentity::default()
        };

        assert_eq!(
            select_catalog_entry(&entries, &identity).map(|entry| entry.path.as_path()),
            Some(Path::new("z6-v2.dcp"))
        );
    }

    #[test]
    fn matches_vendor_prefixed_dcp_by_unique_model_suffix() {
        let entries = vec![entry("Sony ILCE-7M4", 1, "sony.dcp")];
        let identity = CameraIdentity {
            make: Some("SONY".to_string()),
            model: Some("ILCE-7M4".to_string()),
            ..CameraIdentity::default()
        };

        assert_eq!(
            select_catalog_entry(&entries, &identity).map(|entry| entry.path.as_path()),
            Some(Path::new("sony.dcp"))
        );
    }

    #[test]
    fn rejects_ambiguous_same_version_profiles() {
        let entries = vec![
            entry("Nikon Z 9", 1, "z9-a.dcp"),
            entry("Nikon Z 9", 1, "z9-b.dcp"),
        ];
        let identity = CameraIdentity {
            model: Some("NIKON Z 9".to_string()),
            ..CameraIdentity::default()
        };

        assert!(select_catalog_entry(&entries, &identity).is_none());
    }

    #[test]
    fn recognizes_only_adobe_standard_profile_names() {
        assert_eq!(adobe_standard_version("Adobe Standard"), Some(1));
        assert_eq!(adobe_standard_version("Adobe Standard v2"), Some(2));
        assert_eq!(adobe_standard_version("Camera Default"), None);
        assert_eq!(adobe_standard_version("Camera Standard"), None);
    }

    #[test]
    fn reads_camera_and_profile_names_from_adobe_dcp_tiff() {
        let camera = b"Nikon Z 7 2\0";
        let profile = b"Adobe Standard\0";
        let data_start = 8 + 2 + (2 * 12) + 4;
        let profile_start = data_start + camera.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"IIRC");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        for (tag, value, offset) in [
            (UNIQUE_CAMERA_MODEL_TAG, camera.as_slice(), data_start),
            (PROFILE_NAME_TAG, profile.as_slice(), profile_start),
        ] {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&2u16.to_le_bytes());
            bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(camera);
        bytes.extend_from_slice(profile);

        assert_eq!(
            tiff_ascii_tag(&bytes, UNIQUE_CAMERA_MODEL_TAG).as_deref(),
            Some("Nikon Z 7 2")
        );
        assert_eq!(
            tiff_ascii_tag(&bytes, PROFILE_NAME_TAG).as_deref(),
            Some("Adobe Standard")
        );
    }
}
