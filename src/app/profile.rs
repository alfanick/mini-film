use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result, bail};
use mini_film::{
    ConvertedProfile, GrainSettings, HaldOptions, XmpFilmRecipe, convert_xmp_to_hald,
    extract_film_recipe, profile_info_line, rawtherapee_hald_clut_profile_text,
    write_rawtherapee_profile,
};
use walkdir::WalkDir;

use crate::app::apply::ApplyArgs;

pub(crate) struct ResolvedProfile {
    pub(crate) hald_path: Option<PathBuf>,
    pub(crate) rawtherapee_profiles: Vec<PathBuf>,
    pub(crate) grain: GrainSettings,
    pub(crate) resolved_stem: String,
}

pub(crate) enum ProfileInfo {
    HaldPng {
        path: PathBuf,
    },
    Emulation {
        path: PathBuf,
        recipe: Box<XmpFilmRecipe>,
        source: PathBuf,
        converted: Box<ConvertedProfile>,
        hald_path: PathBuf,
    },
    RgbTableProfile {
        path: PathBuf,
        converted: Box<ConvertedProfile>,
        hald_path: PathBuf,
    },
    RawTherapeePp3 {
        path: PathBuf,
    },
}

pub(crate) fn inspect_profile(
    selector: &str,
    profiles_root: &Path,
    hald_dir: &Path,
    hald_level: u32,
) -> Result<ProfileInfo> {
    let selector_path = profile_selector_path(selector)?;
    if selector_path.exists() {
        return inspect_profile_path(&selector_path, profiles_root, hald_dir, hald_level);
    }

    if looks_like_rgb_profile_selector(selector) {
        for root in rgb_profile_roots(profiles_root, profiles_root) {
            if let Some(path) = find_rgb_xmp_by_name(&root, selector)? {
                return inspect_profile_path(&path, profiles_root, hald_dir, hald_level);
            }
        }
    }

    for root in emulation_selector_roots(profiles_root) {
        if let Some(path) = find_xmp_by_name(&root, selector)? {
            return inspect_profile_path(&path, profiles_root, hald_dir, hald_level);
        }
    }

    for root in rgb_profile_roots(profiles_root, profiles_root) {
        if let Some(path) = find_rgb_xmp_by_name(&root, selector)? {
            return inspect_profile_path(&path, profiles_root, hald_dir, hald_level);
        }
    }

    if let Some(path) = find_hald_by_name(hald_dir, selector)? {
        return Ok(ProfileInfo::HaldPng { path });
    }

    bail!(
        "could not resolve profile {:?} as an emulation, internal RGBTable profile, or Hald under {}",
        selector,
        profiles_root.display()
    )
}

pub(crate) fn rawtherapee_profiles_with_hald(
    resolved: &ResolvedProfile,
    temp_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let mut profiles = resolved.rawtherapee_profiles.clone();
    if let Some(hald_path) = &resolved.hald_path {
        let lut_profile = temp_dir.join("rt-hald-clut.pp3");
        std::fs::write(&lut_profile, rawtherapee_hald_clut_profile_text(hald_path))
            .with_context(|| format!("writing {}", lut_profile.display()))?;
        profiles.push(lut_profile);
    }
    Ok(profiles)
}

/// Resolve a CLI profile selector into a concrete Hald file plus recipe metadata.
///
/// The selector can be a real path, an emulation XMP name under `emulations/`,
/// a generated Hald name under `hald_dir`, or a human-authored `.pp3` path.
/// Emulation XMP inputs generate a temporary Hald from their linked internal
/// RGBTable profile and may generate RawTherapee `.pp3` files for
/// tone/color/sharpening metadata; raw PNG Hald and PP3 inputs have no attached
/// mini-film grain metadata, so they resolve with defaults.
pub(crate) fn resolve_profile(args: &ApplyArgs, temp_dir: &Path) -> Result<ResolvedProfile> {
    let selector_path = profile_selector_path(&args.profile)?;
    if selector_path.exists() {
        return profile_from_path(
            &selector_path,
            args.hald_level,
            &args.profiles_root,
            &args.hald_dir,
            temp_dir,
        );
    }

    for root in emulation_selector_roots(&args.profiles_root) {
        if let Some(path) = find_xmp_by_name(&root, &args.profile)? {
            return profile_from_path(
                &path,
                args.hald_level,
                &args.profiles_root,
                &args.hald_dir,
                temp_dir,
            );
        }
    }

    if let Some(path) = find_hald_by_name(&args.hald_dir, &args.profile)? {
        let resolved_stem = profile_stem_for_output(&path);
        return Ok(ResolvedProfile {
            hald_path: Some(path),
            rawtherapee_profiles: Vec::new(),
            grain: GrainSettings::default(),
            resolved_stem,
        });
    }

    bail!(
        "could not resolve profile {:?} as a file, emulation XMP name under {}, Hald name under {}, or PP3 path",
        args.profile,
        args.profiles_root.display(),
        args.hald_dir.display()
    );
}

fn profile_selector_path(selector: &str) -> Result<PathBuf> {
    if let Some(path) = local_file_url_to_path(selector)? {
        return Ok(path);
    }
    Ok(PathBuf::from(selector))
}

fn local_file_url_to_path(selector: &str) -> Result<Option<PathBuf>> {
    let Some(rest) = selector.strip_prefix("file://") else {
        return Ok(None);
    };
    if rest.contains('?') || rest.contains('#') {
        bail!("file:// profile URLs with query strings or fragments are not supported");
    }

    let path = if rest.starts_with('/') {
        rest
    } else {
        let (host, path) = rest.split_once('/').ok_or_else(|| {
            anyhow::anyhow!("file:// profile URL must include an absolute local path")
        })?;
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            bail!("file:// profile URL host must be empty or localhost, got {host:?}");
        }
        path
    };

    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Ok(Some(PathBuf::from(percent_decode_file_url_path(&path)?)))
}

fn percent_decode_file_url_path(path: &str) -> Result<String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                bail!("invalid percent escape in file:// profile URL");
            }
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).context("file:// profile URL path is not valid UTF-8")
}

fn hex_value(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => bail!("invalid percent escape in file:// profile URL"),
    }
}

/// Resolve an explicit profile path by extension.
///
/// PNG files are already usable Hald CLUTs and are returned directly. XMP files
/// must be emulations that point at a Look; direct RGBTable profile XMPs are
/// reserved for internal LUT resolution and the `convert` command. Other
/// extensions are rejected early to make command failures clearer.
fn profile_from_path(
    path: &Path,
    hald_level: u32,
    profiles_root: &Path,
    hald_dir: &Path,
    temp_dir: &Path,
) -> Result<ResolvedProfile> {
    let resolved_stem = profile_stem_for_output(path).to_string();
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("png") => Ok(ResolvedProfile {
            hald_path: Some(path.to_path_buf()),
            rawtherapee_profiles: Vec::new(),
            grain: GrainSettings::default(),
            resolved_stem,
        }),
        Some(ext) if ext.eq_ignore_ascii_case("xmp") => {
            profile_from_xmp(path, hald_level, profiles_root, hald_dir, temp_dir)
        }
        Some(ext) if ext.eq_ignore_ascii_case("pp3") => Ok(ResolvedProfile {
            hald_path: None,
            rawtherapee_profiles: vec![path.to_path_buf()],
            grain: GrainSettings::default(),
            resolved_stem,
        }),
        Some(ext) => {
            bail!("unsupported profile path extension .{ext}; expected .png, .xmp, or .pp3")
        }
        None => bail!("profile path has no extension: {}", path.display()),
    }
}

fn inspect_profile_path(
    path: &Path,
    profiles_root: &Path,
    hald_dir: &Path,
    hald_level: u32,
) -> Result<ProfileInfo> {
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("png") => Ok(ProfileInfo::HaldPng {
            path: path.to_path_buf(),
        }),
        Some(ext) if ext.eq_ignore_ascii_case("xmp") => {
            inspect_xmp_profile_path(path, profiles_root, hald_dir, hald_level)
        }
        Some(ext) if ext.eq_ignore_ascii_case("pp3") => Ok(ProfileInfo::RawTherapeePp3 {
            path: path.to_path_buf(),
        }),
        Some(ext) => {
            bail!("unsupported profile path extension .{ext}; expected .png, .xmp, or .pp3")
        }
        None => bail!("profile path has no extension: {}", path.display()),
    }
}

fn inspect_xmp_profile_path(
    path: &Path,
    profiles_root: &Path,
    hald_dir: &Path,
    hald_level: u32,
) -> Result<ProfileInfo> {
    let recipe = extract_film_recipe(path)?;
    if recipe.rgb_table.is_some() {
        let hald_path = cached_hald_path(path, hald_level, hald_dir)?;
        let converted = convert_xmp_to_hald(
            path,
            &hald_path,
            HaldOptions {
                hald_level,
                overwrite: false,
                info_only: true,
            },
        )?;
        return Ok(ProfileInfo::RgbTableProfile {
            path: path.to_path_buf(),
            converted: Box::new(converted),
            hald_path,
        });
    }

    let source = resolve_recipe_profile(&recipe, profiles_root, path)
        .with_context(|| format!("resolving linked profile for preset {}", path.display()))?;
    let hald_path = cached_hald_path(&source, hald_level, hald_dir)?;
    let converted = convert_xmp_to_hald(
        &source,
        &hald_path,
        HaldOptions {
            hald_level,
            overwrite: false,
            info_only: true,
        },
    )?;
    Ok(ProfileInfo::Emulation {
        path: path.to_path_buf(),
        recipe: Box::new(recipe),
        source,
        converted: Box::new(converted),
        hald_path,
    })
}

/// Resolve an XMP file to a temporary Hald and recipe settings.
///
/// User-facing XMPs are Lightroom emulation presets. The linked Look is resolved
/// by UUID/name under the internal `profiles/` tree before conversion. The
/// generated Hald is written into the caller's temp directory, grain comes from
/// the emulation recipe, and supported tone/color/sharpening metadata is written
/// as RawTherapee `.pp3` side profiles.
pub(crate) fn profile_from_xmp(
    path: &Path,
    hald_level: u32,
    profiles_root: &Path,
    hald_dir: &Path,
    temp_dir: &Path,
) -> Result<ResolvedProfile> {
    profile_from_xmp_inner(path, hald_level, profiles_root, hald_dir, temp_dir, true)
}

pub(crate) fn profile_from_xmp_quiet(
    path: &Path,
    hald_level: u32,
    profiles_root: &Path,
    hald_dir: &Path,
    temp_dir: &Path,
) -> Result<ResolvedProfile> {
    profile_from_xmp_inner(path, hald_level, profiles_root, hald_dir, temp_dir, false)
}

fn profile_from_xmp_inner(
    path: &Path,
    hald_level: u32,
    profiles_root: &Path,
    hald_dir: &Path,
    temp_dir: &Path,
    print_info: bool,
) -> Result<ResolvedProfile> {
    let recipe = extract_film_recipe(path)?;
    if recipe.rgb_table.is_some() {
        bail!(
            "profile XMPs with RGBTable are internal; use an emulation XMP from emulations instead: {}",
            path.display()
        );
    };
    let source = resolve_recipe_profile(&recipe, profiles_root, path)
        .with_context(|| format!("resolving linked profile for preset {}", path.display()))?;

    let output = cached_hald_path(&source, hald_level, hald_dir)?;
    let converted = if output.exists() {
        convert_xmp_to_hald(
            &source,
            &output,
            HaldOptions {
                hald_level,
                overwrite: false,
                info_only: true,
            },
        )?
    } else {
        convert_xmp_to_hald(
            &source,
            &output,
            HaldOptions {
                hald_level,
                overwrite: false,
                info_only: false,
            },
        )?
    };
    if print_info {
        eprintln!("{}", profile_info_line(&converted));
    }
    let mut rawtherapee_profiles = Vec::new();
    if let Some(path) = write_rawtherapee_profile(
        &temp_dir.join("source.pp3"),
        &converted.adjustments,
        converted.sharpening,
    )? {
        rawtherapee_profiles.push(path);
    }
    if let Some(path) = write_rawtherapee_profile(
        &temp_dir.join("emulation.pp3"),
        &recipe.adjustments,
        recipe.sharpening,
    )? {
        rawtherapee_profiles.push(path);
    }
    Ok(ResolvedProfile {
        hald_path: Some(output),
        rawtherapee_profiles,
        grain: recipe.grain,
        resolved_stem: profile_stem_for_output(path),
    })
}

/// Find the RGBTable profile referenced by a preset recipe.
///
/// Presets often live near their referenced profiles, but users can also provide
/// a profiles root. The search tries the configured root, the preset directory,
/// and its parent, preferring exact Look UUID matches and falling back to Look
/// name matches that must contain a real RGBTable.
fn resolve_recipe_profile(
    recipe: &mini_film::XmpFilmRecipe,
    profiles_root: &Path,
    preset_path: &Path,
) -> Result<PathBuf> {
    for root in rgb_profile_roots(profiles_root, preset_path) {
        if let Some(uuid) = &recipe.look_uuid
            && let Some(path) = find_profile_by_uuid(&root, uuid)?
        {
            return Ok(path);
        }
        if let Some(name) = &recipe.look_name
            && let Some(path) = find_rgb_xmp_by_name(&root, name)?
        {
            return Ok(path);
        }
    }

    bail!("preset does not contain a resolvable RGB table or linked Look UUID/name")
}

fn cached_hald_path(source: &Path, hald_level: u32, hald_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(hald_dir).with_context(|| format!("creating {}", hald_dir.display()))?;
    let metadata = fs::metadata(source).with_context(|| format!("reading {}", source.display()))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hald_level.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    modified.hash(&mut hasher);
    let hash = hasher.finish();

    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|stem| sanitize_filename::sanitize(stem).into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "profile".to_string());
    Ok(hald_dir.join(format!("{stem}.l{hald_level}.{hash:016x}.hald.png")))
}

fn find_hald_by_name(root: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !root.exists() {
        return Ok(None);
    }
    ProfileNameIndex::build(root, &["png"], true, None)?.find_best(name)
}

fn find_xmp_by_name(root: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !root.exists() {
        return Ok(None);
    }
    ProfileNameIndex::build(root, &["xmp"], false, None)?.find_best(name)
}

/// Find an RGBTable-bearing XMP profile by name.
///
/// The first pass uses the generic XMP name matcher. If that resolves to a
/// preset rather than a profile, the second pass scans the tree for stems that
/// normalize to the requested name after removing common `profile` suffixes, and
/// validates candidates by parsing them for an embedded RGBTable.
fn find_rgb_xmp_by_name(root: &Path, name: &str) -> Result<Option<PathBuf>> {
    if let Some(path) =
        ProfileNameIndex::build(root, &["xmp"], false, Some(true))?.find_best(name)?
    {
        return Ok(Some(path));
    }

    let wanted = normalize_rgb_profile_name(name);
    for path in ProfileNameIndex::build(root, &["xmp"], false, Some(true))?.candidates {
        if normalize_rgb_profile_name(&path.normalized) == wanted {
            return Ok(Some(path.path));
        }
    }

    Ok(None)
}

#[derive(Clone)]
struct ProfileNameEntry {
    path: PathBuf,
    normalized: String,
}

struct ProfileNameIndex {
    candidates: Vec<ProfileNameEntry>,
}

impl ProfileNameIndex {
    fn build(
        root: &Path,
        extensions: &[&str],
        hald_png: bool,
        rgb_only: Option<bool>,
    ) -> Result<Self> {
        if !root.exists() {
            return Ok(Self {
                candidates: Vec::new(),
            });
        }

        let mut candidates = Vec::new();
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
                continue;
            };
            if !extensions
                .iter()
                .any(|candidate| ext.eq_ignore_ascii_case(candidate))
            {
                continue;
            }

            if let Some(expected_rgb_only) = rgb_only {
                let has_rgb_table = extract_film_recipe(path)
                    .ok()
                    .is_some_and(|recipe| recipe.rgb_table.is_some());
                if has_rgb_table != expected_rgb_only {
                    continue;
                }
            }

            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let normalized = normalize_profile_stem_for_index(stem, hald_png);
            if normalized.is_empty() {
                continue;
            }
            candidates.push(ProfileNameEntry {
                path: path.to_path_buf(),
                normalized,
            });
        }

        Ok(Self { candidates })
    }

    fn find_best(&self, selector: &str) -> Result<Option<PathBuf>> {
        if self.candidates.is_empty() {
            return Ok(None);
        }
        let wanted = normalize_name(selector);
        if wanted.is_empty() {
            return Ok(None);
        }

        let mut best: Option<(u32, &ProfileNameEntry)> = None;
        for candidate in &self.candidates {
            if candidate.normalized == wanted {
                return Ok(Some(candidate.path.clone()));
            }

            if let Some(score) = profile_name_distance_score(&candidate.normalized, &wanted) {
                if let Some((best_score, _)) = best
                    && score >= best_score
                {
                    continue;
                }
                best = Some((score, candidate));
            }
        }

        Ok(best.map(|(_, candidate)| candidate.path.clone()))
    }
}

fn profile_name_distance_score(candidate: &str, wanted: &str) -> Option<u32> {
    if wanted.is_empty() {
        return None;
    }

    let token_match = wants_token_subset_match(candidate, wanted);
    let contains_query =
        wanted.len() > 3 && (candidate.contains(wanted) || wanted.contains(candidate));
    let distance = levenshtein(candidate, wanted);
    let threshold = levenshtein_threshold(wanted.len());

    if token_match.full {
        return Some(length_delta_u32(candidate.len(), wanted.len()));
    }
    if token_match.partial && wanted.len() >= 4 && distance <= threshold.saturating_add(3) {
        return Some(120 + distance as u32 + length_delta_u32(candidate.len(), wanted.len()));
    }
    if contains_query && distance <= threshold.saturating_add(4) {
        return Some(240 + distance as u32 + length_delta_u32(candidate.len(), wanted.len()));
    }
    None
}

fn length_delta_u32(left: usize, right: usize) -> u32 {
    left.abs_diff(right) as u32
}

struct TokenMatch {
    full: bool,
    partial: bool,
}

fn wants_token_subset_match(candidate: &str, wanted: &str) -> TokenMatch {
    let wanted_tokens: Vec<_> = wanted
        .split_whitespace()
        .filter(|token| token_is_significant(token))
        .collect();
    if wanted_tokens.is_empty() {
        return TokenMatch {
            full: false,
            partial: false,
        };
    }
    let candidate_tokens: Vec<_> = candidate.split_whitespace().collect();

    let mut matched = 0usize;
    let mut partial_match = 0usize;
    for token in &wanted_tokens {
        let exact_match = candidate_tokens
            .iter()
            .any(|candidate_token| candidate_token == token);
        if exact_match {
            matched += 1;
            continue;
        }

        let contains = candidate_tokens.iter().any(|candidate_token| {
            candidate_token.contains(token) || token.contains(candidate_token)
        });
        if contains {
            partial_match += 1;
        }
    }
    TokenMatch {
        full: matched == wanted_tokens.len(),
        partial: partial_match > 0,
    }
}

fn token_is_significant(token: &str) -> bool {
    if token.len() >= 3 {
        return true;
    }
    if token.len() != 2 {
        return false;
    }
    let chars: Vec<_> = token.chars().collect();
    if chars.len() != 2 {
        return false;
    }
    (chars[0].eq_ignore_ascii_case(&'v') && chars[1].is_ascii_digit())
        || chars.iter().any(|character| character.is_ascii_digit())
}

fn levenshtein_threshold(length: usize) -> usize {
    match length {
        0 => 0,
        1 => 1,
        2 => 1,
        3..=5 => 2,
        6..=8 => 3,
        9..=12 => 4,
        13..=18 => 5,
        _ => 6,
    }
}

fn normalize_profile_stem_for_index(stem: &str, hald_png: bool) -> String {
    let mut candidate = normalize_name(stem);
    if hald_png {
        candidate = candidate.trim_end_matches("hald").trim().to_string();
    }
    candidate = candidate.trim_end_matches("profile").trim().to_string();
    candidate
}

fn levenshtein(left: &str, right: &str) -> usize {
    if left == right {
        return 0;
    }
    let left_len = left.len();
    let right_len = right.len();
    if left_len == 0 {
        return right_len;
    }
    if right_len == 0 {
        return left_len;
    }

    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let mut previous: Vec<usize> = (0..=right_len).collect();
    let mut current = vec![0usize; right_len + 1];

    for (i, &left_byte) in left_bytes.iter().enumerate() {
        current[0] = i + 1;
        for (j, &right_byte) in right_bytes.iter().enumerate() {
            let insertion = current[j] + 1;
            let deletion = previous[j + 1] + 1;
            let substitution = previous[j] + if left_byte == right_byte { 0 } else { 1 };
            current[j + 1] = insertion.min(deletion).min(substitution);
        }
        previous.clone_from_slice(&current);
    }
    previous[right_len]
}

fn looks_like_rgb_profile_selector(name: &str) -> bool {
    let normalized = normalize_name(name);
    normalized.ends_with("profile") || normalized.ends_with("profile xmp")
}

fn normalize_rgb_profile_name(name: &str) -> String {
    normalize_name(name)
        .trim_end_matches("xmp")
        .trim()
        .trim_end_matches("profile")
        .trim()
        .to_string()
}

/// Find an RGBTable-bearing profile whose XMP UUID matches a Look UUID.
///
/// The resolver walks XMP files recursively, ignores malformed files and presets
/// without RGB tables, and returns the first profile whose parsed UUID equals
/// the preset's linked Look UUID. This is the strongest preset-to-profile match
/// because display names can collide or vary by vendor packaging.
fn find_profile_by_uuid(root: &Path, uuid: &str) -> Result<Option<PathBuf>> {
    if !root.exists() {
        return Ok(None);
    }

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("xmp") {
            continue;
        }
        let Ok(recipe) = extract_film_recipe(path) else {
            continue;
        };
        if recipe.rgb_table.is_none() {
            continue;
        }
        if recipe.uuid.as_deref() == Some(uuid) {
            return Ok(Some(path.to_path_buf()));
        }
    }

    Ok(None)
}

fn profile_stem_for_output(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("profile")
        .to_string()
}

pub(crate) fn normalize_name(value: &str) -> String {
    let with_spaces = value
        .to_ascii_lowercase()
        .replace(['_', '-', '.', '/'], " ")
        .replace('\\', " ")
        .replace('+', " plus ");
    with_spaces.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn emulation_selector_roots(root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    push_unique_root(&mut roots, root.join("emulations"));
    if root
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("emulations"))
    {
        push_unique_root(&mut roots, root.to_path_buf());
    }
    if let Some(parent) = canonical_parent(root) {
        push_unique_root(&mut roots, parent.join("emulations"));
    }
    if roots.is_empty() {
        push_unique_root(&mut roots, root.to_path_buf());
    }
    roots
}

fn rgb_profile_roots(profiles_root: &Path, preset_path: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    push_unique_root(&mut roots, profiles_root.join("profiles"));
    if profiles_root
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("profiles"))
    {
        push_unique_root(&mut roots, profiles_root.to_path_buf());
    }

    if let Some(parent) = preset_path.parent() {
        if let Some(layout_root) = parent.parent() {
            push_unique_root(&mut roots, layout_root.join("profiles"));
        }
        push_unique_root(&mut roots, parent.join("profiles"));
    }

    if let Some(parent) = canonical_parent(profiles_root) {
        push_unique_root(&mut roots, parent.join("profiles"));
    }
    if roots.is_empty() {
        push_unique_root(&mut roots, profiles_root.to_path_buf());
    }

    roots
}

fn canonical_parent(path: &Path) -> Option<PathBuf> {
    path.canonicalize()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn push_unique_root(roots: &mut Vec<PathBuf>, root: PathBuf) {
    if !root.exists() {
        return;
    }
    if !roots.iter().any(|candidate| candidate == &root) {
        roots.push(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{ExportOptions, JpegSubsampling};

    fn apply_args(profile: String, profiles_root: PathBuf, hald_dir: PathBuf) -> ApplyArgs {
        ApplyArgs {
            raw: PathBuf::from("input.dng"),
            output: PathBuf::from("output.jpg"),
            profile,
            hald_dir,
            profiles_root,
            hald_level: 16,
            rawtherapee: PathBuf::from("rawtherapee-cli"),
            convert: PathBuf::from("convert"),
            keep_intermediate: None,
            no_grain: false,
            color_noise_iso_threshold: 1600,
            grain: None,
            grain_preset: None,
            grain_seed: None,
            export: ExportOptions {
                jpg_quality: 95,
                resize: None,
                long_edge: None,
                max_width: None,
                max_height: None,
                jpeg_subsampling: JpegSubsampling::S444,
                strip_metadata: false,
                progressive_jpeg: false,
            },
        }
    }

    #[test]
    fn normalize_name_removes_case_punctuation_and_extra_spaces() {
        assert_eq!(
            normalize_name(" Kodak_Portra-400.profile "),
            "kodak portra 400 profile"
        );
    }

    #[test]
    fn normalize_name_keeps_plus_as_word() {
        assert_eq!(normalize_name("Scala + grainy"), "scala plus grainy");
        assert_eq!(normalize_name("Scala plus grainy"), "scala plus grainy");
    }

    #[test]
    fn full_token_match_scores_scala_plus_grainy() {
        let candidate = normalize_name("Agfa Scala 200 + grainy");
        let wanted = normalize_name("scala + grainy");
        assert!(wants_token_subset_match(&candidate, &wanted).full);
        assert!(profile_name_distance_score(&candidate, &wanted).is_some());
    }

    #[test]
    fn short_version_token_is_significant_in_fuzzy_match() {
        let candidate = normalize_name("Fuji Superia 200 v6 grainy");
        let wanted = normalize_name("superia 200 v6 grainy");
        let token_match = wants_token_subset_match(&candidate, &wanted);
        assert!(token_match.full);

        let non_version = normalize_name("Fuji Superia 200 grainy");
        let non_version_match = wants_token_subset_match(&non_version, &wanted);
        assert!(!non_version_match.full);
        assert!(!non_version_match.partial);
    }

    #[test]
    fn rgb_profile_selector_detection_handles_profile_suffixes() {
        assert!(looks_like_rgb_profile_selector("Polaroid 600 profile"));
        assert!(looks_like_rgb_profile_selector("Polaroid 600 profile.xmp"));
        assert_eq!(
            normalize_rgb_profile_name("Polaroid 600 profile.xmp"),
            "polaroid 600"
        );
        assert!(!looks_like_rgb_profile_selector("Polaroid 600 grainy"));
    }

    #[test]
    fn push_unique_root_ignores_missing_and_duplicate_roots() {
        let dir = tempfile::tempdir().unwrap();
        let mut roots = Vec::new();
        push_unique_root(&mut roots, dir.path().join("missing"));
        assert!(roots.is_empty());
        push_unique_root(&mut roots, dir.path().to_path_buf());
        push_unique_root(&mut roots, dir.path().to_path_buf());
        assert_eq!(roots, vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn pp3_path_resolves_as_rawtherapee_only_profile() {
        let dir = tempfile::tempdir().unwrap();
        let pp3 = dir.path().join("edited.pp3");
        std::fs::write(&pp3, "[Exposure]\nCompensation=1\n").unwrap();
        let args = apply_args(
            pp3.display().to_string(),
            dir.path().to_path_buf(),
            dir.path().join("hald"),
        );

        let resolved = resolve_profile(&args, dir.path()).unwrap();
        assert!(resolved.hald_path.is_none());
        assert_eq!(resolved.rawtherapee_profiles, vec![pp3]);
        assert!(!resolved.grain.is_enabled());
    }

    #[test]
    fn file_url_profile_path_resolves_as_rawtherapee_only_profile() {
        let dir = tempfile::tempdir().unwrap();
        let pp3 = dir.path().join("edited profile.pp3");
        std::fs::write(&pp3, "[Exposure]\nCompensation=1\n").unwrap();
        let selector = format!("file://{}", pp3.display()).replace(' ', "%20");
        let args = apply_args(selector, dir.path().to_path_buf(), dir.path().join("hald"));

        let resolved = resolve_profile(&args, dir.path()).unwrap();
        assert!(resolved.hald_path.is_none());
        assert_eq!(resolved.rawtherapee_profiles, vec![pp3]);
        assert!(!resolved.grain.is_enabled());
    }

    #[test]
    fn file_url_profile_paths_accept_localhost_and_decode_escapes() {
        assert_eq!(
            local_file_url_to_path("file:///tmp/RNI%20Films/look.pp3")
                .unwrap()
                .unwrap(),
            PathBuf::from("/tmp/RNI Films/look.pp3")
        );
        assert_eq!(
            local_file_url_to_path("file://localhost/tmp/RNI%20Films/look.pp3")
                .unwrap()
                .unwrap(),
            PathBuf::from("/tmp/RNI Films/look.pp3")
        );
        assert!(local_file_url_to_path("file://example.com/tmp/look.pp3").is_err());
        assert!(local_file_url_to_path("file:///tmp/look.pp3?download=1").is_err());
        assert!(local_file_url_to_path("look.pp3").unwrap().is_none());
    }

    #[test]
    fn inspect_profile_identifies_direct_pp3_paths() {
        let dir = tempfile::tempdir().unwrap();
        let pp3 = dir.path().join("edited.pp3");
        std::fs::write(&pp3, "[Exposure]\nCompensation=1\n").unwrap();

        match inspect_profile(
            &pp3.display().to_string(),
            dir.path(),
            &dir.path().join("hald"),
            8,
        )
        .unwrap()
        {
            ProfileInfo::RawTherapeePp3 { path } => assert_eq!(path, pp3),
            _ => panic!("expected pp3 profile info"),
        }
    }

    #[test]
    fn inspect_profile_resolves_file_url_as_path() {
        let dir = tempfile::tempdir().unwrap();
        let pp3 = dir.path().join("edited profile.pp3");
        std::fs::write(&pp3, "[Exposure]\nCompensation=1\n").unwrap();
        let selector = format!("file://{}", pp3.display()).replace(' ', "%20");

        match inspect_profile(&selector, dir.path(), &dir.path().join("hald"), 8).unwrap() {
            ProfileInfo::RawTherapeePp3 { path } => assert_eq!(path, pp3),
            _ => panic!("expected pp3 profile info"),
        }
    }

    #[test]
    fn inspect_profile_fuzzy_hald_lookup_by_profile_name() {
        let dir = tempfile::tempdir().unwrap();
        let hald_dir = dir.path().join("hald");
        std::fs::create_dir_all(&hald_dir).unwrap();
        let hald = hald_dir.join("Agfa Scala 200 profile.l16.a1.hald.png");
        std::fs::write(&hald, b"x").unwrap();

        match inspect_profile("agfa scala", dir.path(), &hald_dir, 16).unwrap() {
            ProfileInfo::HaldPng { path } => assert_eq!(path, hald),
            _ => panic!("expected hald profile info"),
        }
    }
}
