use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use mini_film::{
    GrainSettings, HaldOptions, SharpeningSettings, convert_xmp_to_hald, extract_film_recipe,
    profile_info_line,
};
use walkdir::WalkDir;

use crate::app::apply::ApplyArgs;

pub(crate) struct ResolvedProfile {
    pub(crate) hald_path: PathBuf,
    pub(crate) grain: GrainSettings,
    pub(crate) sharpening: SharpeningSettings,
}

/// Resolve a CLI profile selector into a concrete Hald file plus recipe metadata.
///
/// The selector can be a real path, an XMP profile/preset name under
/// `profiles_root`, or a generated Hald name under `hald_dir`. XMP inputs may
/// generate a temporary Hald and preserve grain/sharpening metadata; raw PNG
/// Hald inputs have no attached recipe metadata, so they resolve with defaults.
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

    if let Some(path) = find_xmp_by_name(&args.profiles_root, &args.profile)? {
        return profile_from_path(&path, args.hald_level, &args.profiles_root, temp_dir);
    }

    if let Some(path) = find_hald_by_name(&args.hald_dir, &args.profile)? {
        return Ok(ResolvedProfile {
            hald_path: path,
            grain: GrainSettings::default(),
            sharpening: SharpeningSettings::default(),
        });
    }

    bail!(
        "could not resolve profile {:?} as a file, XMP name under {}, or Hald name under {}",
        args.profile,
        args.profiles_root.display(),
        args.hald_dir.display()
    );
}

/// Resolve an explicit profile path by extension.
///
/// PNG files are already usable Hald CLUTs and are returned directly. XMP files
/// may be RGBTable profiles or presets that point at a Look, so they are sent
/// through the XMP resolver. Other extensions are rejected early to make command
/// failures clearer.
fn profile_from_path(
    path: &Path,
    hald_level: u32,
    profiles_root: &Path,
    temp_dir: &Path,
) -> Result<ResolvedProfile> {
    match path.extension().and_then(|s| s.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("png") => Ok(ResolvedProfile {
            hald_path: path.to_path_buf(),
            grain: GrainSettings::default(),
            sharpening: SharpeningSettings::default(),
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
/// If the XMP embeds an RGBTable it is the source profile. If it is a preset,
/// the linked Look is resolved by UUID/name before conversion. The generated
/// Hald is written into the caller's temp directory, grain comes from the preset
/// recipe, and sharpening comes from the converted source profile metadata.
fn profile_from_xmp(
    path: &Path,
    hald_level: u32,
    profiles_root: &Path,
    temp_dir: &Path,
) -> Result<ResolvedProfile> {
    let recipe = extract_film_recipe(path)?;
    let source = if recipe.rgb_table.is_some() {
        path.to_path_buf()
    } else {
        resolve_recipe_profile(&recipe, profiles_root, path)
            .with_context(|| format!("resolving linked profile for preset {}", path.display()))?
    };

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
    eprintln!("{}", profile_info_line(&converted));
    Ok(ResolvedProfile {
        hald_path: output,
        grain: recipe.grain,
        sharpening: converted.sharpening,
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
    let mut roots = vec![profiles_root.to_path_buf()];
    if let Some(parent) = preset_path.parent() {
        roots.push(parent.to_path_buf());
        if let Some(grandparent) = parent.parent() {
            roots.push(grandparent.to_path_buf());
        }
    }

    for root in roots {
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
