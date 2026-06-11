#[cfg(feature = "desktop-app")]
mod enabled {
    use std::{
        env, fs,
        io::ErrorKind,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::Mutex,
        thread,
    };

    use anyhow::{Context, Result, anyhow, bail};
    use serde::{Deserialize, Serialize};

    use crate::{
        app::{
            batch_daemon::{BatchDaemonArgs, run_batch_daemon},
            info::profile_info_text_for_selector,
            util::{default_hald_dir, half_cpu_thread_count},
        },
        cli::{
            BatchOutputFormat, ExportOptions, GalleryTemplate, JpegSubsampling, LensCorrections,
        },
    };

    #[derive(Default)]
    struct AppRuntime {
        daemon: Mutex<Option<AppDaemon>>,
    }

    struct AppDaemon {
        review_url: String,
    }

    #[derive(Clone, Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AppDefaults {
        version: &'static str,
        profiles_root: String,
        hald_dir: String,
        review_address: String,
        jobs: usize,
        jpg_quality: u8,
        rawtherapee: String,
        convert: String,
        publish_album: String,
        color_noise_iso_threshold: u32,
        progressive_jpeg: bool,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AppDaemonRequest {
        input: String,
        output: String,
        profiles_root: String,
        profiles: Vec<String>,
        review_address: String,
        jobs: Option<usize>,
        long_edge: Option<u32>,
        jpg_quality: Option<u8>,
        gallery: String,
        publish_album: String,
        rawtherapee: String,
        convert: String,
        hald_dir: String,
        nikon_wtu: String,
        color_noise_iso_threshold: Option<u32>,
        grain_preset: String,
        progressive_jpeg: bool,
        no_grain: bool,
        lens_corrections: bool,
    }

    #[derive(Clone, Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AppStartResponse {
        review_url: String,
    }

    pub(crate) fn run_desktop_app() -> Result<()> {
        tauri::Builder::default()
            .manage(AppRuntime::default())
            .invoke_handler(tauri::generate_handler![app_defaults, start_app_daemon])
            .run(tauri::generate_context!())
            .map_err(|error| anyhow!("running mini-film desktop app: {error}"))
    }

    #[tauri::command]
    fn app_defaults() -> AppDefaults {
        AppDefaults {
            version: env!("CARGO_PKG_VERSION"),
            profiles_root: resolve_profiles_root(None).to_string_lossy().to_string(),
            hald_dir: default_hald_dir().to_string_lossy().to_string(),
            review_address: "127.0.0.1:8090".to_string(),
            jobs: half_cpu_thread_count(),
            jpg_quality: 95,
            rawtherapee: "rawtherapee-cli".to_string(),
            convert: "convert".to_string(),
            publish_album: "published".to_string(),
            color_noise_iso_threshold: 1600,
            progressive_jpeg: false,
        }
    }

    #[tauri::command]
    fn start_app_daemon(
        state: tauri::State<'_, AppRuntime>,
        request: AppDaemonRequest,
    ) -> Result<AppStartResponse, String> {
        start_app_daemon_inner(&state, request).map_err(|error| format!("{error:#}"))
    }

    fn start_app_daemon_inner(
        state: &tauri::State<'_, AppRuntime>,
        request: AppDaemonRequest,
    ) -> Result<AppStartResponse> {
        let mut daemon = state
            .daemon
            .lock()
            .map_err(|_| anyhow!("desktop app daemon state lock poisoned"))?;
        if let Some(daemon) = daemon.as_ref() {
            return Ok(AppStartResponse {
                review_url: daemon.review_url.clone(),
            });
        }

        let args = request.into_args()?;
        validate_app_daemon_args(&args)?;
        let review_url = review_url_for_address(
            args.review_address
                .as_deref()
                .ok_or_else(|| anyhow!("review address is missing"))?,
        );

        let thread_args = args;
        thread::Builder::new()
            .name("mini-film-app-daemon".to_string())
            .spawn(move || {
                if let Err(error) = run_batch_daemon(thread_args) {
                    eprintln!("mini-film app daemon stopped: {error:#}");
                }
            })
            .context("starting desktop app daemon thread")?;

        *daemon = Some(AppDaemon {
            review_url: review_url.clone(),
        });
        Ok(AppStartResponse { review_url })
    }

    impl AppDaemonRequest {
        fn into_args(self) -> Result<BatchDaemonArgs> {
            let input = required_path("input inbox", &self.input)?;
            let output = required_path("output folder", &self.output)?;
            let profiles_root = resolve_profiles_root(optional_path(&self.profiles_root));
            let hald_dir = optional_path(&self.hald_dir).unwrap_or_else(default_hald_dir);
            let profile = normalize_profiles(self.profiles)?;
            let review_address = if self.review_address.trim().is_empty() {
                "127.0.0.1:8090".to_string()
            } else {
                self.review_address.trim().to_string()
            };
            let rawtherapee = required_path("RawTherapee binary", &self.rawtherapee)?;
            let convert = required_path("convert binary", &self.convert)?;
            let gallery = parse_gallery(&self.gallery)?;
            let publish_album = if self.publish_album.trim().is_empty() {
                "published".to_string()
            } else {
                self.publish_album.trim().to_string()
            };
            let grain_preset = optional_string(&self.grain_preset);

            Ok(BatchDaemonArgs {
                input,
                output,
                profile,
                hald_dir,
                profiles_root,
                hald_level: 16,
                rawtherapee,
                convert,
                no_grain: self.no_grain,
                lens_corrections: if self.lens_corrections {
                    LensCorrections::all()
                } else {
                    LensCorrections::none()
                },
                grain: None,
                grain_preset,
                grain_seed: None,
                color_noise_iso_threshold: self.color_noise_iso_threshold.unwrap_or(1600),
                jobs: self.jobs,
                debounce_seconds: 0,
                nikon_wtu: optional_string(&self.nikon_wtu),
                nikon_wtu_port: 15740,
                nikon_wtu_name: None,
                nikon_wtu_guid: None,
                review_address: Some(review_address),
                gallery,
                gallery_thumbnail_long_edge: 1024,
                gallery_columns: 4,
                publish_album,
                output_format: BatchOutputFormat::Jpg,
                export: ExportOptions {
                    jpg_quality: self.jpg_quality.unwrap_or(95),
                    resize: None,
                    long_edge: self.long_edge,
                    max_width: None,
                    max_height: None,
                    jpeg_subsampling: JpegSubsampling::S444,
                    strip_metadata: false,
                    progressive_jpeg: self.progressive_jpeg,
                },
            })
        }
    }

    fn validate_app_daemon_args(args: &BatchDaemonArgs) -> Result<()> {
        if !args.input.is_dir() {
            bail!("input inbox is not a directory: {}", args.input.display());
        }
        fs::create_dir_all(&args.output)
            .with_context(|| format!("creating output folder {}", args.output.display()))?;

        verify_dependency_binary("rawtherapee-cli", &args.rawtherapee)?;
        verify_dependency_binary("convert", &args.convert)?;
        verify_dependency_binary("exiftool", Path::new("exiftool"))?;

        for profile in &args.profile {
            profile_info_text_for_selector(
                profile,
                &args.profiles_root,
                &args.hald_dir,
                args.hald_level,
            )
            .with_context(|| format!("resolving profile {profile:?}"))?;
        }
        Ok(())
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
            .with_context(|| {
                format!("running dependency probe for {name} at {}", path.display())
            })?;
        Ok(())
    }

    fn required_path(label: &str, raw: &str) -> Result<PathBuf> {
        let Some(value) = optional_string(raw) else {
            bail!("{label} is required");
        };
        Ok(PathBuf::from(value))
    }

    fn optional_path(raw: &str) -> Option<PathBuf> {
        optional_string(raw).map(PathBuf::from)
    }

    fn optional_string(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    fn normalize_profiles(profiles: Vec<String>) -> Result<Vec<String>> {
        let profiles = profiles
            .into_iter()
            .filter_map(|profile| optional_string(&profile))
            .collect::<Vec<_>>();
        if profiles.is_empty() {
            bail!("at least one profile is required");
        }
        Ok(profiles)
    }

    fn parse_gallery(raw: &str) -> Result<Option<GalleryTemplate>> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "none" => Ok(None),
            "modern" => Ok(Some(GalleryTemplate::Modern)),
            "soft" => Ok(Some(GalleryTemplate::Soft)),
            "compact" => Ok(Some(GalleryTemplate::Compact)),
            "hero" => Ok(Some(GalleryTemplate::Hero)),
            "phone" => Ok(Some(GalleryTemplate::Phone)),
            other => bail!("unsupported gallery template {other:?}"),
        }
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

    fn review_url_for_address(address: &str) -> String {
        let host = address
            .strip_prefix("0.0.0.0:")
            .map(|port| format!("127.0.0.1:{port}"))
            .or_else(|| {
                address
                    .strip_prefix("[::]:")
                    .map(|port| format!("127.0.0.1:{port}"))
            })
            .unwrap_or_else(|| address.to_string());
        format!("http://{host}")
    }
}

#[cfg(not(feature = "desktop-app"))]
mod disabled {
    use anyhow::{Result, bail};

    pub(crate) fn run_desktop_app() -> Result<()> {
        bail!("mini-film app was not built into this binary; rebuild with --features desktop-app")
    }
}

#[cfg(not(feature = "desktop-app"))]
pub(crate) use disabled::run_desktop_app;
#[cfg(feature = "desktop-app")]
pub(crate) use enabled::run_desktop_app;
