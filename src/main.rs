mod app;
mod cli;
mod updater;
mod util;

use std::{
    env,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;

use crate::app::apply::{ApplyArgs, run_apply};
use crate::app::batch::{BatchArgs, run_batch};
use crate::app::batch_daemon::{BatchDaemonArgs, run_batch_daemon};
use crate::app::desktop::run_desktop_app;
use crate::app::info::{InfoArgs, run_info};
use crate::app::nikon::{NikonArgs, run_nikon};
use crate::app::pp3::{Pp3Args, run_pp3};
use crate::app::review::{ReviewPublishCommandArgs, run_review_publish};
use crate::app::run_hald;
use crate::app::run_update;
use crate::app::sampler::{SamplerArgs, run_sampler};
use crate::app::util::{
    InputFileFilter, configure_threads, default_hald_dir, half_cpu_thread_count,
};
use crate::cli::{Cli, CommandKind, ExportOptions};

/// Parse CLI arguments and dispatch to the selected mini-film workflow.
///
/// The top-level binary keeps clap-generated command shapes separate from the
/// runtime structs used by the application modules. It initializes the Rayon
/// thread pool once, then maps shared apply/batch flags into `ExportOptions` so
/// the downstream pipeline can handle single-file and batch processing through
/// the same conversion/export code.
fn main() -> Result<()> {
    configure_threads();

    let args = args_with_default_command(env::args().collect::<Vec<_>>());
    startup_dependency_check(&args)?;
    let cli = Cli::parse_from(&args);

    match cli.command {
        CommandKind::App => run_desktop_app(),
        CommandKind::Hald {
            input,
            output,
            hald_level,
            overwrite,
            info_only,
        } => run_hald(
            &input,
            &output.unwrap_or_else(default_hald_dir),
            hald_level,
            overwrite,
            info_only,
        ),
        CommandKind::Info {
            profile,
            profiles_root,
            hald_dir,
            hald_level,
        } => run_info(InfoArgs {
            profile,
            profiles_root: resolve_profiles_root(profiles_root),
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            hald_level,
        }),
        CommandKind::Pp3 {
            profile,
            output,
            profiles_root,
            hald_dir,
            hald_level,
        } => run_pp3(Pp3Args {
            profile,
            output,
            profiles_root: resolve_profiles_root(profiles_root),
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            hald_level,
        }),
        CommandKind::Nikon {
            profile,
            output,
            report,
            name,
            profiles_root,
            hald_dir,
            hald_level,
        } => run_nikon(NikonArgs {
            profile,
            output,
            report,
            name,
            profiles_root: resolve_profiles_root(profiles_root),
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            hald_level,
        }),
        CommandKind::Apply {
            raw,
            output,
            profile,
            hald_dir,
            profiles_root,
            hald_level,
            rawtherapee,
            convert,
            keep_intermediate,
            no_grain,
            color_noise_iso_threshold,
            lens_corrections,
            lcp_root,
            grain,
            grain_preset,
            grain_seed,
            grain_engine,
            jpg_quality,
            resize,
            long_edge,
            max_width,
            max_height,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        } => run_apply(ApplyArgs {
            raw,
            output,
            profile,
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            profiles_root: resolve_profiles_root(profiles_root),
            hald_level,
            rawtherapee,
            convert,
            keep_intermediate,
            no_grain,
            lens_corrections: lens_corrections.unwrap_or_default(),
            lcp_root: resolve_lcp_root(lcp_root),
            color_noise_iso_threshold,
            grain,
            grain_preset,
            grain_seed,
            grain_engine,
            export: ExportOptions {
                jpg_quality,
                resize,
                long_edge,
                max_width,
                max_height,
                jpeg_subsampling,
                strip_metadata,
                progressive_jpeg,
            },
            retouch: None,
            bw_filter: crate::app::retouch::BwFilter::None,
        }),
        CommandKind::Batch {
            input,
            output,
            profile,
            hald_dir,
            profiles_root,
            hald_level,
            rawtherapee,
            convert,
            no_grain,
            color_noise_iso_threshold,
            lens_corrections,
            lcp_root,
            grain,
            grain_preset,
            grain_seed,
            grain_engine,
            jobs,
            input_jpg_only,
            input_raw_only,
            output_format,
            gallery,
            gallery_thumbnail_long_edge,
            gallery_columns,
            jpg_quality,
            resize,
            long_edge,
            max_width,
            max_height,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        } => run_batch(BatchArgs {
            input,
            output,
            profile,
            input_file_filter: input_file_filter(input_jpg_only, input_raw_only),
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            profiles_root: resolve_profiles_root(profiles_root),
            hald_level,
            rawtherapee,
            convert,
            no_grain,
            lens_corrections: lens_corrections.unwrap_or_default(),
            lcp_root: resolve_lcp_root(lcp_root),
            color_noise_iso_threshold,
            grain,
            grain_preset,
            grain_seed,
            grain_engine,
            jobs,
            output_format,
            gallery,
            gallery_thumbnail_long_edge,
            gallery_columns,
            export: ExportOptions {
                jpg_quality,
                resize,
                long_edge,
                max_width,
                max_height,
                jpeg_subsampling,
                strip_metadata,
                progressive_jpeg,
            },
        }),
        CommandKind::BatchDaemon {
            input,
            output,
            profile,
            hald_dir,
            profiles_root,
            hald_level,
            rawtherapee,
            convert,
            no_grain,
            color_noise_iso_threshold,
            lens_corrections,
            lcp_root,
            grain,
            grain_preset,
            grain_seed,
            grain_engine,
            jobs,
            input_jpg_only,
            input_raw_only,
            debounce_seconds,
            nikon_wtu,
            nikon_wtu_port,
            nikon_wtu_name,
            nikon_wtu_guid,
            review_address,
            codex,
            codex_binary,
            codex_model,
            codex_timeout,
            gallery,
            gallery_thumbnail_long_edge,
            gallery_columns,
            publish_album,
            output_format,
            jpg_quality,
            resize,
            long_edge,
            max_width,
            max_height,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        } => run_batch_daemon(BatchDaemonArgs {
            input,
            output,
            profile,
            input_file_filter: input_file_filter(input_jpg_only, input_raw_only),
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            profiles_root: resolve_profiles_root(profiles_root),
            hald_level,
            rawtherapee,
            convert,
            no_grain,
            lens_corrections: lens_corrections.unwrap_or_default(),
            lcp_root: resolve_lcp_root(lcp_root),
            color_noise_iso_threshold,
            grain,
            grain_preset,
            grain_seed,
            grain_engine,
            jobs,
            debounce_seconds,
            nikon_wtu,
            nikon_wtu_port,
            nikon_wtu_name,
            nikon_wtu_guid,
            review_address,
            codex: codex.filter(|flags| flags.is_enabled()),
            codex_binary,
            codex_model,
            codex_timeout,
            gallery,
            gallery_thumbnail_long_edge,
            gallery_columns,
            publish_album,
            output_format,
            export: ExportOptions {
                jpg_quality,
                resize,
                long_edge,
                max_width,
                max_height,
                jpeg_subsampling,
                strip_metadata,
                progressive_jpeg,
            },
            invocation: Some(format_daemon_invocation(&args)),
        }),
        CommandKind::Sampler {
            raw,
            output,
            profiles_root,
            hald_dir,
            hald_level,
            rawtherapee,
            convert,
            montage: _,
            no_grain,
            color_noise_iso_threshold,
            lens_corrections,
            lcp_root,
            grain_seed,
            grain_engine,
            no_cache,
            jobs,
            thumbnail_long_edge,
            columns,
            jpg_quality,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        } => run_sampler(SamplerArgs {
            raw,
            output,
            profiles_root: resolve_profiles_root(profiles_root),
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            hald_level,
            rawtherapee,
            convert,
            no_grain,
            lens_corrections: lens_corrections.unwrap_or_default(),
            lcp_root: resolve_lcp_root(lcp_root),
            color_noise_iso_threshold,
            grain_seed,
            grain_engine,
            no_cache,
            jobs,
            thumbnail_long_edge,
            columns,
            jpg_quality,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
        }),
        CommandKind::ReviewPublish {
            state,
            input_root,
            output_root,
            album,
            min_rating,
            label,
            tag,
            output_format,
            hald_dir,
            profiles_root,
            hald_level,
            rawtherapee,
            convert,
            jobs,
            gallery,
            gallery_thumbnail_long_edge,
            gallery_columns,
            jpg_quality,
            resize,
            long_edge,
            max_width,
            max_height,
            jpeg_subsampling,
            strip_metadata,
            progressive_jpeg,
            rerender_raw,
            no_grain,
            color_noise_iso_threshold,
            lens_corrections,
            lcp_root,
            grain,
            grain_preset,
            grain_seed,
            grain_engine,
            progress_events,
        } => run_review_publish(ReviewPublishCommandArgs {
            state,
            input_root,
            output_root,
            album,
            min_rating,
            labels: label,
            tags: tag,
            output_format,
            hald_dir: hald_dir.unwrap_or_else(default_hald_dir),
            profiles_root: resolve_profiles_root(profiles_root),
            hald_level,
            rawtherapee,
            convert,
            lcp_root: resolve_lcp_root(lcp_root),
            jobs: jobs.unwrap_or_else(half_cpu_thread_count),
            gallery,
            gallery_thumbnail_long_edge,
            gallery_columns,
            export: ExportOptions {
                jpg_quality,
                resize,
                long_edge,
                max_width,
                max_height,
                jpeg_subsampling,
                strip_metadata,
                progressive_jpeg,
            },
            rerender_raw,
            no_grain,
            color_noise_iso_threshold,
            lens_corrections: lens_corrections.unwrap_or_default(),
            grain,
            grain_preset,
            grain_seed,
            grain_engine,
            progress_events,
        }),
        CommandKind::Update => run_update(),
    }
}

fn args_with_default_command(mut args: Vec<String>) -> Vec<String> {
    if args.len() == 1 {
        args.push("app".to_string());
    }
    args
}

fn format_daemon_invocation(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote_command_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_command_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || "-._/:".contains(ch)
            || ch == ' '
            || ch == '\\'
            || ch == '.'
            || ch == ','
            || ch == '_'
            || ch == '-'
    }) {
        if arg.contains(' ') {
            return format!("\"{}\"", arg.replace('\"', "\\\""));
        }
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('\\', "\\\\").replace('\"', "\\\""))
}

const RAWTHERAPEE_BINARY: &str = "rawtherapee-cli";
const CONVERT_BINARY: &str = "convert";
const EXIFTOOL_BINARY: &str = "exiftool";
const CURL_BINARY: &str = "curl";
const CODEX_BINARY: &str = "codex";

fn startup_dependency_check(args: &[String]) -> Result<()> {
    let command = active_command_for_dependency_check(args);
    let help_mode = is_help_mode(args);
    let needs_image_externals = match command {
        Some("apply")
        | Some("batch")
        | Some("daemon")
        | Some("sampler")
        | Some("review-publish") => true,
        Some(_) => false,
        None => help_mode,
    };
    let needs_update_externals =
        matches!(command, Some("update")) || (help_mode && command.is_none());
    let needs_codex = matches!(command, Some("daemon"))
        && args
            .iter()
            .any(|arg| arg == "--codex" || arg.starts_with("--codex="));

    if !needs_image_externals && !needs_update_externals && !needs_codex {
        return Ok(());
    }

    let rawtherapee = resolve_dependency_path(args, "--rawtherapee", RAWTHERAPEE_BINARY);
    let convert = resolve_dependency_path(args, "--convert", CONVERT_BINARY);
    let exiftool = resolve_dependency_path(args, "--exiftool", EXIFTOOL_BINARY);
    let codex = resolve_dependency_path(args, "--codex-binary", CODEX_BINARY);
    let curl = PathBuf::from(CURL_BINARY);

    let mut failures = Vec::new();
    if needs_image_externals {
        if let Err(error) = verify_dependency_binary("rawtherapee-cli", &rawtherapee) {
            failures.push(error.to_string());
        }
        if let Err(error) = verify_dependency_binary("convert", &convert) {
            failures.push(error.to_string());
        }
        if let Err(error) = verify_dependency_binary("exiftool", &exiftool) {
            failures.push(error.to_string());
        }
    }
    if needs_update_externals && let Err(error) = verify_dependency_binary("curl", &curl) {
        failures.push(error.to_string());
    }
    if needs_codex && let Err(error) = verify_dependency_binary("codex", &codex) {
        failures.push(error.to_string());
    }
    if failures.is_empty() {
        return Ok(());
    }

    bail!("dependency check failed:\n  - {}", failures.join("\n  - "));
}

fn active_command_for_dependency_check(args: &[String]) -> Option<&str> {
    let mut positionals = args.iter().skip(1).filter(|arg| !arg.starts_with('-'));
    let first = positionals.next()?;
    if first == "help" {
        return positionals.next().map(String::as_str);
    }
    Some(first.as_str())
}

fn is_help_mode(args: &[String]) -> bool {
    args.len() == 1
        || args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--help" | "-h" | "help" | "--version" | "-V"))
}

fn resolve_dependency_path(args: &[String], flag: &str, default: &str) -> PathBuf {
    for index in 0..args.len() {
        let arg = &args[index];
        if arg == flag
            && let Some(value) = args.get(index + 1)
        {
            return PathBuf::from(value);
        }
        if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
            return PathBuf::from(value);
        }
    }
    PathBuf::from(default)
}

fn resolve_profiles_root(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(explicit) = explicit {
        return explicit;
    }

    if let Ok(profiles_root) = env::var("MINI_FILM_PROFILES_ROOT") {
        let trimmed = profiles_root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    PathBuf::from(".")
}

fn resolve_lcp_root(explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit.or_else(|| {
        env::var("MINI_FILM_LCP_ROOT")
            .ok()
            .map(|root| root.trim().to_string())
            .filter(|root| !root.is_empty())
            .map(PathBuf::from)
    })
}

fn input_file_filter(input_jpg_only: bool, input_raw_only: bool) -> InputFileFilter {
    if input_jpg_only {
        InputFileFilter::JpgOnly
    } else if input_raw_only {
        InputFileFilter::RawOnly
    } else {
        InputFileFilter::All
    }
}

fn verify_dependency_binary(name: &str, path: &Path) -> Result<()> {
    Command::new(path)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| {
            if matches!(err.kind(), ErrorKind::NotFound) {
                anyhow!("{} not found: {}", name, path.display())
            } else {
                anyhow!("{} is not executable: {}", name, err)
            }
        })
        .with_context(|| format!("running dependency probe for {name} at {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, File},
        io::Write,
        os::unix::fs::PermissionsExt,
        path::Path,
    };

    fn write_helper_binary(path: &Path, exit_code: i32) -> PathBuf {
        let mut file = File::create(path).unwrap();
        writeln!(file, "#!/bin/sh").unwrap();
        writeln!(file, "exit {exit_code}").unwrap();
        file.flush().unwrap();
        drop(file);
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
        PathBuf::from(path)
    }

    #[test]
    fn active_command_for_dependency_check_unwraps_help_command_prefix() {
        let args = ["mini-film", "help", "sampler", "--rawtherapee", "/tmp/rt"]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(active_command_for_dependency_check(&args), Some("sampler"));
    }

    #[test]
    fn active_command_for_dependency_check_keeps_main_command() {
        let args = ["mini-film", "batch", "--convert", "/tmp/convert"]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert_eq!(active_command_for_dependency_check(&args), Some("batch"));
    }

    #[test]
    fn empty_invocation_defaults_to_app_command() {
        let args = args_with_default_command(vec!["mini-film".to_string()]);
        assert_eq!(args, vec!["mini-film".to_string(), "app".to_string()]);
        let cli = Cli::parse_from(args);
        assert!(matches!(cli.command, CommandKind::App));
    }

    #[test]
    fn explicit_flags_are_not_rewritten_to_app_command() {
        let args = args_with_default_command(vec!["mini-film".to_string(), "--help".to_string()]);
        assert_eq!(args, vec!["mini-film".to_string(), "--help".to_string()]);
    }

    #[test]
    fn resolve_dependency_path_parses_flags() {
        let args = [
            "mini-film",
            "apply",
            "--rawtherapee",
            "/tmp/rt",
            "--convert=/tmp/conv",
            "--exiftool=/tmp/et",
        ]
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
        assert_eq!(
            resolve_dependency_path(&args, "--rawtherapee", "rawtherapee-cli"),
            PathBuf::from("/tmp/rt")
        );
        assert_eq!(
            resolve_dependency_path(&args, "--convert", "convert"),
            PathBuf::from("/tmp/conv")
        );
        assert_eq!(
            resolve_dependency_path(&args, "--exiftool", "exiftool"),
            PathBuf::from("/tmp/et")
        );
    }

    #[test]
    fn resolve_profiles_root_prefers_flag_and_env() {
        let env_previous = std::env::var("MINI_FILM_PROFILES_ROOT").ok();
        unsafe {
            std::env::set_var("MINI_FILM_PROFILES_ROOT", "/tmp/from-env");
        }
        let env_path = resolve_profiles_root(None);
        assert_eq!(env_path, PathBuf::from("/tmp/from-env"));
        let expected_when_unset = env_previous.clone();

        if let Some(previous) = env_previous {
            unsafe {
                std::env::set_var("MINI_FILM_PROFILES_ROOT", previous);
            }
        } else {
            unsafe {
                std::env::remove_var("MINI_FILM_PROFILES_ROOT");
            }
        }

        assert_eq!(
            resolve_profiles_root(Some(PathBuf::from("/tmp/explicit"))),
            PathBuf::from("/tmp/explicit")
        );
        let expected = expected_when_unset.unwrap_or_else(|| ".".to_string());
        assert_eq!(resolve_profiles_root(None), PathBuf::from(expected));
    }

    #[test]
    fn startup_dependency_check_ignores_when_help_requested() {
        let args = vec![
            "mini-film".to_string(),
            "--help".to_string(),
            "apply".to_string(),
        ];
        assert!(startup_dependency_check(&args).is_ok());
    }

    #[test]
    fn startup_dependency_check_respects_fake_binaries() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = tempfile::tempdir_in(cwd).unwrap();
        let rawtherapee = write_helper_binary(&root.path().join("rawtherapee-cli"), 0);
        let convert = write_helper_binary(&root.path().join("convert"), 0);
        let exiftool = write_helper_binary(&root.path().join("exiftool"), 0);
        let args = vec![
            "mini-film".to_string(),
            "apply".to_string(),
            "--rawtherapee".to_string(),
            rawtherapee.display().to_string(),
            "--convert".to_string(),
            convert.display().to_string(),
            "--exiftool".to_string(),
            exiftool.display().to_string(),
        ];
        assert!(startup_dependency_check(&args).is_ok());
    }

    #[test]
    fn startup_dependency_check_requires_codex_only_when_enabled() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = tempfile::tempdir_in(cwd).unwrap();
        let rawtherapee = write_helper_binary(&root.path().join("rawtherapee-cli"), 0);
        let convert = write_helper_binary(&root.path().join("convert"), 0);
        let exiftool = write_helper_binary(&root.path().join("exiftool"), 0);
        let codex = write_helper_binary(&root.path().join("codex"), 0);
        let args = vec![
            "mini-film".to_string(),
            "daemon".to_string(),
            "in".to_string(),
            "out".to_string(),
            "--profile".to_string(),
            "profile".to_string(),
            "--rawtherapee".to_string(),
            rawtherapee.display().to_string(),
            "--convert".to_string(),
            convert.display().to_string(),
            "--exiftool".to_string(),
            exiftool.display().to_string(),
            "--codex".to_string(),
            "--codex-binary".to_string(),
            codex.display().to_string(),
        ];
        assert!(startup_dependency_check(&args).is_ok());
    }

    #[test]
    fn startup_dependency_check_rejects_missing_helpers() {
        let args = vec![
            "mini-film".to_string(),
            "apply".to_string(),
            "--rawtherapee".to_string(),
            "/tmp/does-not-exist-rt".to_string(),
            "--convert".to_string(),
            "/tmp/does-not-exist-convert".to_string(),
        ];
        assert!(startup_dependency_check(&args).is_err());
    }
}
