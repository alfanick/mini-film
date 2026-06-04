use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mini_film::{
    GrainSettings, HaldOptions, convert_xmp_to_hald, extract_film_recipe, profile_info_line,
    rawtherapee_hald_clut_profile_text, write_rawtherapee_profile,
};
use walkdir::WalkDir;

use crate::app::apply::ApplyArgs;

pub(crate) struct ResolvedProfile {
    pub(crate) hald_path: PathBuf,
    pub(crate) rawtherapee_profiles: Vec<PathBuf>,
    pub(crate) grain: GrainSettings,
}

pub(crate) fn rawtherapee_profiles_with_hald(
    resolved: &ResolvedProfile,
    temp_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let lut_profile = temp_dir.join("rt-hald-clut.pp3");
    std::fs::write(
        &lut_profile,
        rawtherapee_hald_clut_profile_text(&resolved.hald_path),
    )
    .with_context(|| format!("writing {}", lut_profile.display()))?;

    let mut profiles = resolved.rawtherapee_profiles.clone();
    profiles.push(lut_profile);
    Ok(profiles)
}

/// Resolve a CLI profile selector into a concrete Hald file plus recipe metadata.
///
/// The selector can be a real path, an emulation XMP name under `emulations/`,
/// or a generated Hald name under `hald_dir`. Emulation XMP inputs generate a
/// temporary Hald from their linked internal RGBTable profile and may generate
/// RawTherapee `.pp3` files for tone/color/sharpening metadata; raw PNG Hald
/// inputs have no attached recipe metadata, so they resolve with defaults.
pub(crate) fn resolve_profile(args: &ApplyArgs, temp_dir: &Path) -> Result<ResolvedProfile> {
    let selector_path = Path::new(&args.profile);
    if selector_path.exists() {
        return profile_from_path(
            selector_path,
            args.hald_level,
            &args.profiles_root,
            temp_dir,
        );
    }

    for root in emulation_selector_roots(&args.profiles_root) {
        if let Some(path) = find_xmp_by_name(&root, &args.profile)? {
            return profile_from_path(&path, args.hald_level, &args.profiles_root, temp_dir);
        }
    }

    if let Some(path) = find_hald_by_name(&args.hald_dir, &args.profile)? {
        return Ok(ResolvedProfile {
            hald_path: path,
            rawtherapee_profiles: Vec::new(),
            grain: GrainSettings::default(),
        });
    }

    bail!(
        "could not resolve profile {:?} as a file, emulation XMP name under {}, or Hald name under {}",
        args.profile,
        args.profiles_root.display(),
        args.hald_dir.display()
    );
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
    temp_dir: &Path,
) -> Result<ResolvedProfile> {
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("png") => Ok(ResolvedProfile {
            hald_path: path.to_path_buf(),
            rawtherapee_profiles: Vec::new(),
            grain: GrainSettings::default(),
        }),
        Some(ext) if ext.eq_ignore_ascii_case("xmp") => {
            profile_from_xmp(path, hald_level, profiles_root, temp_dir)
        }
        Some(ext) => bail!("unsupported profile path extension .{ext}; expected .png or .xmp"),
        None => bail!("profile path has no extension: {}", path.display()),
    }
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
    temp_dir: &Path,
) -> Result<ResolvedProfile> {
    profile_from_xmp_inner(path, hald_level, profiles_root, temp_dir, true)
}

pub(crate) fn profile_from_xmp_quiet(
    path: &Path,
    hald_level: u32,
    profiles_root: &Path,
    temp_dir: &Path,
) -> Result<ResolvedProfile> {
    profile_from_xmp_inner(path, hald_level, profiles_root, temp_dir, false)
}

fn profile_from_xmp_inner(
    path: &Path,
    hald_level: u32,
    profiles_root: &Path,
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

    let output = temp_dir.join("profile.hald.png");
    let converted = convert_xmp_to_hald(
        &source,
        &output,
        HaldOptions {
            hald_level,
            overwrite: true,
            info_only: false,
        },
    )?;
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
        hald_path: output,
        rawtherapee_profiles,
        grain: recipe.grain,
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
        if let Some(uuid) = &recipe.look_uuid {
            if let Some(path) = find_profile_by_uuid(&root, uuid)? {
                return Ok(path);
            }
        }
        if let Some(name) = &recipe.look_name {
            if let Some(path) = find_rgb_xmp_by_name(&root, name)? {
                return Ok(path);
            }
        }
    }

    bail!("preset does not contain a resolvable RGB table or linked Look UUID/name")
}

fn find_hald_by_name(root: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !root.exists() {
        return Ok(None);
    }
    find_named_file(root, name, &["png"], true)
}

fn find_xmp_by_name(root: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !root.exists() {
        return Ok(None);
    }
    find_named_file(root, name, &["xmp"], false)
}

/// Find an RGBTable-bearing XMP profile by name.
///
/// The first pass uses the generic XMP name matcher. If that resolves to a
/// preset rather than a profile, the second pass scans the tree for stems that
/// normalize to the requested name after removing common `profile` suffixes, and
/// validates candidates by parsing them for an embedded RGBTable.
fn find_rgb_xmp_by_name(root: &Path, name: &str) -> Result<Option<PathBuf>> {
    let Some(candidate) = find_xmp_by_name(root, name)? else {
        return Ok(None);
    };
    if extract_film_recipe(&candidate)
        .map(|recipe| recipe.rgb_table.is_some())
        .unwrap_or(false)
    {
        return Ok(Some(candidate));
    }

    let wanted = normalize_name(name);
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("xmp") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let candidate_name = normalize_name(stem)
            .trim_end_matches("profile")
            .trim()
            .to_string();
        if candidate_name != wanted {
            continue;
        }
        if extract_film_recipe(path)
            .map(|recipe| recipe.rgb_table.is_some())
            .unwrap_or(false)
        {
            return Ok(Some(path.to_path_buf()));
        }
    }

    Ok(None)
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

/// Find a named file using normalized exact match with one fuzzy fallback.
///
/// Matching lowercases names, treats `_`, `-`, and `.` as spaces, and removes
/// generated suffixes such as `hald` or `profile` before comparison. Exact
/// normalized matches win; otherwise the first candidate containing the wanted
/// normalized text is returned to support ergonomic profile-name selectors.
fn find_named_file(
    root: &Path,
    name: &str,
    extensions: &[&str],
    hald_png: bool,
) -> Result<Option<PathBuf>> {
    let wanted = normalize_name(name);
    let mut fuzzy_match = None;

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        if !extensions
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let mut candidate = normalize_name(stem);
        if hald_png {
            candidate = candidate.trim_end_matches("hald").trim().to_string();
        }
        candidate = candidate.trim_end_matches("profile").trim().to_string();

        if candidate == wanted {
            return Ok(Some(path.to_path_buf()));
        }
        if fuzzy_match.is_none() && candidate.contains(&wanted) {
            fuzzy_match = Some(path.to_path_buf());
        }
    }

    Ok(fuzzy_match)
}

pub(crate) fn normalize_name(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .replace(['_', '-', '.'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
