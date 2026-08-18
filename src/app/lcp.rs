use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::SystemTime,
};

use exif::{Context as ExifContext, Reader as ExifReader, Tag, Value};
use quick_xml::{Reader, XmlVersion, escape::unescape, events::Event};
use sha1::{Digest, Sha1};
use walkdir::WalkDir;

use crate::{
    app::{dcp::normalize_camera_identity, dng::DngFallbackConfig, util::is_raw_input_file},
    cli::LensCorrections,
};

const UNIQUE_CAMERA_MODEL: Tag = Tag(ExifContext::Tiff, 0xc614);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LcpProfile {
    pub(crate) path: PathBuf,
    pub(crate) filename: String,
    pub(crate) fingerprint: String,
}

impl LcpProfile {
    pub(crate) fn cache_identity(&self) -> String {
        format!("lcp-{}", self.fingerprint)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ResolvedLensCorrection {
    Disabled,
    AdobeLcp(LcpProfile),
    LensfunAuto,
}

impl ResolvedLensCorrection {
    pub(crate) fn cache_identity(&self, corrections: LensCorrections) -> String {
        let source = match self {
            Self::Disabled => "disabled".to_string(),
            Self::AdobeLcp(profile) => profile.cache_identity(),
            Self::LensfunAuto => "lensfun-auto".to_string(),
        };
        format!(
            "lens-{source}-d{}c{}v{}",
            u8::from(corrections.distortion),
            u8::from(corrections.ca),
            u8::from(corrections.vignetting)
        )
    }

    pub(crate) fn lcp_profile(&self) -> Option<&LcpProfile> {
        match self {
            Self::AdobeLcp(profile) => Some(profile),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct LcpCatalogEntry {
    path: PathBuf,
    make: String,
    camera_models: Vec<String>,
    pretty_lens_names: Vec<String>,
    generic_lens_names: Vec<String>,
    lens_ids: Vec<String>,
    lens_specification: Option<LensSpecification>,
    sensor_format_factor: Option<f64>,
    calibration_focals: Vec<f64>,
    version: u32,
}

#[derive(Clone)]
struct CachedCatalog {
    modified: Option<SystemTime>,
    entries: Arc<Vec<LcpCatalogEntry>>,
}

#[derive(Clone, Copy, Debug)]
struct LensSpecification {
    min_focal: f64,
    max_focal: f64,
    min_aperture: Option<f64>,
    max_aperture: Option<f64>,
}

#[derive(Debug, Default)]
struct LensIdentity {
    make: Option<String>,
    model: Option<String>,
    unique_camera_model: Option<String>,
    lens_model: Option<String>,
    lens_ids: Vec<String>,
    lens_specification: Option<LensSpecification>,
    focal_length: Option<f64>,
    sensor_format_factor: Option<f64>,
}

#[derive(Default)]
struct LcpXmlValues {
    raw_flags: Vec<bool>,
    makes: Vec<String>,
    models: Vec<String>,
    unique_camera_models: Vec<String>,
    camera_pretty_names: Vec<String>,
    lens_names: Vec<String>,
    lens_pretty_names: Vec<String>,
    alternate_lens_names: Vec<String>,
    lens_ids: Vec<String>,
    alternate_lens_ids: Vec<String>,
    lens_infos: Vec<String>,
    sensor_format_factors: Vec<f64>,
    focal_lengths: Vec<f64>,
    profile_names: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CandidateRank {
    lens_match: u8,
    camera_match: u8,
    factor_distance: u32,
    version: u32,
    focal_distance: u32,
}

impl Ord for CandidateRank {
    fn cmp(&self, other: &Self) -> Ordering {
        self.lens_match
            .cmp(&other.lens_match)
            .then(self.camera_match.cmp(&other.camera_match))
            .then(other.factor_distance.cmp(&self.factor_distance))
            .then(self.version.cmp(&other.version))
            .then(other.focal_distance.cmp(&self.focal_distance))
    }
}

impl PartialOrd for CandidateRank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

static CATALOG_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedCatalog>>> = OnceLock::new();

pub(crate) fn resolve_lcp_profile(
    input: &Path,
    dng_fallback: &DngFallbackConfig,
    lcp_root: Option<&Path>,
) -> Option<LcpProfile> {
    if !is_raw_input_file(input) {
        return None;
    }
    let root = lcp_root
        .map(Path::to_path_buf)
        .or_else(|| dng_fallback.adobe_lcp_root())?;
    let identity = read_lens_identity(input)?;
    let entries = catalog_entries(&root);
    let entry = select_catalog_entry(&entries, &identity)?;
    let fingerprint = sha1_file(&entry.path)?;
    let filename = entry.path.file_name()?.to_str()?.to_string();
    Some(LcpProfile {
        path: entry.path.clone(),
        filename,
        fingerprint,
    })
}

pub(crate) fn resolve_lens_correction(
    input: &Path,
    dng_fallback: &DngFallbackConfig,
    lcp_root: Option<&Path>,
    corrections: LensCorrections,
) -> ResolvedLensCorrection {
    if !corrections.is_enabled() || !is_raw_input_file(input) {
        return ResolvedLensCorrection::Disabled;
    }
    if let Some(profile) = resolve_lcp_profile(input, dng_fallback, lcp_root) {
        return ResolvedLensCorrection::AdobeLcp(profile);
    }
    ResolvedLensCorrection::LensfunAuto
}

pub(crate) fn lens_correction_cache_identity(
    input: &Path,
    dng_fallback: &DngFallbackConfig,
    lcp_root: Option<&Path>,
    corrections: LensCorrections,
) -> String {
    resolve_lens_correction(input, dng_fallback, lcp_root, corrections).cache_identity(corrections)
}

fn catalog_entries(root: &Path) -> Arc<Vec<LcpCatalogEntry>> {
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

    let mut paths = WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("lcp"))
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

fn read_catalog_entry(path: &Path) -> Option<LcpCatalogEntry> {
    let file = File::open(path).ok()?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut stack = Vec::<String>::new();
    let mut values = LcpXmlValues::default();

    loop {
        match reader.read_event_into(&mut buffer).ok()? {
            Event::Start(event) => {
                collect_lcp_attributes(&event, &mut values)?;
                stack.push(local_xml_name(event.name().as_ref()));
            }
            Event::Empty(event) => collect_lcp_attributes(&event, &mut values)?,
            Event::Text(event) => {
                let decoded = event.decode().ok()?;
                let value = unescape(&decoded).ok()?.trim().to_string();
                if !value.is_empty() {
                    collect_lcp_text(&stack, &value, &mut values);
                }
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    values_to_catalog_entry(path, values)
}

fn collect_lcp_attributes(
    event: &quick_xml::events::BytesStart<'_>,
    values: &mut LcpXmlValues,
) -> Option<()> {
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute.ok()?;
        let key = local_xml_name(attribute.key.as_ref());
        let value = attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .ok()?
            .into_owned();
        collect_lcp_value(&key, value.trim(), values);
    }
    Some(())
}

fn collect_lcp_text(stack: &[String], value: &str, values: &mut LcpXmlValues) {
    let Some(current) = stack.last() else {
        return;
    };
    if current == "li" {
        if stack.iter().any(|name| name == "AlternateLensNames") {
            push_unique(&mut values.alternate_lens_names, value);
        } else if stack.iter().any(|name| name == "AlternateLensIDs") {
            push_unique(&mut values.alternate_lens_ids, value);
        }
    } else {
        collect_lcp_value(current, value, values);
    }
}

fn collect_lcp_value(key: &str, value: &str, values: &mut LcpXmlValues) {
    if value.is_empty() {
        return;
    }
    match key {
        "CameraRawProfile" => match value.to_ascii_lowercase().as_str() {
            "true" => values.raw_flags.push(true),
            "false" => values.raw_flags.push(false),
            _ => {}
        },
        "Make" => push_unique(&mut values.makes, value),
        "Model" => push_unique(&mut values.models, value),
        "UniqueCameraModel" => push_unique(&mut values.unique_camera_models, value),
        "CameraPrettyName" => push_unique(&mut values.camera_pretty_names, value),
        "Lens" => push_unique(&mut values.lens_names, value),
        "LensPrettyName" => push_unique(&mut values.lens_pretty_names, value),
        "LensID" => push_unique(&mut values.lens_ids, value),
        "LensInfo" => push_unique(&mut values.lens_infos, value),
        "SensorFormatFactor" => push_f64(&mut values.sensor_format_factors, value),
        "FocalLength" => push_f64(&mut values.focal_lengths, value),
        "ProfileName" => push_unique(&mut values.profile_names, value),
        _ => {}
    }
}

fn values_to_catalog_entry(path: &Path, values: LcpXmlValues) -> Option<LcpCatalogEntry> {
    if values.raw_flags.is_empty() || values.raw_flags.iter().any(|raw| !raw) {
        return None;
    }
    let make = unique_normalized(&values.makes, normalize_make)?;
    let mut camera_models = normalized_values(
        values
            .models
            .iter()
            .chain(&values.unique_camera_models)
            .chain(&values.camera_pretty_names),
        normalize_camera_identity,
    );
    camera_models.sort();
    camera_models.dedup();

    let mut pretty_lens_names = normalized_values(
        values
            .lens_pretty_names
            .iter()
            .chain(&values.alternate_lens_names),
        normalize_lens_identity,
    );
    pretty_lens_names.sort();
    pretty_lens_names.dedup();
    let mut generic_lens_names =
        normalized_values(values.lens_names.iter(), normalize_lens_identity);
    generic_lens_names.sort();
    generic_lens_names.dedup();
    if pretty_lens_names.is_empty() && generic_lens_names.is_empty() {
        return None;
    }

    let mut lens_ids = normalized_values(
        values.lens_ids.iter().chain(&values.alternate_lens_ids),
        normalize_lens_identity,
    );
    lens_ids.sort();
    lens_ids.dedup();
    let lens_specification = values
        .lens_infos
        .iter()
        .find_map(|value| parse_lens_info(value));
    let sensor_format_factor = consistent_f64(&values.sensor_format_factors, 0.01);
    let mut calibration_focals = values.focal_lengths;
    calibration_focals.sort_by(f64::total_cmp);
    calibration_focals.dedup_by(|left, right| (*left - *right).abs() < 0.001);
    let embedded_version = values
        .profile_names
        .iter()
        .filter_map(|name| profile_version(name))
        .max();
    let filename_version = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(profile_version);
    let version = embedded_version
        .into_iter()
        .chain(filename_version)
        .max()
        .unwrap_or(1);

    Some(LcpCatalogEntry {
        path: path.to_path_buf(),
        make,
        camera_models,
        pretty_lens_names,
        generic_lens_names,
        lens_ids,
        lens_specification,
        sensor_format_factor,
        calibration_focals,
        version,
    })
}

fn select_catalog_entry<'a>(
    entries: &'a [LcpCatalogEntry],
    identity: &LensIdentity,
) -> Option<&'a LcpCatalogEntry> {
    let make = normalize_make(identity.make.as_deref()?);
    let lens_model = identity.lens_model.as_deref().map(normalize_lens_identity);
    let lens_ids = identity
        .lens_ids
        .iter()
        .map(|value| normalize_lens_identity(value))
        .collect::<HashSet<_>>();
    let camera_models = [
        identity.unique_camera_model.as_deref(),
        identity.model.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(normalize_camera_identity)
    .collect::<HashSet<_>>();

    let mut ranked = entries
        .iter()
        .filter(|entry| entry.make == make)
        .filter_map(|entry| {
            candidate_rank(
                entry,
                identity,
                lens_model.as_deref(),
                &lens_ids,
                &camera_models,
            )
            .map(|rank| (entry, rank))
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(_, rank)| std::cmp::Reverse(*rank));
    let (best, best_rank) = ranked.first().copied()?;
    if ranked.get(1).is_some_and(|(_, rank)| *rank == best_rank) {
        return None;
    }
    Some(best)
}

fn candidate_rank(
    entry: &LcpCatalogEntry,
    identity: &LensIdentity,
    lens_model: Option<&str>,
    lens_ids: &HashSet<String>,
    camera_models: &HashSet<String>,
) -> Option<CandidateRank> {
    if !sensor_factor_compatible(entry.sensor_format_factor, identity.sensor_format_factor)
        || !lens_specification_compatible(entry.lens_specification, identity.lens_specification)
    {
        return None;
    }
    if let (Some(focal), Some(specification)) = (identity.focal_length, entry.lens_specification)
        && (focal < specification.min_focal * 0.98 || focal > specification.max_focal * 1.02)
    {
        return None;
    }

    let lens_match = if !lens_ids.is_empty()
        && entry
            .lens_ids
            .iter()
            .any(|lens_id| lens_ids.contains(lens_id))
    {
        4
    } else if lens_model
        .is_some_and(|model| entry.pretty_lens_names.iter().any(|name| name == model))
    {
        3
    } else if lens_model.is_some_and(|model| {
        entry
            .pretty_lens_names
            .iter()
            .any(|name| lens_name_is_unambiguous_subset(model, name))
    }) {
        2
    } else if lens_model
        .is_some_and(|model| entry.generic_lens_names.iter().any(|name| name == model))
    {
        1
    } else {
        return None;
    };
    let camera_match = if entry
        .camera_models
        .iter()
        .any(|model| camera_models.contains(model))
    {
        2
    } else if entry.camera_models.is_empty() {
        1
    } else {
        0
    };
    let factor_distance = match (entry.sensor_format_factor, identity.sensor_format_factor) {
        (Some(entry), Some(input)) => ((entry - input).abs() * 10_000.0).round() as u32,
        _ => u32::MAX / 2,
    };
    let focal_distance = match identity.focal_length {
        Some(focal) if !entry.calibration_focals.is_empty() => entry
            .calibration_focals
            .iter()
            .map(|calibration| ((calibration - focal).abs() * 1_000.0).round() as u32)
            .min()
            .unwrap_or(u32::MAX / 2),
        _ => u32::MAX / 2,
    };
    Some(CandidateRank {
        lens_match,
        camera_match,
        factor_distance,
        version: entry.version,
        focal_distance,
    })
}

fn lens_name_is_unambiguous_subset(input: &str, profile: &str) -> bool {
    let input = input.split_whitespace().collect::<HashSet<_>>();
    let profile = profile.split_whitespace().collect::<HashSet<_>>();
    let (smaller, larger) = if input.len() <= profile.len() {
        (&input, &profile)
    } else {
        (&profile, &input)
    };
    let meaningful = smaller
        .iter()
        .copied()
        .filter(|token| !matches!(*token, "f" | "mm"))
        .collect::<Vec<_>>();
    meaningful.len() >= 4
        && meaningful
            .iter()
            .any(|token| token.bytes().any(|byte| byte.is_ascii_digit()))
        && meaningful.iter().all(|token| larger.contains(*token))
}

fn read_lens_identity(path: &Path) -> Option<LensIdentity> {
    let file = File::open(path).ok()?;
    let exif = ExifReader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()?;
    let focal_length = field_number(&exif, Tag::FocalLength);
    let focal_length_35 = field_number(&exif, Tag::FocalLengthIn35mmFilm);
    let sensor_format_factor = focal_length
        .filter(|value| *value > 0.0)
        .and_then(|focal| focal_length_35.map(|equivalent| equivalent / focal))
        .filter(|factor| (0.5..=8.0).contains(factor));
    let identity = LensIdentity {
        make: field_ascii(&exif, Tag::Make),
        model: field_ascii(&exif, Tag::Model),
        unique_camera_model: field_ascii(&exif, UNIQUE_CAMERA_MODEL),
        lens_model: field_ascii(&exif, Tag::LensModel),
        lens_ids: Vec::new(),
        lens_specification: field_lens_specification(&exif),
        focal_length,
        sensor_format_factor,
    };
    (identity.make.is_some() && identity.lens_model.is_some()).then_some(identity)
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

fn field_number(exif: &exif::Exif, tag: Tag) -> Option<f64> {
    let value = &exif.fields().find(|field| field.tag == tag)?.value;
    match value {
        Value::Rational(values) => values.first().map(|value| value.to_f64()),
        Value::SRational(values) => values.first().map(|value| value.to_f64()),
        Value::Short(values) => values.first().map(|value| f64::from(*value)),
        Value::Long(values) => values.first().map(|value| f64::from(*value)),
        Value::Float(values) => values.first().map(|value| f64::from(*value)),
        Value::Double(values) => values.first().copied(),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

fn field_lens_specification(exif: &exif::Exif) -> Option<LensSpecification> {
    let field = exif
        .fields()
        .find(|field| field.tag == Tag::LensSpecification)?;
    let Value::Rational(values) = &field.value else {
        return None;
    };
    let values = values.get(..4)?;
    Some(LensSpecification {
        min_focal: values[0].to_f64(),
        max_focal: values[1].to_f64(),
        min_aperture: rational_nonzero(values[2]),
        max_aperture: rational_nonzero(values[3]),
    })
}

fn rational_nonzero(value: exif::Rational) -> Option<f64> {
    (value.denom != 0 && value.num != 0).then(|| value.to_f64())
}

fn normalize_make(value: &str) -> String {
    let mut tokens = normalize_camera_identity(value)
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    while tokens.last().is_some_and(|token| {
        matches!(
            token.as_str(),
            "corporation" | "corp" | "inc" | "limited" | "ltd" | "company" | "co"
        )
    }) {
        tokens.pop();
    }
    tokens.join(" ")
}

fn normalize_lens_identity(value: &str) -> String {
    let mut output = String::new();
    let mut previous_kind = None::<u8>;
    for character in value.chars() {
        let kind = if character.is_alphabetic() {
            1
        } else if character.is_ascii_digit() {
            2
        } else {
            0
        };
        if kind == 0 {
            if !output.is_empty() && !output.ends_with(' ') {
                output.push(' ');
            }
            previous_kind = None;
            continue;
        }
        if previous_kind.is_some_and(|previous| previous != kind)
            && !output.is_empty()
            && !output.ends_with(' ')
        {
            output.push(' ');
        }
        for lowercase in character.to_lowercase() {
            output.push(lowercase);
        }
        previous_kind = Some(kind);
    }
    let mut tokens = output.split_whitespace().collect::<Vec<_>>();
    while tokens.first().is_some_and(|token| {
        matches!(
            *token,
            "canon"
                | "nikon"
                | "sony"
                | "sigma"
                | "tamron"
                | "zeiss"
                | "fujifilm"
                | "fuji"
                | "panasonic"
                | "leica"
                | "olympus"
                | "pentax"
                | "ricoh"
                | "samyang"
                | "rokinon"
                | "tokina"
                | "viltrox"
                | "laowa"
                | "venus"
                | "optics"
        )
    }) {
        tokens.remove(0);
    }
    tokens.join(" ")
}

fn parse_lens_info(value: &str) -> Option<LensSpecification> {
    let values = value
        .split_whitespace()
        .map(parse_ratio)
        .collect::<Option<Vec<_>>>()?;
    let values = values.get(..4)?;
    Some(LensSpecification {
        min_focal: values[0],
        max_focal: values[1],
        min_aperture: (values[2] > 0.0).then_some(values[2]),
        max_aperture: (values[3] > 0.0).then_some(values[3]),
    })
}

fn parse_ratio(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    (denominator != 0.0).then_some(numerator / denominator)
}

fn profile_version(value: &str) -> Option<u32> {
    let bytes = value.as_bytes();
    let mut version = None;
    for index in 0..bytes.len().saturating_sub(2) {
        if bytes[index].is_ascii_whitespace()
            && matches!(bytes[index + 1], b'v' | b'V')
            && bytes[index + 2].is_ascii_digit()
        {
            let end = bytes[index + 2..]
                .iter()
                .position(|byte| !byte.is_ascii_digit())
                .map(|offset| index + 2 + offset)
                .unwrap_or(bytes.len());
            version = value[index + 2..end].parse().ok().or(version);
        }
    }
    version.or(Some(1))
}

fn sensor_factor_compatible(profile: Option<f64>, input: Option<f64>) -> bool {
    match (profile, input) {
        (Some(profile), Some(input)) => (profile - input).abs() <= 0.2,
        _ => true,
    }
}

fn lens_specification_compatible(
    profile: Option<LensSpecification>,
    input: Option<LensSpecification>,
) -> bool {
    let (Some(profile), Some(input)) = (profile, input) else {
        return true;
    };
    close(profile.min_focal, input.min_focal, 0.02)
        && close(profile.max_focal, input.max_focal, 0.02)
        && optional_close(profile.min_aperture, input.min_aperture, 0.05)
        && optional_close(profile.max_aperture, input.max_aperture, 0.05)
}

fn close(left: f64, right: f64, relative_tolerance: f64) -> bool {
    (left - right).abs() <= left.abs().max(right.abs()).max(1.0) * relative_tolerance
}

fn optional_close(left: Option<f64>, right: Option<f64>, tolerance: f64) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => close(left, right, tolerance),
        _ => true,
    }
}

fn unique_normalized(values: &[String], normalize: impl Fn(&str) -> String) -> Option<String> {
    let values = values
        .iter()
        .map(|value| normalize(value))
        .filter(|value| !value.is_empty())
        .collect::<HashSet<_>>();
    (values.len() == 1)
        .then(|| values.into_iter().next())
        .flatten()
}

fn normalized_values<'a>(
    values: impl Iterator<Item = &'a String>,
    normalize: impl Fn(&str) -> String,
) -> Vec<String> {
    values
        .map(|value| normalize(value))
        .filter(|value| !value.is_empty())
        .collect()
}

fn consistent_f64(values: &[f64], tolerance: f64) -> Option<f64> {
    let first = *values.first()?;
    values
        .iter()
        .all(|value| (*value - first).abs() <= tolerance)
        .then_some(first)
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

fn push_f64(values: &mut Vec<f64>, value: &str) {
    if let Ok(value) = value.parse::<f64>()
        && value.is_finite()
    {
        values.push(value);
    }
}

fn local_xml_name(name: &[u8]) -> String {
    let name = name.rsplit(|byte| *byte == b':').next().unwrap_or(name);
    String::from_utf8_lossy(name).into_owned()
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
    Some(hex_digest(hasher.finalize().as_slice()))
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, lens: &str, version: u32, focals: &[f64]) -> LcpCatalogEntry {
        LcpCatalogEntry {
            path: PathBuf::from(path),
            make: normalize_make("NIKON CORPORATION"),
            camera_models: vec![normalize_camera_identity("Nikon D3")],
            pretty_lens_names: vec![normalize_lens_identity(lens)],
            generic_lens_names: Vec::new(),
            lens_ids: vec!["174".to_string()],
            lens_specification: Some(LensSpecification {
                min_focal: 200.0,
                max_focal: 500.0,
                min_aperture: Some(5.6),
                max_aperture: Some(5.6),
            }),
            sensor_format_factor: Some(1.0),
            calibration_focals: focals.to_vec(),
            version,
        }
    }

    fn dsc_4115_identity() -> LensIdentity {
        LensIdentity {
            make: Some("NIKON CORPORATION".to_string()),
            model: Some("NIKON Z 7_2".to_string()),
            lens_model: Some("VR 200-500mm f/5.6E".to_string()),
            focal_length: Some(500.0),
            sensor_format_factor: Some(1.0),
            ..LensIdentity::default()
        }
    }

    #[test]
    fn parses_recursive_raw_attribute_profile_and_alternate_name() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join("Nikon");
        fs::create_dir_all(&nested).unwrap();
        let raw = nested.join("lens v2 - RAW.lcp");
        fs::write(
            &raw,
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:stCamera="http://ns.adobe.com/photoshop/1.0/camera-profile">
<rdf:Description stCamera:Make="NIKON CORPORATION" stCamera:Model="NIKON D3" stCamera:CameraRawProfile="True" stCamera:LensID="174" stCamera:Lens="200.0-500.0 mm f/5.6" stCamera:LensInfo="2000/10 5000/10 56/10 56/10" stCamera:LensPrettyName="Nikon AF-S NIKKOR 200-500mm f/5.6E ED VR" stCamera:ProfileName="Adobe (Nikon AF-S NIKKOR 200-500mm f/5.6E ED VR) v2" stCamera:SensorFormatFactor="1" stCamera:FocalLength="200"><stCamera:AlternateLensNames><rdf:Seq><rdf:li>Nikkor 200-500 test alias</rdf:li></rdf:Seq></stCamera:AlternateLensNames></rdf:Description>
<rdf:Description stCamera:Make="NIKON CORPORATION" stCamera:Model="NIKON D3" stCamera:CameraRawProfile="True" stCamera:LensID="174" stCamera:Lens="200.0-500.0 mm f/5.6" stCamera:LensInfo="2000/10 5000/10 56/10 56/10" stCamera:LensPrettyName="Nikon AF-S NIKKOR 200-500mm f/5.6E ED VR" stCamera:ProfileName="Adobe (Nikon AF-S NIKKOR 200-500mm f/5.6E ED VR) v2" stCamera:SensorFormatFactor="1" stCamera:FocalLength="500" />
</x:xmpmeta>"#,
        )
        .unwrap();
        let nonraw = nested.join("lens.lcp");
        fs::write(
            &nonraw,
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:stCamera="http://ns.adobe.com/photoshop/1.0/camera-profile"><stCamera:Make>NIKON CORPORATION</stCamera:Make><stCamera:CameraRawProfile>False</stCamera:CameraRawProfile><stCamera:LensPrettyName>Nikon lens</stCamera:LensPrettyName></x:xmpmeta>"#,
        )
        .unwrap();

        let entries = catalog_entries(temp.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].version, 2);
        assert_eq!(entries[0].calibration_focals, vec![200.0, 500.0]);
        assert!(
            entries[0]
                .pretty_lens_names
                .contains(&normalize_lens_identity("Nikkor 200-500 test alias"))
        );
    }

    #[test]
    fn matches_nikon_short_exif_name_to_adobe_marketing_name() {
        let entries = vec![entry(
            "NIKON D3 (Nikon AF-S NIKKOR 200-500mm f5.6E ED VR) - RAW.lcp",
            "Nikon AF-S NIKKOR 200-500mm f/5.6E ED VR",
            1,
            &[200.0, 300.0, 400.0, 500.0],
        )];

        assert_eq!(
            select_catalog_entry(&entries, &dsc_4115_identity()).map(|entry| entry.path.as_path()),
            Some(Path::new(
                "NIKON D3 (Nikon AF-S NIKKOR 200-500mm f5.6E ED VR) - RAW.lcp"
            ))
        );
    }

    #[test]
    fn refuses_fuzzy_generation_mismatch_or_ambiguous_alias() {
        assert!(!lens_name_is_unambiguous_subset(
            &normalize_lens_identity("NIKKOR Z 24-70mm f/2.8 S"),
            &normalize_lens_identity("Nikon AF-S NIKKOR 24-70mm f/2.8E ED VR")
        ));

        let entries = vec![
            entry(
                "first.lcp",
                "Nikon AF-S NIKKOR 200-500mm f/5.6E ED VR",
                1,
                &[500.0],
            ),
            entry(
                "duplicate.lcp",
                "Nikon AF-S NIKKOR 200-500mm f/5.6E ED VR",
                1,
                &[500.0],
            ),
        ];
        assert!(select_catalog_entry(&entries, &dsc_4115_identity()).is_none());
    }

    #[test]
    fn cache_identity_covers_source_and_requested_components() {
        let source = ResolvedLensCorrection::LensfunAuto;
        let all = source.cache_identity(LensCorrections::all());
        let distortion = source.cache_identity(LensCorrections {
            distortion: true,
            ca: false,
            vignetting: false,
        });
        assert_ne!(all, distortion);
        assert!(all.contains("lensfun-auto"));
    }

    #[test]
    fn enabled_dng_without_matching_lcp_falls_back_to_lensfun() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("frame.dng");
        let catalog = temp.path().join("empty-lcp-catalog");
        fs::write(&input, b"not a TIFF").unwrap();
        fs::create_dir(&catalog).unwrap();

        assert_eq!(
            resolve_lens_correction(
                &input,
                &DngFallbackConfig::default(),
                Some(&catalog),
                LensCorrections::all(),
            ),
            ResolvedLensCorrection::LensfunAuto
        );
    }
}
