use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use mini_film::GrainEngine;
use serde::{Deserialize, Serialize};

const DEFAULT_HALD_LEVEL: u32 = 16;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Review and publish RAW, JPEG, and HEIC photos with optional RAW film profiles"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: CommandKind,
}

#[derive(Subcommand, Debug)]
pub(crate) enum CommandKind {
    /// Launch the desktop wizard and embedded review webview.
    App,

    /// Convert Adobe Camera Raw crs:RGBTable XMP profiles to Hald CLUT PNGs.
    Hald {
        /// XMP profile file or directory to convert.
        input: PathBuf,

        /// Output PNG path for a single file, or output directory for a directory input. Defaults to $HOME/.cache/mini-film/hald.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Hald level. Level 16 produces a 256x256x256 CLUT stored as a 4096x4096 PNG.
        #[arg(short = 'l', long, default_value_t = DEFAULT_HALD_LEVEL)]
        hald_level: u32,

        /// Overwrite existing output files.
        #[arg(long)]
        overwrite: bool,

        /// Print table metadata without writing PNGs.
        #[arg(long)]
        info_only: bool,
    },

    /// Print parsed details for an emulation or internal RGBTable profile.
    Info {
        /// Profile selector: emulation XMP path/name, internal RGBTable XMP path/name, Hald PNG path/name, or PP3 path.
        profile: String,

        /// Film library root. Emulation XMPs are selected from emulations/ and RGBTable profiles from profiles/.
        #[arg(long)]
        profiles_root: Option<PathBuf>,

        /// Directory containing generated cached Hald PNGs. Defaults to $HOME/.cache/mini-film/hald.
        #[arg(long)]
        hald_dir: Option<PathBuf>,

        /// Hald level used when reporting the cached Hald path for XMP profiles.
        #[arg(short = 'l', long, default_value_t = DEFAULT_HALD_LEVEL)]
        hald_level: u32,
    },

    /// Print the RawTherapee PP3 generated for an emulation or RGBTable profile.
    Pp3 {
        /// Profile selector: emulation XMP path/name, internal RGBTable XMP path/name, Hald PNG path/name, or PP3 path.
        profile: String,

        /// Output PP3 path.
        #[arg(short, long, default_value = "/dev/stdout")]
        output: PathBuf,

        /// Film library root. Emulation XMPs are selected from emulations/ and RGBTable profiles from profiles/.
        #[arg(long)]
        profiles_root: Option<PathBuf>,

        /// Directory containing generated cached Hald PNGs. Defaults to $HOME/.cache/mini-film/hald.
        #[arg(long)]
        hald_dir: Option<PathBuf>,

        /// Hald level used when reporting the cached Hald path for XMP profiles.
        #[arg(short = 'l', long, default_value_t = DEFAULT_HALD_LEVEL)]
        hald_level: u32,
    },

    /// Fit an XMP/RGBTable/Hald look into a Nikon classic .NCP Picture Control.
    Nikon {
        /// Profile selector: emulation XMP path/name, internal RGBTable XMP path/name, or Hald PNG path/name.
        profile: String,

        /// Output .NCP path.
        #[arg(short, long)]
        output: PathBuf,

        /// Optional report path describing approximation error and fitted controls.
        #[arg(long)]
        report: Option<PathBuf>,

        /// Picture Control display name. NCP names are ASCII and truncated to 19 bytes.
        #[arg(long)]
        name: Option<String>,

        /// Film library root. Emulation XMPs are selected from emulations/ and RGBTable profiles from profiles/.
        #[arg(long)]
        profiles_root: Option<PathBuf>,

        /// Directory containing generated cached Hald PNGs. Defaults to $HOME/.cache/mini-film/hald.
        #[arg(long)]
        hald_dir: Option<PathBuf>,

        /// Hald level used when resolving cached Hald paths for XMP profiles.
        #[arg(short = 'l', long, default_value_t = DEFAULT_HALD_LEVEL)]
        hald_level: u32,
    },

    /// Develop a RAW file or process a JPEG/HEIC input, optionally with a profile.
    Apply {
        /// Input file. RAW inputs support common camera RAW formats such as `.dng`,
        /// `.nef`, `.cr2`, `.cr3`, `.arw`, `.raf`, `.orf`, `.rw2`; compressed
        /// inputs support `.jpg`, `.jpeg`, `.heic`, and `.heif`.
        raw: PathBuf,

        /// Output image path.
        #[arg(short, long)]
        output: PathBuf,

        /// Optional profile selector: Hald PNG path/name, emulation XMP path/name, or RawTherapee PP3 path.
        /// If omitted, RawTherapee develops RAW inputs with its defaults while JPEG/HEIC inputs are converted directly.
        /// With a profile, JPEG is processed directly by RawTherapee and HEIC is prepared as a 16-bit TIFF first.
        #[arg(short, long)]
        profile: Option<String>,

        /// Directory containing generated cached Hald PNGs. Defaults to $HOME/.cache/mini-film/hald.
        #[arg(long)]
        hald_dir: Option<PathBuf>,

        /// Film library root. Emulation XMPs are selected from emulations/ and RGBTable profiles from profiles/.
        #[arg(long)]
        profiles_root: Option<PathBuf>,

        /// Hald level to use when --profile points to an XMP or resolves to an XMP.
        #[arg(short = 'l', long, default_value_t = DEFAULT_HALD_LEVEL)]
        hald_level: u32,

        /// Path to rawtherapee-cli binary.
        #[arg(long, default_value = "rawtherapee-cli")]
        rawtherapee: PathBuf,

        /// Path to convert binary.
        #[arg(long, default_value = "convert")]
        convert: PathBuf,

        /// Keep the intermediate TIFF generated by RawTherapee.
        #[arg(long)]
        keep_intermediate: Option<PathBuf>,

        /// Disable Lightroom XMP grain emulation.
        #[arg(long)]
        no_grain: bool,

        /// Minimum raw ISO for enabling RawTherapee directional pyramid color noise.
        /// Use 0 to disable color-noise processing.
        #[arg(long, default_value_t = 1600)]
        color_noise_iso_threshold: u32,

        /// Enable RawTherapee lens corrections.
        ///
        /// Without an explicit value, enables distortion, ca, and vignetting.
        /// Optionally pass a comma-separated list of:
        /// `distortion`, `ca`, `vignetting`.
        ///
        /// Examples:
        /// - `--lens-corrections`
        /// - `--lens-corrections distortion,ca`
        /// - `--lens-corrections all`
        #[arg(long, num_args = 0..=1, value_parser = parse_lens_corrections_arg, default_missing_value = "all")]
        lens_corrections: Option<LensCorrections>,

        /// Optional Lensfun profile root for RawTherapee LCP profiles.
        ///
        /// If omitted, mini-film resolves from `MINI_FILM_LCP_ROOT` when set.
        #[arg(long)]
        lcp_root: Option<PathBuf>,

        /// Override grain as amount,size,frequency, each 0..100. Example: --grain 30,45,45
        #[arg(long)]
        grain: Option<String>,

        /// Built-in grain override: light, medium, or heavy.
        #[arg(long)]
        grain_preset: Option<String>,

        /// Seed for deterministic generated grain. Defaults to current time of day.
        #[arg(long)]
        grain_seed: Option<u64>,

        /// Grain renderer to use for profiled outputs.
        #[arg(long, value_enum, default_value_t = GrainEngine::default())]
        grain_engine: GrainEngine,

        /// JPEG quality when output path ends in .jpg or .jpeg.
        #[arg(long, default_value_t = 95)]
        jpg_quality: u8,

        /// Resize final output with GraphicsMagick geometry, for example 3000x3000 or 3000x3000>.
        #[arg(long)]
        resize: Option<String>,

        /// Resize final output so the longest edge is at most this many pixels.
        #[arg(long)]
        long_edge: Option<u32>,

        /// Resize final output so width is at most this many pixels.
        #[arg(long)]
        max_width: Option<u32>,

        /// Resize final output so height is at most this many pixels.
        #[arg(long)]
        max_height: Option<u32>,

        /// JPEG chroma subsampling.
        #[arg(long, value_enum, default_value_t = JpegSubsampling::S444)]
        jpeg_subsampling: JpegSubsampling,

        /// Strip profiles and text metadata from final output.
        #[arg(long)]
        strip_metadata: bool,

        /// Write progressive/interlaced JPEG output.
        #[arg(long)]
        progressive_jpeg: bool,
    },

    /// Apply an optional profile or directly convert compressed inputs in a folder.
    Batch {
        /// Input folder scanned recursively for supported RAW and compressed image files.
        ///
        /// Without filters, both file groups are accepted: RAW (e.g. `.dng`, `.nef`, `.cr2`,
        /// `.cr3`, `.arw`, `.raf`, `.orf`, `.rw2`, ...) and JPEG/HEIC (`.jpg`, `.jpeg`,
        /// `.heic`, `.heif`).
        input: PathBuf,

        /// Output folder. It is created if it does not exist.
        output: PathBuf,

        /// Optional profile selector: Hald PNG path/name, emulation XMP path/name, or RawTherapee PP3 path.
        /// If omitted, RawTherapee develops each RAW with its defaults and JPEG/HEIC inputs are converted directly.
        /// If provided, the profile is also applied to standalone JPEG/HEIC inputs.
        #[arg(short, long)]
        profile: Option<String>,

        /// Directory containing generated cached Hald PNGs. Defaults to $HOME/.cache/mini-film/hald.
        #[arg(long)]
        hald_dir: Option<PathBuf>,

        /// Film library root. Emulation XMPs are selected from emulations/ and RGBTable profiles from profiles/.
        #[arg(long)]
        profiles_root: Option<PathBuf>,

        /// Hald level to use when --profile points to an XMP or resolves to an XMP.
        #[arg(short = 'l', long, default_value_t = DEFAULT_HALD_LEVEL)]
        hald_level: u32,

        /// Path to rawtherapee-cli binary.
        #[arg(long, default_value = "rawtherapee-cli")]
        rawtherapee: PathBuf,

        /// Path to convert binary.
        #[arg(long, default_value = "convert")]
        convert: PathBuf,

        /// Disable Lightroom XMP grain emulation.
        #[arg(long)]
        no_grain: bool,

        /// Minimum raw ISO for enabling RawTherapee directional pyramid color noise.
        /// Use 0 to disable color-noise processing.
        #[arg(long, default_value_t = 1600)]
        color_noise_iso_threshold: u32,

        /// Enable RawTherapee lens corrections.
        ///
        /// Without an explicit value, enables distortion, ca, and vignetting.
        /// Optionally pass a comma-separated list of:
        /// `distortion`, `ca`, `vignetting`.
        #[arg(long, num_args = 0..=1, value_parser = parse_lens_corrections_arg, default_missing_value = "all")]
        lens_corrections: Option<LensCorrections>,

        /// Optional Lensfun profile root for RawTherapee LCP profiles.
        ///
        /// If omitted, mini-film resolves from `MINI_FILM_LCP_ROOT` when set.
        #[arg(long)]
        lcp_root: Option<PathBuf>,

        /// Override grain as amount,size,frequency, each 0..100. Example: --grain 30,45,45
        #[arg(long)]
        grain: Option<String>,

        /// Built-in grain override: light, medium, or heavy.
        #[arg(long)]
        grain_preset: Option<String>,

        /// Base seed for deterministic generated grain. Defaults to current time of day.
        #[arg(long)]
        grain_seed: Option<u64>,

        /// Grain renderer to use for RAW outputs.
        #[arg(long, value_enum, default_value_t = GrainEngine::default())]
        grain_engine: GrainEngine,

        /// Number of files to process in parallel. Defaults to half of CPU threads.
        #[arg(long)]
        jobs: Option<usize>,

        /// Process only JPEG/HEIC inputs (JPG/JPEG/HEIC/HEIF).
        #[arg(long, conflicts_with = "input_raw_only")]
        input_jpg_only: bool,

        /// Process only RAW inputs (default RAW extensions and extensions supported by
        /// `--raw` operations).
        #[arg(long, conflicts_with = "input_jpg_only")]
        input_raw_only: bool,

        /// Output format for generated batch files.
        #[arg(long, value_enum, default_value_t = BatchOutputFormat::Jpg)]
        output_format: BatchOutputFormat,

        /// Create a batch gallery in the output directory (`index.html`) using
        /// the selected template.
        #[arg(long = "gallery", value_enum)]
        gallery: Option<GalleryTemplate>,

        /// Gallery thumbnail longest edge in pixels.
        #[arg(long = "gallery-thumbnail-long-edge", default_value_t = 1024)]
        gallery_thumbnail_long_edge: u32,

        /// Maximum thumbnails per gallery row.
        #[arg(long = "gallery-columns", default_value_t = 4)]
        gallery_columns: u32,

        /// JPEG quality for JPG batch outputs.
        #[arg(long, default_value_t = 95)]
        jpg_quality: u8,

        /// Resize final outputs with GraphicsMagick geometry, for example 3000x3000 or 3000x3000>.
        #[arg(long)]
        resize: Option<String>,

        /// Resize final outputs so the longest edge is at most this many pixels.
        #[arg(long)]
        long_edge: Option<u32>,

        /// Resize final outputs so width is at most this many pixels.
        #[arg(long)]
        max_width: Option<u32>,

        /// Resize final outputs so height is at most this many pixels.
        #[arg(long)]
        max_height: Option<u32>,

        /// JPEG chroma subsampling.
        #[arg(long, value_enum, default_value_t = JpegSubsampling::S444)]
        jpeg_subsampling: JpegSubsampling,

        /// Strip profiles and text metadata from final outputs.
        #[arg(long)]
        strip_metadata: bool,

        /// Write progressive/interlaced JPEGs.
        #[arg(long)]
        progressive_jpeg: bool,
    },

    /// Render every resolvable XMP profile as a structured contact-sheet thumbnail.
    Sampler {
        /// RAW file to use as the sampler source (supports common camera RAW formats
        /// like `.dng`, `.nef`, `.cr2`, `.cr3`, `.arw`, `.raf`, `.orf`, `.rw2`).
        raw: PathBuf,

        /// Output contact sheet path (.jpg/.jpeg or .html).
        #[arg(short, long)]
        output: PathBuf,

        /// Film library root. Sampler reads emulation XMPs from emulations/ and resolves RGBTables from profiles/.
        #[arg(long)]
        profiles_root: Option<PathBuf>,

        /// Directory containing generated cached Hald PNGs. Defaults to $HOME/.cache/mini-film/hald.
        #[arg(long)]
        hald_dir: Option<PathBuf>,

        /// Hald level to use for temporary XMP profile conversion.
        #[arg(short = 'l', long, default_value_t = DEFAULT_HALD_LEVEL)]
        hald_level: u32,

        /// Path to rawtherapee-cli binary.
        #[arg(long, default_value = "rawtherapee-cli")]
        rawtherapee: PathBuf,

        /// Path to convert binary.
        #[arg(long, default_value = "convert")]
        convert: PathBuf,

        /// Legacy compatibility option; sampler sheet assembly now uses convert.
        #[arg(long, default_value = "montage", hide = true)]
        montage: PathBuf,

        /// Disable Lightroom XMP grain emulation.
        #[arg(long)]
        no_grain: bool,

        /// Minimum raw ISO for enabling RawTherapee directional pyramid color noise.
        /// Use 0 to disable color-noise processing.
        #[arg(long, default_value_t = 1600)]
        color_noise_iso_threshold: u32,

        /// Enable RawTherapee lens corrections.
        ///
        /// Without an explicit value, enables distortion, ca, and vignetting.
        /// Optionally pass a comma-separated list of:
        /// `distortion`, `ca`, `vignetting`.
        #[arg(long, num_args = 0..=1, value_parser = parse_lens_corrections_arg, default_missing_value = "all")]
        lens_corrections: Option<LensCorrections>,

        /// Optional Lensfun profile root for RawTherapee LCP profiles.
        ///
        /// If omitted, mini-film resolves from `MINI_FILM_LCP_ROOT` when set.
        #[arg(long)]
        lcp_root: Option<PathBuf>,

        /// Base seed for deterministic generated grain. Defaults to current time of day.
        #[arg(long)]
        grain_seed: Option<u64>,

        /// Grain renderer to use for RAW outputs.
        #[arg(long, value_enum, default_value_t = GrainEngine::default())]
        grain_engine: GrainEngine,

        /// Disable /tmp sampler thumbnail cache and regenerate every profile thumbnail.
        #[arg(long)]
        no_cache: bool,

        /// Number of profiles to render in parallel. Defaults to half of CPU threads.
        #[arg(long)]
        jobs: Option<usize>,

        /// Thumbnail longest edge in pixels.
        #[arg(long, default_value_t = 512)]
        thumbnail_long_edge: u32,

        /// Maximum thumbnails per sampler row.
        #[arg(long, default_value_t = 8)]
        columns: u32,

        /// JPEG quality for thumbnails and JPEG contact sheets.
        #[arg(long, default_value_t = 95)]
        jpg_quality: u8,

        /// JPEG chroma subsampling for thumbnails and the final contact sheet.
        #[arg(long, value_enum, default_value_t = JpegSubsampling::S444)]
        jpeg_subsampling: JpegSubsampling,

        /// Strip profiles and text metadata from generated JPEGs.
        #[arg(long)]
        strip_metadata: bool,

        /// Write progressive/interlaced sampler JPEGs.
        #[arg(long = "progressive", alias = "progressive-jpeg")]
        progressive_jpeg: bool,
    },

    /// Watch an input inbox folder and apply optional profiles as files arrive.
    #[command(name = "daemon")]
    BatchDaemon {
        /// Input folder to watch recursively for new RAW and compressed image files.
        ///
        /// Without filters, both file groups are accepted: RAW (e.g. `.dng`, `.nef`, `.cr2`,
        /// `.cr3`, `.arw`, `.raf`, `.orf`, `.rw2`, ...) and JPEG/HEIC (`.jpg`, `.jpeg`,
        /// `.heic`, `.heif`).
        input: PathBuf,

        /// Output root folder. It is created if it does not exist.
        output: PathBuf,

        /// Profile selectors to apply to each incoming RAW or standalone JPEG/HEIC. Repeat for each profile.
        /// If omitted, each RAW is developed once with RawTherapee defaults and compressed inputs are converted directly.
        #[arg(short = 'p', long = "profile")]
        profile: Vec<String>,

        /// Directory containing generated cached Hald PNGs. Defaults to $HOME/.cache/mini-film/hald.
        #[arg(long)]
        hald_dir: Option<PathBuf>,

        /// Film library root. Emulation XMPs are selected from emulations/ and RGBTables from profiles/.
        #[arg(long)]
        profiles_root: Option<PathBuf>,

        /// Hald level used when --profile resolves to emulation XMPs.
        #[arg(short = 'l', long, default_value_t = DEFAULT_HALD_LEVEL)]
        hald_level: u32,

        /// Path to rawtherapee-cli binary.
        #[arg(long, default_value = "rawtherapee-cli")]
        rawtherapee: PathBuf,

        /// Path to convert binary.
        #[arg(long, default_value = "convert")]
        convert: PathBuf,

        /// Disable Lightroom XMP grain emulation.
        #[arg(long)]
        no_grain: bool,

        /// Minimum raw ISO for enabling RawTherapee directional pyramid color noise.
        /// Use 0 to disable color-noise processing.
        #[arg(long, default_value_t = 1600)]
        color_noise_iso_threshold: u32,

        /// Enable RawTherapee lens corrections.
        ///
        /// Without an explicit value, enables distortion, ca, and vignetting.
        /// Optionally pass a comma-separated list of:
        /// `distortion`, `ca`, `vignetting`.
        #[arg(long, num_args = 0..=1, value_parser = parse_lens_corrections_arg, default_missing_value = "all")]
        lens_corrections: Option<LensCorrections>,

        /// Optional Lensfun profile root for RawTherapee LCP profiles.
        ///
        /// If omitted, mini-film resolves from `MINI_FILM_LCP_ROOT` when set.
        #[arg(long)]
        lcp_root: Option<PathBuf>,

        /// Override grain as amount,size,frequency, for example 30,45,45.
        #[arg(long)]
        grain: Option<String>,

        /// Built-in grain preset: light, medium, or heavy.
        #[arg(long)]
        grain_preset: Option<String>,

        /// Base seed for deterministic generated grain. Defaults to current time of day.
        #[arg(long)]
        grain_seed: Option<u64>,

        /// Grain renderer to use for profiled outputs.
        #[arg(long, value_enum, default_value_t = GrainEngine::default())]
        grain_engine: GrainEngine,

        /// Number of files to process in parallel. Defaults to half of CPU threads.
        #[arg(long)]
        jobs: Option<usize>,

        /// Process only JPEG/HEIC inputs (JPG/JPEG/HEIC/HEIF).
        #[arg(long, conflicts_with = "input_raw_only")]
        input_jpg_only: bool,

        /// Process only RAW inputs (default RAW extensions and extensions supported by
        /// `--raw` operations).
        #[arg(long, conflicts_with = "input_jpg_only")]
        input_raw_only: bool,

        /// Debounce time in seconds for newly-created files when no inotify-style
        /// close/move completion notification is available.
        #[arg(long, default_value_t = 0)]
        debounce_seconds: u64,

        /// Also ingest RAW files from a paired Nikon Connect-to-PC / Wireless Transmitter Utility camera at this host/IP.
        #[arg(long)]
        nikon_wtu: Option<String>,

        /// Nikon PTP/IP port for --nikon-wtu.
        #[arg(long, default_value_t = 15740)]
        nikon_wtu_port: u16,

        /// Computer name sent to the Nikon camera during PTP/IP init.
        #[arg(long)]
        nikon_wtu_name: Option<String>,

        /// Stable 16-byte initiator GUID for Nikon pairing, as hex or colon-separated hex.
        #[arg(long)]
        nikon_wtu_guid: Option<String>,

        /// Serve the live review web UI at this host:port, for example 0.0.0.0:8090.
        #[arg(long)]
        review_address: Option<String>,

        /// Analyze rendered pictures with Codex using embedded RAW previews.
        ///
        /// Without an explicit value, generates tags only. Optionally pass a
        /// comma-separated list of: `tags`, `note`, `rating`, or `all`.
        ///
        /// Examples:
        /// - `--codex`
        /// - `--codex tags,note`
        /// - `--codex all`
        #[arg(long, num_args = 0..=1, value_parser = parse_codex_analysis_arg, default_missing_value = "tags")]
        codex: Option<CodexAnalysisFlags>,

        /// Path to codex CLI binary for --codex review analysis.
        #[arg(long, default_value = "codex")]
        codex_binary: PathBuf,

        /// Codex model used for --codex review analysis.
        #[arg(long, default_value = "gpt-5.4-mini")]
        codex_model: String,

        /// Timeout in seconds for each --codex image analysis.
        #[arg(long, default_value_t = 45)]
        codex_timeout: u64,

        /// Default gallery template for review publish jobs.
        #[arg(long = "gallery", value_enum)]
        gallery: Option<GalleryTemplate>,

        /// Default review publish gallery thumbnail longest edge in pixels.
        #[arg(long = "gallery-thumbnail-long-edge", default_value_t = 1024)]
        gallery_thumbnail_long_edge: u32,

        /// Default maximum thumbnails per review publish gallery row.
        #[arg(long = "gallery-columns", default_value_t = 4)]
        gallery_columns: u32,

        /// Default relative publish directory used by the review UI.
        #[arg(long, default_value = "published")]
        publish_album: String,

        /// Output format for generated files.
        #[arg(long, value_enum, default_value_t = BatchOutputFormat::Jpg)]
        output_format: BatchOutputFormat,

        /// JPEG quality for JPG outputs.
        #[arg(long, default_value_t = 95)]
        jpg_quality: u8,

        /// Resize final outputs with GraphicsMagick geometry, for example 3000x3000 or 3000x3000>.
        #[arg(long)]
        resize: Option<String>,

        /// Resize final outputs so the longest edge is at most this many pixels.
        #[arg(long)]
        long_edge: Option<u32>,

        /// Resize final outputs so width is at most this many pixels.
        #[arg(long)]
        max_width: Option<u32>,

        /// Resize final outputs so height is at most this many pixels.
        #[arg(long)]
        max_height: Option<u32>,

        /// JPEG chroma subsampling.
        #[arg(long, value_enum, default_value_t = JpegSubsampling::S444)]
        jpeg_subsampling: JpegSubsampling,

        /// Strip profiles and text metadata from final outputs.
        #[arg(long)]
        strip_metadata: bool,

        /// Write progressive/interlaced JPEG output.
        #[arg(long = "progressive", alias = "progressive-jpeg")]
        progressive_jpeg: bool,
    },

    /// Publish a daemon review state into a flat album folder.
    #[command(name = "review-publish")]
    ReviewPublish {
        /// Review state JSON generated by daemon review mode.
        #[arg(long)]
        state: PathBuf,

        /// Original daemon input root. RAW paths in the review state must stay inside it.
        #[arg(long)]
        input_root: PathBuf,

        /// Daemon output root. The album path is resolved inside this directory.
        #[arg(long)]
        output_root: PathBuf,

        /// Relative publish directory inside --output-root.
        #[arg(long)]
        album: String,

        /// Publish images rated at least this value.
        #[arg(long, default_value_t = 0)]
        min_rating: u8,

        /// Color label filter. Repeat to allow multiple labels.
        #[arg(long)]
        label: Vec<String>,

        /// Tag filter. Repeat to allow multiple tags.
        #[arg(long)]
        tag: Vec<String>,

        /// Output format for published files.
        #[arg(long, value_enum, default_value_t = BatchOutputFormat::Jpg)]
        output_format: BatchOutputFormat,

        /// Directory containing generated cached Hald PNGs. Defaults to $HOME/.cache/mini-film/hald.
        #[arg(long)]
        hald_dir: Option<PathBuf>,

        /// Film library root. Emulation XMPs are selected from emulations/ and RGBTables from profiles/.
        #[arg(long)]
        profiles_root: Option<PathBuf>,

        /// Hald level used when rerendering RAW files.
        #[arg(short = 'l', long, default_value_t = DEFAULT_HALD_LEVEL)]
        hald_level: u32,

        /// Path to rawtherapee-cli binary.
        #[arg(long, default_value = "rawtherapee-cli")]
        rawtherapee: PathBuf,

        /// Path to convert binary.
        #[arg(long, default_value = "convert")]
        convert: PathBuf,

        /// Number of gallery thumbnail jobs. Defaults to half of CPU threads.
        #[arg(long)]
        jobs: Option<usize>,

        /// Generate a gallery in the publish folder.
        #[arg(long = "gallery", value_enum)]
        gallery: Option<GalleryTemplate>,

        /// Gallery thumbnail longest edge in pixels.
        #[arg(long = "gallery-thumbnail-long-edge", default_value_t = 1024)]
        gallery_thumbnail_long_edge: u32,

        /// Maximum thumbnails per gallery row.
        #[arg(long = "gallery-columns", default_value_t = 4)]
        gallery_columns: u32,

        /// JPEG quality for JPG outputs.
        #[arg(long, default_value_t = 95)]
        jpg_quality: u8,

        /// Resize final outputs with GraphicsMagick geometry, for example 3000x3000 or 3000x3000>.
        #[arg(long)]
        resize: Option<String>,

        /// Resize final outputs so the longest edge is at most this many pixels.
        #[arg(long)]
        long_edge: Option<u32>,

        /// Resize final outputs so width is at most this many pixels.
        #[arg(long)]
        max_width: Option<u32>,

        /// Resize final outputs so height is at most this many pixels.
        #[arg(long)]
        max_height: Option<u32>,

        /// JPEG chroma subsampling.
        #[arg(long, value_enum, default_value_t = JpegSubsampling::S444)]
        jpeg_subsampling: JpegSubsampling,

        /// Strip profiles and text metadata from final outputs.
        #[arg(long)]
        strip_metadata: bool,

        /// Write progressive/interlaced JPEG output.
        #[arg(long = "progressive", alias = "progressive-jpeg")]
        progressive_jpeg: bool,

        /// Rerender selected outputs from original RAWs instead of linking existing reviewed files.
        #[arg(long)]
        rerender_raw: bool,

        /// Disable Lightroom XMP grain emulation when rerendering RAWs.
        #[arg(long)]
        no_grain: bool,

        /// Minimum raw ISO for enabling RawTherapee directional pyramid color noise.
        /// Use 0 to disable color-noise processing.
        #[arg(long, default_value_t = 1600)]
        color_noise_iso_threshold: u32,

        /// Enable RawTherapee lens corrections for rerendered RAWs.
        #[arg(long, num_args = 0..=1, value_parser = parse_lens_corrections_arg, default_missing_value = "all")]
        lens_corrections: Option<LensCorrections>,

        /// Optional Lensfun profile root for RawTherapee LCP profiles.
        ///
        /// If omitted, mini-film resolves from `MINI_FILM_LCP_ROOT` when set.
        #[arg(long)]
        lcp_root: Option<PathBuf>,

        /// Override grain as amount,size,frequency, for example 30,45,45.
        #[arg(long)]
        grain: Option<String>,

        /// Built-in grain preset: light, medium, or heavy.
        #[arg(long)]
        grain_preset: Option<String>,

        /// Base seed for deterministic generated grain when rerendering RAWs.
        #[arg(long)]
        grain_seed: Option<u64>,

        /// Grain renderer to use when rerendering RAWs.
        #[arg(long, value_enum, default_value_t = GrainEngine::default())]
        grain_engine: GrainEngine,

        /// Emit newline-delimited progress events for the daemon review UI.
        #[arg(long, hide = true)]
        progress_events: bool,
    },

    /// Check for a newer mini-film release and refresh the Lensfun database.
    Update,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum JpegSubsampling {
    S444,
    S422,
    S420,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodexAnalysisFlags {
    pub(crate) tags: bool,
    pub(crate) note: bool,
    pub(crate) rating: bool,
}

impl CodexAnalysisFlags {
    pub(crate) const fn tags_only() -> Self {
        Self {
            tags: true,
            note: false,
            rating: false,
        }
    }

    pub(crate) const fn all() -> Self {
        Self {
            tags: true,
            note: true,
            rating: true,
        }
    }

    pub(crate) const fn none() -> Self {
        Self {
            tags: false,
            note: false,
            rating: false,
        }
    }

    pub(crate) const fn is_enabled(self) -> bool {
        self.tags || self.note || self.rating
    }

    pub(crate) fn key(self) -> String {
        let mut parts = Vec::new();
        if self.tags {
            parts.push("tags");
        }
        if self.note {
            parts.push("note");
        }
        if self.rating {
            parts.push("rating");
        }
        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join("+")
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LensCorrections {
    pub(crate) distortion: bool,
    pub(crate) ca: bool,
    pub(crate) vignetting: bool,
}

impl LensCorrections {
    pub(crate) const fn all() -> Self {
        Self {
            distortion: true,
            ca: true,
            vignetting: true,
        }
    }

    pub(crate) const fn none() -> Self {
        Self {
            distortion: false,
            ca: false,
            vignetting: false,
        }
    }

    pub(crate) const fn is_enabled(self) -> bool {
        self.distortion || self.ca || self.vignetting
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum BatchOutputFormat {
    Jpg,
    Tiff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum GalleryTemplate {
    /// Light modern card grid with generous spacing.
    Modern,
    /// Softer palette and muted spacing for quiet browsing.
    Soft,
    /// Compact dense rows with smaller text.
    Compact,
    /// Asymmetric hero layout with larger emphasis on the first row.
    Hero,
    /// Dense square tiles like iOS Photos.
    Phone,
    /// Render all gallery templates into `<output>/<template>/index.html`.
    All,
}

impl std::fmt::Display for GalleryTemplate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GalleryTemplate::Modern => write!(formatter, "modern"),
            GalleryTemplate::Soft => write!(formatter, "soft"),
            GalleryTemplate::Compact => write!(formatter, "compact"),
            GalleryTemplate::Hero => write!(formatter, "hero"),
            GalleryTemplate::Phone => write!(formatter, "phone"),
            GalleryTemplate::All => write!(formatter, "all"),
        }
    }
}

impl GalleryTemplate {
    pub(crate) const fn concrete_templates() -> &'static [GalleryTemplate; 5] {
        &[
            GalleryTemplate::Modern,
            GalleryTemplate::Soft,
            GalleryTemplate::Compact,
            GalleryTemplate::Hero,
            GalleryTemplate::Phone,
        ]
    }

    pub(crate) fn is_all(self) -> bool {
        matches!(self, GalleryTemplate::All)
    }
}

fn parse_codex_analysis_arg(raw: &str) -> Result<CodexAnalysisFlags, String> {
    parse_codex_analysis(raw)
}

fn parse_codex_analysis(raw: &str) -> Result<CodexAnalysisFlags, String> {
    if raw.trim().is_empty() {
        return Err("--codex value cannot be empty".to_string());
    }
    let trimmed = raw.trim().to_ascii_lowercase();
    if matches!(trimmed.as_str(), "tags" | "tag") {
        return Ok(CodexAnalysisFlags::tags_only());
    }

    let mut flags = CodexAnalysisFlags::none();
    let mut saw_token = false;
    let mut seen_disabled = false;
    let mut seen_enabled = false;
    for token in raw.split(',') {
        let token = token.trim().to_ascii_lowercase();
        if token.is_empty() {
            return Err("--codex contains an empty token".to_string());
        }
        saw_token = true;
        match token.as_str() {
            "all" => {
                flags = CodexAnalysisFlags::all();
                seen_enabled = true;
            }
            "tags" | "tag" => {
                flags.tags = true;
                seen_enabled = true;
            }
            "note" | "notes" | "description" => {
                flags.note = true;
                seen_enabled = true;
            }
            "rating" | "rate" => {
                flags.rating = true;
                seen_enabled = true;
            }
            "none" | "off" => {
                seen_disabled = true;
            }
            _ => {
                return Err(format!(
                    "unsupported --codex token {token:?}; expected tags,note,rating,all"
                ));
            }
        }
    }

    if saw_token && seen_disabled && seen_enabled {
        return Err("--codex cannot mix disabled and enabled values".to_string());
    }
    if seen_disabled {
        return Ok(CodexAnalysisFlags::none());
    }
    if !saw_token {
        return Err("--codex requires at least one token".to_string());
    }
    Ok(flags)
}

fn parse_lens_corrections_arg(raw: &str) -> Result<LensCorrections, String> {
    parse_lens_corrections(raw)
}

fn parse_lens_corrections(raw: &str) -> Result<LensCorrections, String> {
    let mut correction = LensCorrections::none();
    if raw.trim().is_empty() {
        return Err("--lens-corrections value cannot be empty".to_string());
    }

    let mut saw_token = false;
    let mut seen_disabled = false;
    let mut seen_enabled = false;
    for token in raw.split(',') {
        let token = token.trim().to_ascii_lowercase();
        if token.is_empty() {
            return Err("--lens-corrections contains an empty token".to_string());
        }
        saw_token = true;
        match token.as_str() {
            "all" => {
                correction = LensCorrections::all();
                seen_enabled = true;
            }
            "distortion" => {
                correction.distortion = true;
                seen_enabled = true;
            }
            "ca" | "chromatic-aberration" | "chromatic_aberration" => {
                correction.ca = true;
                seen_enabled = true;
            }
            "vignetting" | "vignette" | "lens-vignetting" => {
                correction.vignetting = true;
                seen_enabled = true;
            }
            "none" | "off" => {
                seen_disabled = true;
            }
            _ => {
                return Err(format!(
                    "unsupported --lens-corrections token {token:?}; expected distortion,ca,vignetting,all"
                ));
            }
        }
    }

    if saw_token && seen_disabled && seen_enabled {
        return Err("--lens-corrections cannot mix disabled and enabled values".to_string());
    }
    if seen_disabled {
        return Ok(LensCorrections::none());
    }
    if !saw_token {
        return Err("--lens-corrections requires at least one token".to_string());
    }
    Ok(correction)
}

impl BatchOutputFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            BatchOutputFormat::Jpg => "jpg",
            BatchOutputFormat::Tiff => "tif",
        }
    }
}

impl std::fmt::Display for BatchOutputFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatchOutputFormat::Jpg => write!(formatter, "jpg"),
            BatchOutputFormat::Tiff => write!(formatter, "tiff"),
        }
    }
}

impl JpegSubsampling {
    pub(crate) fn graphicsmagick_sampling_factor(self) -> &'static str {
        match self {
            JpegSubsampling::S444 => "1x1,1x1,1x1",
            JpegSubsampling::S422 => "2x1,1x1,1x1",
            JpegSubsampling::S420 => "2x2,1x1,1x1",
        }
    }
}

impl std::fmt::Display for JpegSubsampling {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JpegSubsampling::S444 => write!(formatter, "s444"),
            JpegSubsampling::S422 => write!(formatter, "s422"),
            JpegSubsampling::S420 => write!(formatter, "s420"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExportOptions {
    pub(crate) jpg_quality: u8,
    pub(crate) resize: Option<String>,
    pub(crate) long_edge: Option<u32>,
    pub(crate) max_width: Option<u32>,
    pub(crate) max_height: Option<u32>,
    pub(crate) jpeg_subsampling: JpegSubsampling,
    pub(crate) strip_metadata: bool,
    pub(crate) progressive_jpeg: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_output_format_extensions_match_generated_files() {
        assert_eq!(BatchOutputFormat::Jpg.extension(), "jpg");
        assert_eq!(BatchOutputFormat::Tiff.extension(), "tif");
    }

    #[test]
    fn jpeg_subsampling_maps_to_graphicsmagick_sampling_factors() {
        assert_eq!(
            JpegSubsampling::S444.graphicsmagick_sampling_factor(),
            "1x1,1x1,1x1"
        );
        assert_eq!(
            JpegSubsampling::S422.graphicsmagick_sampling_factor(),
            "2x1,1x1,1x1"
        );
        assert_eq!(
            JpegSubsampling::S420.graphicsmagick_sampling_factor(),
            "2x2,1x1,1x1"
        );
    }

    #[test]
    fn cli_parses_level_16_as_the_hald_default_for_all_profile_commands() {
        let cli = Cli::parse_from(["mini-film", "hald", "profiles"]);
        assert!(matches!(
            cli.command,
            CommandKind::Hald { hald_level: 16, .. }
        ));

        let cli = Cli::parse_from(["mini-film", "info", "profile"]);
        assert!(matches!(
            cli.command,
            CommandKind::Info { hald_level: 16, .. }
        ));

        let cli = Cli::parse_from(["mini-film", "pp3", "profile"]);
        assert!(matches!(
            cli.command,
            CommandKind::Pp3 { hald_level: 16, .. }
        ));

        let cli = Cli::parse_from(["mini-film", "nikon", "profile", "--output", "out.ncp"]);
        assert!(matches!(
            cli.command,
            CommandKind::Nikon { hald_level: 16, .. }
        ));

        let cli = Cli::parse_from([
            "mini-film",
            "apply",
            "--output",
            "out.jpg",
            "--profile",
            "profile",
            "input.dng",
        ]);
        assert!(matches!(
            cli.command,
            CommandKind::Apply { hald_level: 16, .. }
        ));

        let cli = Cli::parse_from([
            "mini-film",
            "batch",
            "input-dir",
            "output-dir",
            "--profile",
            "profile",
        ]);
        assert!(matches!(
            cli.command,
            CommandKind::Batch { hald_level: 16, .. }
        ));

        let cli = Cli::parse_from(["mini-film", "sampler", "input.dng", "--output", "out.jpg"]);
        assert!(matches!(
            cli.command,
            CommandKind::Sampler { hald_level: 16, .. }
        ));

        let cli = Cli::parse_from([
            "mini-film",
            "sampler",
            "input.dng",
            "--output",
            "out.jpg",
            "--jobs",
            "8",
            "--columns",
            "4",
            "--no-cache",
        ]);
        assert!(matches!(
            cli.command,
            CommandKind::Sampler {
                jobs: Some(8),
                columns: 4,
                no_cache: true,
                ..
            }
        ));

        let cli = Cli::parse_from([
            "mini-film",
            "batch",
            "input-dir",
            "output-dir",
            "--profile",
            "profile",
            "--gallery",
            "soft",
            "--gallery-thumbnail-long-edge",
            "1024",
            "--gallery-columns",
            "5",
        ]);
        assert!(matches!(
            cli.command,
            CommandKind::Batch {
                gallery: Some(crate::cli::GalleryTemplate::Soft),
                gallery_thumbnail_long_edge: 1024,
                gallery_columns: 5,
                ..
            }
        ));

        let cli = Cli::parse_from([
            "mini-film",
            "batch",
            "input-dir",
            "output-dir",
            "--profile",
            "profile",
            "--gallery",
            "all",
        ]);
        assert!(matches!(
            cli.command,
            CommandKind::Batch {
                gallery: Some(crate::cli::GalleryTemplate::All),
                ..
            }
        ));

        let cli = Cli::parse_from([
            "mini-film",
            "daemon",
            "input-dir",
            "output-dir",
            "--profile",
            "portra 400 grainy",
            "--profile",
            "portra 400",
            "--jobs",
            "12",
            "--debounce-seconds",
            "15",
            "--output-format",
            "tiff",
        ]);
        assert!(matches!(
            cli.command,
            CommandKind::BatchDaemon {
                profile,
                jobs: Some(12),
                debounce_seconds: 15,
                output_format: crate::cli::BatchOutputFormat::Tiff,
                ..
            } if profile == vec!["portra 400 grainy", "portra 400"]
        ));

        let cli = Cli::parse_from([
            "mini-film",
            "daemon",
            "input-dir",
            "output-dir",
            "--profile",
            "scala",
            "--nikon-wtu",
            "192.168.1.50",
            "--nikon-wtu-name",
            "mini-film",
            "--nikon-wtu-guid",
            "000102030405060708090a0b0c0d0e0f",
        ]);
        assert!(matches!(
            cli.command,
            CommandKind::BatchDaemon {
                nikon_wtu: Some(camera),
                nikon_wtu_name: Some(name),
                nikon_wtu_guid: Some(guid),
                ..
            } if camera == "192.168.1.50"
                && name == "mini-film"
                && guid == "000102030405060708090a0b0c0d0e0f"
        ));

        let cli = Cli::parse_from([
            "mini-film",
            "daemon",
            "input-dir",
            "output-dir",
            "--profile",
            "scala",
            "--review-address",
            "0.0.0.0:8090",
            "--gallery",
            "phone",
            "--gallery-thumbnail-long-edge",
            "768",
            "--gallery-columns",
            "6",
        ]);
        assert!(matches!(
            cli.command,
            CommandKind::BatchDaemon {
                review_address: Some(address),
                gallery: Some(crate::cli::GalleryTemplate::Phone),
                gallery_thumbnail_long_edge: 768,
                gallery_columns: 6,
                ..
            } if address == "0.0.0.0:8090"
        ));

        let cli = Cli::parse_from(["mini-film", "update"]);
        assert!(matches!(cli.command, CommandKind::Update));

        let cli = Cli::parse_from(["mini-film", "app"]);
        assert!(matches!(cli.command, CommandKind::App));
    }

    #[test]
    fn cli_lens_corrections_disabled_by_default_and_can_be_enabled() {
        let cli = Cli::parse_from([
            "mini-film",
            "apply",
            "--output",
            "out.jpg",
            "--profile",
            "profile",
            "input.dng",
        ]);
        assert!(matches!(
            cli.command,
            CommandKind::Apply {
                lens_corrections: None,
                ..
            }
        ));

        let cli = Cli::parse_from([
            "mini-film",
            "apply",
            "input.dng",
            "--output",
            "out.jpg",
            "--profile",
            "profile",
            "--lens-corrections",
        ]);
        match cli.command {
            CommandKind::Apply {
                lens_corrections: Some(corrections),
                ..
            } => {
                assert!(corrections.distortion);
                assert!(corrections.ca);
                assert!(corrections.vignetting);
            }
            _ => panic!("wrong command"),
        }

        let cli = Cli::parse_from([
            "mini-film",
            "batch",
            "input-dir",
            "output-dir",
            "--profile",
            "profile",
            "--lens-corrections=distortion,ca",
        ]);
        match cli.command {
            CommandKind::Batch {
                lens_corrections: Some(corrections),
                ..
            } => {
                assert!(corrections.distortion);
                assert!(corrections.ca);
                assert!(!corrections.vignetting);
            }
            _ => panic!("wrong command"),
        }

        let cli = Cli::parse_from([
            "mini-film",
            "sampler",
            "input.dng",
            "--output",
            "sheet.jpg",
            "--lens-corrections",
            "vignetting",
        ]);
        match cli.command {
            CommandKind::Sampler {
                lens_corrections: Some(corrections),
                ..
            } => {
                assert!(!corrections.distortion);
                assert!(!corrections.ca);
                assert!(corrections.vignetting);
            }
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn cli_codex_defaults_to_tags_and_accepts_explicit_fields() {
        let cli = Cli::parse_from([
            "mini-film",
            "daemon",
            "input-dir",
            "output-dir",
            "--profile",
            "profile",
            "--review-address",
            "127.0.0.1:8090",
            "--codex",
        ]);
        match cli.command {
            CommandKind::BatchDaemon {
                codex: Some(flags), ..
            } => {
                assert!(flags.tags);
                assert!(!flags.note);
                assert!(!flags.rating);
            }
            _ => panic!("wrong command"),
        }

        let cli = Cli::parse_from([
            "mini-film",
            "daemon",
            "input-dir",
            "output-dir",
            "--profile",
            "profile",
            "--review-address",
            "127.0.0.1:8090",
            "--codex=tags,note,rating",
        ]);
        match cli.command {
            CommandKind::BatchDaemon {
                codex: Some(flags), ..
            } => {
                assert!(flags.tags);
                assert!(flags.note);
                assert!(flags.rating);
            }
            _ => panic!("wrong command"),
        }
    }

    #[test]
    fn cli_codex_rejects_unknown_tokens() {
        let error = Cli::try_parse_from([
            "mini-film",
            "daemon",
            "input-dir",
            "output-dir",
            "--profile",
            "profile",
            "--codex",
            "faces",
        ])
        .unwrap_err()
        .to_string();

        assert!(error.contains("unsupported --codex token \"faces\""));
    }

    #[test]
    fn cli_lens_corrections_rejects_unknown_tokens() {
        let error = Cli::try_parse_from([
            "mini-film",
            "apply",
            "--output",
            "out.jpg",
            "--profile",
            "profile",
            "--lens-corrections",
            "radial",
            "input.dng",
        ])
        .unwrap_err()
        .to_string();

        assert!(error.contains("unsupported --lens-corrections token \"radial\""));
    }

    #[test]
    fn cli_grain_engine_defaults_to_legacy_and_accepts_alternates() {
        let apply = Cli::parse_from(["mini-film", "apply", "input.dng", "--output", "out.jpg"]);
        assert!(matches!(
            apply.command,
            CommandKind::Apply {
                grain_engine: GrainEngine::Legacy,
                ..
            }
        ));

        let batch = Cli::parse_from([
            "mini-film",
            "batch",
            "input-dir",
            "output-dir",
            "--grain-engine",
            "legacy",
        ]);
        assert!(matches!(
            batch.command,
            CommandKind::Batch {
                grain_engine: GrainEngine::Legacy,
                ..
            }
        ));

        let exact = Cli::parse_from([
            "mini-film",
            "apply",
            "input.dng",
            "--output",
            "out.jpg",
            "--grain-engine",
            "rfgr",
        ]);
        assert!(matches!(
            exact.command,
            CommandKind::Apply {
                grain_engine: GrainEngine::Rfgr,
                ..
            }
        ));

        let fast = Cli::parse_from([
            "mini-film",
            "apply",
            "input.dng",
            "--output",
            "out.jpg",
            "--grain-engine",
            "rfgrfast",
        ]);
        assert!(matches!(
            fast.command,
            CommandKind::Apply {
                grain_engine: GrainEngine::RfgrFast,
                ..
            }
        ));

        let daemon = Cli::parse_from(["mini-film", "daemon", "input-dir", "output-dir"]);
        assert!(matches!(
            daemon.command,
            CommandKind::BatchDaemon {
                grain_engine: GrainEngine::Legacy,
                ..
            }
        ));

        let sampler = Cli::parse_from(["mini-film", "sampler", "input.dng", "--output", "out.jpg"]);
        assert!(matches!(
            sampler.command,
            CommandKind::Sampler {
                grain_engine: GrainEngine::Legacy,
                ..
            }
        ));

        let publish = Cli::parse_from([
            "mini-film",
            "review-publish",
            "--state",
            "state.json",
            "--input-root",
            "in",
            "--output-root",
            "out",
            "--album",
            "published",
        ]);
        assert!(matches!(
            publish.command,
            CommandKind::ReviewPublish {
                grain_engine: GrainEngine::Legacy,
                ..
            }
        ));
    }

    #[test]
    fn cli_grain_engine_rejects_unknown_values() {
        let error = Cli::try_parse_from([
            "mini-film",
            "apply",
            "input.dng",
            "--output",
            "out.jpg",
            "--grain-engine",
            "paper",
        ])
        .unwrap_err()
        .to_string();

        assert!(error.contains("invalid value 'paper'"));
    }

    #[test]
    fn gallery_template_concrete_list_excludes_all_variant() {
        let templates = GalleryTemplate::concrete_templates();
        assert_eq!(templates.len(), 5);
        assert!(
            !templates
                .iter()
                .any(|template| matches!(template, GalleryTemplate::All))
        );
        assert!(templates.contains(&GalleryTemplate::Modern));
        assert!(templates.contains(&GalleryTemplate::Phone));
    }
}
