#[cfg(feature = "desktop-app")]
mod enabled {
    use std::{
        collections::BTreeMap,
        env, fs,
        io::ErrorKind,
        net::{TcpListener, TcpStream, ToSocketAddrs},
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::Mutex,
        thread,
        time::{Duration, Instant},
    };

    use anyhow::{Context, Result, anyhow, bail};
    use mini_film::GrainEngine;
    use serde::{Deserialize, Serialize};
    use tauri_plugin_dialog::DialogExt;

    use crate::{
        app::{
            batch_daemon::{BatchDaemonArgs, run_batch_daemon},
            info::profile_info_text_for_selector,
            sampler::{
                collect_xmp_profiles, emulation_root, profile_display_name_from_relative,
                profile_name_parts, variant_sort_key,
            },
            util::{InputFileFilter, default_hald_dir, half_cpu_thread_count},
        },
        cli::{
            BatchOutputFormat, CodexAnalysisFlags, ExportOptions, GalleryTemplate, JpegSubsampling,
            LensCorrections,
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
        input: String,
        output: String,
        profiles_root: String,
        profiles: Vec<String>,
        hald_dir: String,
        review_port: u16,
        allow_others: bool,
        jobs: usize,
        long_edge: Option<u32>,
        jpg_quality: u8,
        gallery: String,
        rawtherapee: String,
        convert: String,
        publish_album: String,
        nikon_wtu: String,
        color_noise_iso_threshold: u32,
        grain_preset: String,
        progressive_jpeg: bool,
        no_grain: bool,
        lens_corrections: bool,
        codex: bool,
        codex_tags: bool,
        codex_note: bool,
        codex_rating: bool,
        codex_binary: String,
        codex_model: String,
        codex_timeout: u64,
    }

    #[derive(Clone, Debug, Default, Deserialize, Serialize)]
    #[serde(default)]
    #[serde(rename_all = "camelCase")]
    struct AppDaemonRequest {
        input: String,
        output: String,
        profiles_root: String,
        profiles: Vec<String>,
        review_port: Option<u16>,
        allow_others: bool,
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
        codex: bool,
        codex_tags: bool,
        codex_note: bool,
        codex_rating: bool,
        codex_binary: String,
        codex_model: String,
        codex_timeout: Option<u64>,
    }

    #[derive(Clone, Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AppStartResponse {
        review_url: String,
    }

    pub(crate) fn run_desktop_app() -> Result<()> {
        tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .manage(AppRuntime::default())
            .invoke_handler(tauri::generate_handler![
                app_defaults,
                pick_directory,
                profile_tree,
                start_app_daemon
            ])
            .run(tauri::generate_context!())
            .map_err(|error| anyhow!("running mini-film desktop app: {error}"))
    }

    #[tauri::command]
    fn app_defaults() -> AppDefaults {
        AppDefaults::load()
    }

    impl AppDefaults {
        fn load() -> Self {
            let default = Self::factory_defaults();
            let Ok(text) = fs::read_to_string(app_settings_path()) else {
                return default;
            };
            let Ok(saved) = serde_json::from_str::<AppDaemonRequest>(&text) else {
                return default;
            };
            default.with_saved(saved)
        }

        fn factory_defaults() -> Self {
            AppDefaults {
                version: env!("CARGO_PKG_VERSION"),
                input: default_user_path(&["Pictures", "Scratch", "Inbox"])
                    .unwrap_or_else(|| PathBuf::from("."))
                    .to_string_lossy()
                    .to_string(),
                output: default_user_path(&["Pictures", "mini-film"])
                    .unwrap_or_else(|| PathBuf::from("."))
                    .to_string_lossy()
                    .to_string(),
                profiles_root: resolve_profiles_root(None).to_string_lossy().to_string(),
                profiles: Vec::new(),
                hald_dir: default_hald_dir().to_string_lossy().to_string(),
                review_port: 8090,
                allow_others: true,
                jobs: half_cpu_thread_count(),
                long_edge: None,
                jpg_quality: 95,
                gallery: String::new(),
                rawtherapee: "rawtherapee-cli".to_string(),
                convert: "convert".to_string(),
                publish_album: "published".to_string(),
                nikon_wtu: String::new(),
                color_noise_iso_threshold: 1600,
                grain_preset: String::new(),
                progressive_jpeg: false,
                no_grain: false,
                lens_corrections: true,
                codex: false,
                codex_tags: true,
                codex_note: false,
                codex_rating: false,
                codex_binary: "codex".to_string(),
                codex_model: "gpt-5.4-mini".to_string(),
                codex_timeout: 45,
            }
        }

        fn with_saved(mut self, saved: AppDaemonRequest) -> Self {
            self.input = saved_string(saved.input, self.input);
            self.output = saved_string(saved.output, self.output);
            self.profiles_root = saved_string(saved.profiles_root, self.profiles_root);
            self.profiles = saved.profiles;
            self.review_port = saved.review_port.unwrap_or(self.review_port);
            self.allow_others = saved.allow_others;
            self.jobs = saved.jobs.unwrap_or(self.jobs);
            self.long_edge = saved.long_edge;
            self.jpg_quality = saved.jpg_quality.unwrap_or(self.jpg_quality);
            self.gallery = saved.gallery;
            self.publish_album = saved_string(saved.publish_album, self.publish_album);
            self.rawtherapee = saved_string(saved.rawtherapee, self.rawtherapee);
            self.convert = saved_string(saved.convert, self.convert);
            self.hald_dir = saved_string(saved.hald_dir, self.hald_dir);
            self.nikon_wtu = saved.nikon_wtu;
            self.color_noise_iso_threshold = saved
                .color_noise_iso_threshold
                .unwrap_or(self.color_noise_iso_threshold);
            self.grain_preset = saved.grain_preset;
            self.progressive_jpeg = saved.progressive_jpeg;
            self.no_grain = saved.no_grain;
            self.lens_corrections = saved.lens_corrections;
            self.codex = saved.codex;
            self.codex_tags = saved.codex_tags;
            self.codex_note = saved.codex_note;
            self.codex_rating = saved.codex_rating;
            self.codex_binary = saved_string(saved.codex_binary, self.codex_binary);
            self.codex_model = saved_string(saved.codex_model, self.codex_model);
            self.codex_timeout = saved.codex_timeout.unwrap_or(self.codex_timeout);
            self
        }
    }

    #[derive(Clone, Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AppProfileTree {
        root: String,
        count: usize,
        children: Vec<AppProfileNode>,
    }

    #[derive(Clone, Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AppProfileNode {
        label: String,
        profiles: Vec<AppProfileLeaf>,
        children: Vec<AppProfileNode>,
    }

    #[derive(Clone, Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct AppProfileLeaf {
        name: String,
        path: String,
        relative: String,
    }

    #[derive(Default)]
    struct ProfileTreeNode {
        profiles: Vec<AppProfileLeaf>,
        children: BTreeMap<String, ProfileTreeNode>,
    }

    #[tauri::command]
    async fn pick_directory(
        app: tauri::AppHandle,
        title: String,
        start: Option<String>,
    ) -> Result<Option<String>, String> {
        let title = if title.trim().is_empty() {
            "Choose folder".to_string()
        } else {
            title.trim().to_string()
        };
        let mut dialog = app
            .dialog()
            .file()
            .set_title(title)
            .set_can_create_directories(true);
        if let Some(start) = existing_directory_hint(start.as_deref()) {
            dialog = dialog.set_directory(start);
        }

        tauri::async_runtime::spawn_blocking(move || {
            dialog
                .blocking_pick_folder()
                .map(file_path_to_string)
                .transpose()
                .map_err(|error| format!("{error:#}"))
        })
        .await
        .map_err(|error| format!("directory picker task failed: {error}"))?
    }

    #[tauri::command]
    fn profile_tree(profiles_root: String) -> Result<AppProfileTree, String> {
        profile_tree_inner(profiles_root).map_err(|error| format!("{error:#}"))
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

        let saved_request = request.clone();
        let args = request.into_args()?;
        validate_app_daemon_args(&args)?;
        save_app_settings(&saved_request)?;
        let review_url = review_url_for_address(
            args.review_address
                .as_deref()
                .ok_or_else(|| anyhow!("review address is missing"))?,
        );
        let connect_address = review_connect_address(
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

        wait_for_review_server(&connect_address, Duration::from_secs(120))
            .with_context(|| format!("waiting for review server at {review_url}"))?;

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
            let review_address = resolve_review_address(
                self.allow_others,
                self.review_port.filter(|port| *port > 0).unwrap_or(8090),
            )?;
            let rawtherapee = required_path("RawTherapee binary", &self.rawtherapee)?;
            let convert = required_path("convert binary", &self.convert)?;
            let gallery = parse_gallery(&self.gallery)?;
            let publish_album = if self.publish_album.trim().is_empty() {
                "published".to_string()
            } else {
                self.publish_album.trim().to_string()
            };
            let grain_preset = optional_string(&self.grain_preset);
            let codex_flags = if self.codex {
                let mut flags = CodexAnalysisFlags {
                    tags: self.codex_tags,
                    note: self.codex_note,
                    rating: self.codex_rating,
                };
                if !flags.is_enabled() {
                    flags = CodexAnalysisFlags::tags_only();
                }
                Some(flags)
            } else {
                None
            };
            let codex_binary = if codex_flags.is_some() {
                required_path("Codex binary", &self.codex_binary)?
            } else {
                optional_path(&self.codex_binary).unwrap_or_else(|| PathBuf::from("codex"))
            };
            let codex_model = if self.codex_model.trim().is_empty() {
                "gpt-5.4-mini".to_string()
            } else {
                self.codex_model.trim().to_string()
            };

            Ok(BatchDaemonArgs {
                input,
                output,
                profile,
                input_file_filter: InputFileFilter::All,
                hald_dir,
                profiles_root,
                hald_level: 16,
                rawtherapee,
                dng_fallback: crate::app::dng::DngFallbackConfig::default(),
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
                grain_engine: GrainEngine::default(),
                color_noise_iso_threshold: self.color_noise_iso_threshold.unwrap_or(1600),
                jobs: self.jobs,
                debounce_seconds: 0,
                nikon_wtu: optional_string(&self.nikon_wtu),
                nikon_wtu_port: 15740,
                nikon_wtu_name: None,
                nikon_wtu_guid: None,
                review_address: Some(review_address),
                hugin_bin_dir: None,
                codex: codex_flags,
                codex_binary,
                codex_model,
                codex_timeout: self.codex_timeout.unwrap_or(45),
                lcp_root: lcp_root_from_env(),
                gallery,
                gallery_thumbnail_long_edge: 1024,
                gallery_columns: 4,
                publish_album,
                output_format: BatchOutputFormat::Jpg,
                invocation: None,
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
        if args.codex.is_some() {
            verify_dependency_binary("codex", &args.codex_binary)?;
        }

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
        Ok(expand_user_path(&value))
    }

    fn optional_path(raw: &str) -> Option<PathBuf> {
        optional_string(raw).map(|path| expand_user_path(&path))
    }

    fn optional_string(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    }

    fn normalize_profiles(profiles: Vec<String>) -> Result<Vec<String>> {
        Ok(profiles
            .into_iter()
            .filter_map(|profile| optional_string(&profile))
            .collect::<Vec<_>>())
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
                return expand_user_path(trimmed);
            }
        }

        if let Some(default) = default_user_path(&["Pictures", "profile-library"]) {
            return default;
        }

        PathBuf::from(".")
    }

    fn lcp_root_from_env() -> Option<PathBuf> {
        env::var("MINI_FILM_LCP_ROOT").ok().and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(expand_user_path(trimmed))
            }
        })
    }

    fn resolve_review_address(allow_others: bool, preferred_port: u16) -> Result<String> {
        let host = if allow_others { "0.0.0.0" } else { "127.0.0.1" };
        let preferred = format!("{host}:{preferred_port}");
        if TcpListener::bind(&preferred).is_ok() {
            return Ok(preferred);
        }

        let random =
            TcpListener::bind(format!("{host}:0")).context("finding free review server port")?;
        let port = random
            .local_addr()
            .context("reading free review server port")?
            .port();
        Ok(format!("{host}:{port}"))
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

    fn review_connect_address(address: &str) -> String {
        address
            .strip_prefix("0.0.0.0:")
            .map(|port| format!("127.0.0.1:{port}"))
            .or_else(|| {
                address
                    .strip_prefix("[::]:")
                    .map(|port| format!("127.0.0.1:{port}"))
            })
            .unwrap_or_else(|| address.to_string())
    }

    fn wait_for_review_server(address: &str, timeout: Duration) -> Result<()> {
        let mut addrs = address
            .to_socket_addrs()
            .with_context(|| format!("resolving review server address {address}"))?;
        let addr = addrs
            .next()
            .ok_or_else(|| anyhow!("review server address did not resolve: {address}"))?;
        let started = Instant::now();
        while started.elapsed() < timeout {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(150));
        }
        bail!("review server did not accept connections within {timeout:?}");
    }

    fn save_app_settings(request: &AppDaemonRequest) -> Result<()> {
        let path = app_settings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating app settings directory {}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(request).context("serializing app settings")?;
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    }

    fn app_settings_path() -> PathBuf {
        home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cache")
            .join("mini-film")
            .join("app-settings.json")
    }

    fn saved_string(saved: String, fallback: String) -> String {
        if saved.trim().is_empty() {
            fallback
        } else {
            saved
        }
    }

    fn file_path_to_string(path: tauri_plugin_dialog::FilePath) -> Result<String> {
        let path: PathBuf = path
            .try_into()
            .map_err(|error| anyhow!("selected path is not a local filesystem path: {error}"))?;
        Ok(path.to_string_lossy().to_string())
    }

    fn profile_tree_inner(raw_profiles_root: String) -> Result<AppProfileTree> {
        let profiles_root = resolve_profiles_root(optional_path(&raw_profiles_root));
        let root = emulation_root(&profiles_root);
        let profiles = collect_xmp_profiles(&root)
            .with_context(|| format!("scanning profiles under {}", root.display()))?;
        if profiles.is_empty() {
            bail!("no XMP emulation profiles found under {}", root.display());
        }

        let mut tree = ProfileTreeNode::default();
        for path in profiles {
            let relative = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string();
            let name = profile_display_name_from_relative(&relative);
            let parts = profile_name_parts(&name);
            tree.insert(
                &parts,
                AppProfileLeaf {
                    name,
                    path: path.to_string_lossy().to_string(),
                    relative,
                },
            );
        }

        let children = tree.into_children();
        let count = count_profile_leaves(&children);
        Ok(AppProfileTree {
            root: root.to_string_lossy().to_string(),
            count,
            children,
        })
    }

    impl ProfileTreeNode {
        fn insert(&mut self, parts: &[String], profile: AppProfileLeaf) {
            if let Some((part, rest)) = parts.split_first() {
                self.children
                    .entry(part.clone())
                    .or_default()
                    .insert(rest, profile);
            } else {
                self.profiles.push(profile);
            }
        }

        fn into_children(self) -> Vec<AppProfileNode> {
            children_into_nodes(self.children)
        }

        fn into_node(self, label: String) -> AppProfileNode {
            let ProfileTreeNode {
                mut profiles,
                children,
            } = self;
            profiles.sort_by(|left, right| {
                variant_sort_key(&left.name).cmp(&variant_sort_key(&right.name))
            });
            AppProfileNode {
                label,
                profiles,
                children: children_into_nodes(children),
            }
        }
    }

    fn children_into_nodes(children: BTreeMap<String, ProfileTreeNode>) -> Vec<AppProfileNode> {
        let mut children = children
            .into_iter()
            .map(|(label, child)| child.into_node(label))
            .collect::<Vec<_>>();
        children.sort_by(|left, right| {
            variant_sort_key(&left.label).cmp(&variant_sort_key(&right.label))
        });
        children
    }

    fn count_profile_leaves(nodes: &[AppProfileNode]) -> usize {
        nodes
            .iter()
            .map(|node| node.profiles.len() + count_profile_leaves(&node.children))
            .sum()
    }

    fn existing_directory_hint(raw: Option<&str>) -> Option<PathBuf> {
        let path = raw
            .and_then(optional_string)
            .map(|path| expand_user_path(&path))?;
        if path.is_dir() {
            return Some(path);
        }
        path.parent()
            .filter(|parent| parent.is_dir())
            .map(Path::to_path_buf)
    }

    fn expand_user_path(path: &str) -> PathBuf {
        let path = path.trim();
        if path == "~" {
            if let Some(home) = home_dir() {
                return home;
            }
        } else if let Some(rest) = path.strip_prefix("~/")
            && let Some(home) = home_dir()
        {
            return home.join(rest);
        }
        PathBuf::from(path)
    }

    fn default_user_path(parts: &[&str]) -> Option<PathBuf> {
        let mut path = home_dir()?;
        for part in parts {
            path.push(part);
        }
        Some(path)
    }

    fn home_dir() -> Option<PathBuf> {
        env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
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
