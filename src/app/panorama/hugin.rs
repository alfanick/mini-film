use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};

use super::tools::HuginToolchain;
use crate::{
    app::export::add_convert_thread_limit_with_count,
    cli::{PanoramaMatchingMode, PanoramaProjection},
};

pub(crate) fn align_project(
    tools: &HuginToolchain,
    inputs: &[PathBuf],
    work_dir: &Path,
    matching: PanoramaMatchingMode,
    jobs: usize,
) -> Result<PathBuf> {
    let aligned = work_dir.join(format!("aligned-{matching}.pto"));
    if aligned.is_file() {
        return Ok(aligned);
    }
    fs::create_dir_all(work_dir).with_context(|| format!("creating {}", work_dir.display()))?;
    let generated = work_dir.join("generated.pto");
    let control_points = work_dir.join(format!("control-{matching}.pto"));
    let clean = work_dir.join(format!("clean-{matching}.pto"));

    if !generated.is_file() {
        let mut args = vec![OsString::from("-o"), generated.as_os_str().to_owned()];
        args.extend(inputs.iter().map(|path| path.as_os_str().to_owned()));
        tools.run("pto_gen", tools.pto_gen(), args, jobs)?;
    }
    if !control_points.is_file() {
        let mut args = matching_args(matching);
        args.extend([
            OsString::from("-o"),
            control_points.as_os_str().to_owned(),
            generated.as_os_str().to_owned(),
        ]);
        tools.run("cpfind", tools.cpfind(), args, jobs)?;
    }
    if !clean.is_file() {
        tools.run(
            "cpclean",
            tools.cpclean(),
            [
                OsString::from("-o"),
                clean.as_os_str().to_owned(),
                control_points.as_os_str().to_owned(),
            ],
            jobs,
        )?;
    }
    let optimisation_input = if matching == PanoramaMatchingMode::FlatMosaic {
        let mosaic = work_dir.join("mosaic-variables.pto");
        if !mosaic.is_file() {
            tools.run(
                "pto_var",
                tools.pto_var(),
                [
                    OsString::from("--output"),
                    mosaic.as_os_str().to_owned(),
                    OsString::from("--opt=TrX,TrY,TrZ,r,!TrX0,!TrY0,!TrZ0,!r0"),
                    clean.as_os_str().to_owned(),
                ],
                jobs,
            )?;
        }
        mosaic
    } else {
        clean
    };
    let mut optimiser_args = match matching {
        PanoramaMatchingMode::FlatMosaic => vec![OsString::from("-n"), OsString::from("-m")],
        _ => vec![
            OsString::from("-a"),
            OsString::from("-m"),
            OsString::from("-l"),
        ],
    };
    optimiser_args.extend([
        OsString::from("-o"),
        aligned.as_os_str().to_owned(),
        optimisation_input.as_os_str().to_owned(),
    ]);
    tools.run("autooptimiser", tools.autooptimiser(), optimiser_args, jobs)?;
    require_file(&aligned, "optimized Hugin project")?;
    Ok(aligned)
}

pub(crate) struct StitchProjectionOptions<'a> {
    pub(crate) aligned: &'a Path,
    pub(crate) work_dir: &'a Path,
    pub(crate) projection: PanoramaProjection,
    pub(crate) output: &'a Path,
    pub(crate) preview: bool,
}

pub(crate) fn stitch_projection(
    tools: &HuginToolchain,
    convert: &Path,
    options: StitchProjectionOptions<'_>,
    jobs: usize,
) -> Result<()> {
    let StitchProjectionOptions {
        aligned,
        work_dir,
        projection,
        output,
        preview,
    } = options;
    if output.is_file() {
        return Ok(());
    }
    let projected = work_dir.join(format!(
        "projected-{projection}-{}.pto",
        if preview { "preview" } else { "full" }
    ));
    let prefix = work_dir.join(format!(
        "stitched-{projection}-{}",
        if preview { "preview" } else { "full" }
    ));
    let file_type = if preview { "JPG" } else { "TIF" };
    let compression = if preview { "88" } else { "DEFLATE" };
    tools.run(
        "pano_modify",
        tools.pano_modify(),
        [
            OsString::from(format!("--projection={}", projection.hugin_id())),
            OsString::from("--fov=AUTO"),
            OsString::from("--canvas=AUTO"),
            OsString::from("--crop=AUTO"),
            OsString::from("--output-exposure=AUTO"),
            OsString::from("--output-type=N"),
            OsString::from(format!("--ldr-file={file_type}")),
            OsString::from(format!("--ldr-compression={compression}")),
            OsString::from("--output"),
            projected.as_os_str().to_owned(),
            aligned.as_os_str().to_owned(),
        ],
        jobs,
    )?;
    tools.run(
        "hugin_executor",
        tools.hugin_executor(),
        [
            OsString::from("--stitching"),
            OsString::from(format!("--threads={}", jobs.max(1))),
            OsString::from(format!("--prefix={}", prefix.display())),
            projected.as_os_str().to_owned(),
        ],
        jobs,
    )?;

    let stitched = stitched_output_path(&prefix, preview)?;
    let parent = output
        .parent()
        .with_context(|| format!("panorama output has no parent: {}", output.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let mut command = Command::new(convert);
    add_convert_thread_limit_with_count(&mut command, convert, jobs);
    command.arg(&stitched);
    if preview {
        command
            .arg("-filter")
            .arg("Triangle")
            .arg("-resize")
            .arg("2048x2048>")
            .arg("-interlace")
            .arg("Line")
            .arg("-depth")
            .arg("8")
            .arg("-quality")
            .arg("88");
    } else {
        command.arg("-depth").arg("16").arg("-compress").arg("Zip");
    }
    let result = command
        .arg(output)
        .output()
        .with_context(|| format!("finalizing stitched panorama with {}", convert.display()))?;
    if !result.status.success() {
        bail!(
            "panorama finalization failed with status {}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
    }
    require_file(output, "stitched panorama")
}

pub(crate) fn matching_args(mode: PanoramaMatchingMode) -> Vec<OsString> {
    match mode {
        PanoramaMatchingMode::Automatic => vec![
            OsString::from("--multirow"),
            OsString::from("--ransacmode=auto"),
        ],
        PanoramaMatchingMode::Sequential => vec![
            OsString::from("--linearmatch"),
            OsString::from("--linearmatchlen=2"),
            OsString::from("--ransacmode=rpy"),
        ],
        PanoramaMatchingMode::MultiRow => vec![
            OsString::from("--multirow"),
            OsString::from("--ransacmode=rpy"),
        ],
        PanoramaMatchingMode::FlatMosaic => vec![
            OsString::from("--allpairs"),
            OsString::from("--ransacmode=hom"),
        ],
    }
}

fn stitched_output_path(prefix: &Path, preview: bool) -> Result<PathBuf> {
    let extensions: &[&str] = if preview {
        &["jpg", "jpeg"]
    } else {
        &["tif", "tiff"]
    };
    for extension in extensions {
        let candidate = prefix.with_extension(extension);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    let parent = prefix.parent().unwrap_or_else(|| Path::new("."));
    let stem = prefix
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let mut candidates = fs::read_dir(parent)
        .with_context(|| format!("reading Hugin output directory {}", parent.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_stem()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(stem))
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        extensions
                            .iter()
                            .any(|expected| extension.eq_ignore_ascii_case(expected))
                    })
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.into_iter().next().with_context(|| {
        format!(
            "Hugin finished without creating a {} output for {}",
            extensions.join("/"),
            prefix.display()
        )
    })
}

fn require_file(path: &Path, description: &str) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        bail!("{description} was not created: {}", path.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn matching_modes_use_stable_hugin_recipes() {
        assert_eq!(
            strings(matching_args(PanoramaMatchingMode::Automatic)),
            ["--multirow", "--ransacmode=auto"]
        );
        assert_eq!(
            strings(matching_args(PanoramaMatchingMode::Sequential)),
            ["--linearmatch", "--linearmatchlen=2", "--ransacmode=rpy"]
        );
        assert_eq!(
            strings(matching_args(PanoramaMatchingMode::MultiRow)),
            ["--multirow", "--ransacmode=rpy"]
        );
        assert_eq!(
            strings(matching_args(PanoramaMatchingMode::FlatMosaic)),
            ["--allpairs", "--ransacmode=hom"]
        );
    }

    #[test]
    fn core_projection_ids_match_hugin() {
        assert_eq!(PanoramaProjection::Rectilinear.hugin_id(), 0);
        assert_eq!(PanoramaProjection::Cylindrical.hugin_id(), 1);
        assert_eq!(PanoramaProjection::Equirectangular.hugin_id(), 2);
        assert_eq!(PanoramaProjection::Panini.hugin_id(), 19);
    }

    #[test]
    fn flat_mosaic_uses_homography_control_points() {
        assert_eq!(
            strings(matching_args(PanoramaMatchingMode::FlatMosaic)),
            ["--allpairs", "--ransacmode=hom"]
        );
    }
}
