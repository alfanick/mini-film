use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

const DEFAULT_HALD_LEVEL: u32 = 16;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Develop RAW files with Lightroom-style film profile Hald CLUTs"
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: CommandKind,
}

#[derive(Subcommand, Debug)]
pub(crate) enum CommandKind {
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

    /// Run RawTherapee, then apply a Hald CLUT with GraphicsMagick/ImageMagick convert.
    Apply {
        /// RAW file to develop (supports common camera RAW formats such as `.dng`,
        /// `.nef`, `.cr2`, `.cr3`, `.arw`, `.raf`, `.orf`, `.rw2`).
        raw: PathBuf,

        /// Output image path.
        #[arg(short, long)]
        output: PathBuf,

        /// Profile selector: Hald PNG path/name, emulation XMP path/name, or RawTherapee PP3 path.
        #[arg(short, long)]
        profile: String,

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

        /// Override grain as amount,size,frequency, each 0..100. Example: --grain 30,45,45
        #[arg(long)]
        grain: Option<String>,

        /// Built-in grain override: light, medium, or heavy.
        #[arg(long)]
        grain_preset: Option<String>,

        /// Seed for deterministic generated grain. Defaults to current time of day.
        #[arg(long)]
        grain_seed: Option<u64>,

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

    /// Apply a profile to every supported RAW file in an input folder.
    Batch {
        /// Input folder scanned recursively for supported RAW files (case-insensitive), e.g.
        /// `.dng`, `.nef`, `.cr2`, `.cr3`, `.arw`, `.raf`, `.orf`, `.rw2`, ...
        input: PathBuf,

        /// Output folder. It is created if it does not exist.
        output: PathBuf,

        /// Profile selector: Hald PNG path/name, emulation XMP path/name, or RawTherapee PP3 path.
        #[arg(short, long)]
        profile: String,

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

        /// Override grain as amount,size,frequency, each 0..100. Example: --grain 30,45,45
        #[arg(long)]
        grain: Option<String>,

        /// Built-in grain override: light, medium, or heavy.
        #[arg(long)]
        grain_preset: Option<String>,

        /// Base seed for deterministic generated grain. Defaults to current time of day.
        #[arg(long)]
        grain_seed: Option<u64>,

        /// Number of RAW files to process in parallel. Defaults to half of CPU threads.
        #[arg(long)]
        jobs: Option<usize>,

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

        /// Base seed for deterministic generated grain. Defaults to current time of day.
        #[arg(long)]
        grain_seed: Option<u64>,

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

    /// Watch an input inbox folder and apply one or more profiles as files arrive.
    #[command(name = "daemon")]
    BatchDaemon {
        /// Input folder to watch recursively for new RAW files.
        input: PathBuf,

        /// Output root folder. It is created if it does not exist.
        output: PathBuf,

        /// Profile selectors to apply to each incoming RAW. Repeat this option for each profile.
        /// Profiles are rendered to output files using their profile stems.
        #[arg(short = 'p', long = "profile", required = true)]
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

        /// Override grain as amount,size,frequency, for example 30,45,45.
        #[arg(long)]
        grain: Option<String>,

        /// Built-in grain preset: light, medium, or heavy.
        #[arg(long)]
        grain_preset: Option<String>,

        /// Base seed for deterministic generated grain. Defaults to current time of day.
        #[arg(long)]
        grain_seed: Option<u64>,

        /// Number of files to process in parallel. Defaults to half of CPU threads.
        #[arg(long)]
        jobs: Option<usize>,

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum JpegSubsampling {
    S444,
    S422,
    S420,
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

impl BatchOutputFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            BatchOutputFormat::Jpg => "jpg",
            BatchOutputFormat::Tiff => "tif",
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

#[derive(Clone, Debug)]
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
