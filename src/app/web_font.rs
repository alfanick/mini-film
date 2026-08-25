use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::OnceLock,
    thread,
};

use anyhow::{Context, Result, anyhow, bail};
use sha1::{Digest, Sha1};
use tempfile::NamedTempFile;
use ttf_parser::{Face, name_id};
use ttf2woff2::{BrotliQuality, EncodeOptions, encode_with_options};
use walkdir::WalkDir;

pub(crate) const FONT_FAMILY_ALIAS: &str = "Mini Film PragmataPro Mono Liga";
pub(crate) const FONT_ASSET_PREFIX: &str = "assets/fonts/";

const SOURCE_FAMILY: &str = "PragmataPro Mono Liga";
const CACHE_SCHEMA: &str = "ttf2woff2-q9-v1";
const CACHE_FAMILY: &str = "pragmata-pro-mono-liga";

static PRAGMATA_PRO_MONO_LIGA: OnceLock<Option<PreparedWebFontFamily>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WebFontStyle {
    Normal,
    Italic,
}

impl WebFontStyle {
    pub(crate) const fn css_value(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Italic => "italic",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedWebFontAsset {
    pub(crate) file_name: String,
    pub(crate) path: PathBuf,
    pub(crate) digest: String,
    pub(crate) content_type: &'static str,
}

impl PreparedWebFontAsset {
    pub(crate) fn etag(&self) -> String {
        format!("\"{}\"", self.digest)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedWebFontFace {
    pub(crate) weight: u16,
    pub(crate) style: WebFontStyle,
    pub(crate) source_path: PathBuf,
    pub(crate) asset: PreparedWebFontAsset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreparedWebFontFamily {
    pub(crate) stylesheet: PreparedWebFontAsset,
    pub(crate) faces: [PreparedWebFontFace; 4],
}

impl PreparedWebFontFamily {
    pub(crate) fn stylesheet_href(&self) -> String {
        format!("{FONT_ASSET_PREFIX}{}", self.stylesheet.file_name)
    }

    pub(crate) fn asset(&self, file_name: &str) -> Option<&PreparedWebFontAsset> {
        if self.stylesheet.file_name == file_name {
            return Some(&self.stylesheet);
        }
        self.faces
            .iter()
            .find(|face| face.asset.file_name == file_name)
            .map(|face| &face.asset)
    }

    fn sources_are_installed(&self) -> bool {
        self.faces.iter().all(|face| face.source_path.is_file())
    }
}

/// Prepare the installed official PragmataPro Mono Liga family once per process.
///
/// Absence (including an incomplete four-face family) is a quiet fallback. A
/// complete family that cannot be read, validated, converted, or cached emits a
/// single nonfatal warning and is then treated as absent for this process.
pub(crate) fn pragmata_pro_mono_liga() -> Option<&'static PreparedWebFontFamily> {
    PRAGMATA_PRO_MONO_LIGA
        .get_or_init(|| {
            let home = env::var_os("HOME").filter(|home| !home.is_empty())?;
            let home = PathBuf::from(home);
            let fonts_root = home.join(".fonts");
            let cache_dir = home
                .join(".cache/mini-film/web-fonts")
                .join(CACHE_FAMILY)
                .join(CACHE_SCHEMA);
            match prepare_family(&fonts_root, &cache_dir, &ProductionBackend) {
                Ok(family) => family,
                Err(error) => {
                    eprintln!("mini-film web font unavailable: {error:#}");
                    None
                }
            }
        })
        .as_ref()
        .filter(|family| family.sources_are_installed())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum FaceKind {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FaceKind {
    const ALL: [Self; 4] = [Self::Regular, Self::Bold, Self::Italic, Self::BoldItalic];

    const fn index(self) -> usize {
        match self {
            Self::Regular => 0,
            Self::Bold => 1,
            Self::Italic => 2,
            Self::BoldItalic => 3,
        }
    }

    const fn filename_code(self) -> &'static str {
        match self {
            Self::Regular => "R",
            Self::Bold => "B",
            Self::Italic => "I",
            Self::BoldItalic => "Z",
        }
    }

    const fn weight(self) -> u16 {
        match self {
            Self::Regular | Self::Italic => 400,
            Self::Bold | Self::BoldItalic => 700,
        }
    }

    const fn style(self) -> WebFontStyle {
        match self {
            Self::Regular | Self::Bold => WebFontStyle::Normal,
            Self::Italic | Self::BoldItalic => WebFontStyle::Italic,
        }
    }
}

#[derive(Clone, Debug)]
struct FaceSource {
    kind: FaceKind,
    path: PathBuf,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Revision {
    number: u64,
    spelling: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FamilyRevision {
    revision: Revision,
    directory: PathBuf,
}

#[derive(Default)]
struct CandidateFamily {
    paths: [Option<PathBuf>; 4],
}

impl CandidateFamily {
    fn insert(&mut self, kind: FaceKind, path: PathBuf) {
        let slot = &mut self.paths[kind.index()];
        if slot.as_ref().is_none_or(|current| path < *current) {
            *slot = Some(path);
        }
    }

    fn complete_sources(&self) -> Option<[FaceSource; 4]> {
        Some([
            FaceSource {
                kind: FaceKind::Regular,
                path: self.paths[0].clone()?,
            },
            FaceSource {
                kind: FaceKind::Bold,
                path: self.paths[1].clone()?,
            },
            FaceSource {
                kind: FaceKind::Italic,
                path: self.paths[2].clone()?,
            },
            FaceSource {
                kind: FaceKind::BoldItalic,
                path: self.paths[3].clone()?,
            },
        ])
    }
}

trait FontBackend: Sync {
    fn validate_source(&self, path: &Path, kind: FaceKind) -> Result<()>;
    fn encode(&self, source: &[u8]) -> Result<Vec<u8>>;
}

struct ProductionBackend;

impl FontBackend for ProductionBackend {
    fn validate_source(&self, path: &Path, kind: FaceKind) -> Result<()> {
        let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let face = Face::parse(&data, 0)
            .map_err(|error| anyhow!("parsing {}: {error}", path.display()))?;

        let has_expected_family = face.names().into_iter().any(|name| {
            matches!(name.name_id, name_id::FAMILY | name_id::TYPOGRAPHIC_FAMILY)
                && name.to_string().as_deref() == Some(SOURCE_FAMILY)
        });
        if !has_expected_family {
            bail!(
                "{} is not internally identified as {SOURCE_FAMILY}",
                path.display()
            );
        }
        if !face.is_monospaced() {
            bail!("{} is not internally marked monospaced", path.display());
        }
        if face.weight().to_number() != kind.weight()
            || face.is_italic() != (kind.style() == WebFontStyle::Italic)
        {
            bail!(
                "{} has the wrong internal weight or style for {}",
                path.display(),
                kind.filename_code()
            );
        }
        Ok(())
    }

    fn encode(&self, source: &[u8]) -> Result<Vec<u8>> {
        let options = EncodeOptions {
            quality: BrotliQuality::from(9),
            threads: None,
            ..EncodeOptions::default()
        };
        encode_with_options(source, options).context("encoding TTF as WOFF2")
    }
}

fn prepare_family<B: FontBackend>(
    fonts_root: &Path,
    cache_dir: &Path,
    backend: &B,
) -> Result<Option<PreparedWebFontFamily>> {
    let Some(sources) = discover_sources(fonts_root, backend)? else {
        return Ok(None);
    };

    fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating web-font cache {}", cache_dir.display()))?;

    let faces = thread::scope(|scope| -> Result<Vec<PreparedWebFontFace>> {
        let handles =
            sources.map(|source| scope.spawn(move || prepare_face(source, cache_dir, backend)));
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| anyhow!("web-font conversion worker panicked"))?
            })
            .collect()
    })?;
    let faces: [PreparedWebFontFace; 4] = faces
        .try_into()
        .map_err(|_| anyhow!("expected four prepared web-font faces"))?;

    let css = family_stylesheet(&faces);
    let css_digest = sha1_bytes(css.as_bytes());
    let css_file_name = format!("{CACHE_FAMILY}-{css_digest}.css");
    let css_path = cache_dir.join(&css_file_name);
    ensure_exact_file(&css_path, css.as_bytes())?;

    Ok(Some(PreparedWebFontFamily {
        stylesheet: PreparedWebFontAsset {
            file_name: css_file_name,
            path: css_path,
            digest: css_digest,
            content_type: "text/css; charset=utf-8",
        },
        faces,
    }))
}

fn discover_sources<B: FontBackend>(
    fonts_root: &Path,
    backend: &B,
) -> Result<Option<[FaceSource; 4]>> {
    let fonts_root = match fs::canonicalize(fonts_root) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("opening font directory {}", fonts_root.display()));
        }
    };

    let mut candidates: BTreeMap<FamilyRevision, CandidateFamily> = BTreeMap::new();
    for entry in WalkDir::new(&fonts_root).follow_links(false) {
        let entry = entry.with_context(|| format!("scanning {}", fonts_root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(file_name) = entry.file_name().to_str() else {
            continue;
        };
        let Some((revision, kind)) = parse_official_filename(file_name) else {
            continue;
        };
        let Some(directory) = entry.path().parent() else {
            continue;
        };
        candidates
            .entry(FamilyRevision {
                revision,
                directory: directory.to_owned(),
            })
            .or_default()
            .insert(kind, entry.into_path());
    }

    let mut rejected = Vec::new();
    for (revision, candidate) in candidates.iter().rev() {
        let Some(sources) = candidate.complete_sources() else {
            continue;
        };
        match validate_sources(&sources, backend) {
            Ok(()) => return Ok(Some(sources)),
            Err(error) => rejected.push(format!(
                "revision {} in {}: {error:#}",
                revision.revision.spelling,
                revision.directory.display()
            )),
        }
    }

    if rejected.is_empty() {
        Ok(None)
    } else {
        bail!(
            "installed {SOURCE_FAMILY} family is unusable ({})",
            rejected.join("; ")
        )
    }
}

fn validate_sources<B: FontBackend>(sources: &[FaceSource; 4], backend: &B) -> Result<()> {
    for source in sources {
        backend.validate_source(&source.path, source.kind)?;
    }
    Ok(())
}

fn parse_official_filename(file_name: &str) -> Option<(Revision, FaceKind)> {
    let stem = file_name.strip_suffix(".ttf")?;
    let rest = stem.strip_prefix("PragmataPro_Mono_")?;
    let (code, spelling) = rest.split_once("_liga_")?;
    if spelling.is_empty() || !spelling.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let kind = FaceKind::ALL
        .into_iter()
        .find(|kind| kind.filename_code() == code)?;
    Some((
        Revision {
            number: spelling.parse().ok()?,
            spelling: spelling.to_owned(),
        },
        kind,
    ))
}

fn prepare_face<B: FontBackend>(
    source: FaceSource,
    cache_dir: &Path,
    backend: &B,
) -> Result<PreparedWebFontFace> {
    let source_bytes = fs::read(&source.path)
        .with_context(|| format!("reading web font {}", source.path.display()))?;
    let source_digest = sha1_bytes(&source_bytes);
    let style = source.kind.style();
    let file_name = format!(
        "{CACHE_FAMILY}-{}-{}-{source_digest}.woff2",
        source.kind.weight(),
        style.css_value()
    );
    let path = cache_dir.join(&file_name);

    let woff2 = match fs::read(&path) {
        Ok(bytes) if valid_woff2(&bytes) => bytes,
        Ok(_) | Err(_) => {
            let bytes = backend.encode(&source_bytes)?;
            if !valid_woff2(&bytes) {
                bail!(
                    "WOFF2 encoder produced an invalid file for {}",
                    source.path.display()
                );
            }
            atomic_write(&path, &bytes)?;
            bytes
        }
    };
    let digest = sha1_bytes(&woff2);

    Ok(PreparedWebFontFace {
        weight: source.kind.weight(),
        style,
        source_path: source.path,
        asset: PreparedWebFontAsset {
            file_name,
            path,
            digest,
            content_type: "font/woff2",
        },
    })
}

fn family_stylesheet(faces: &[PreparedWebFontFace; 4]) -> String {
    let mut css = String::new();
    for face in faces {
        css.push_str("@font-face {\n");
        css.push_str(&format!("  font-family: '{FONT_FAMILY_ALIAS}';\n"));
        css.push_str(&format!(
            "  src: url(\"./{}\") format(\"woff2\");\n",
            face.asset.file_name
        ));
        css.push_str(&format!("  font-style: {};\n", face.style.css_value()));
        css.push_str(&format!("  font-weight: {};\n", face.weight));
        css.push_str("  font-display: swap;\n");
        css.push_str("}\n");
    }
    css
}

fn valid_woff2(bytes: &[u8]) -> bool {
    if bytes.len() < 48 || bytes.get(..4) != Some(b"wOF2") {
        return false;
    }
    let Some(length) = bytes.get(8..12) else {
        return false;
    };
    let declared = u32::from_be_bytes(length.try_into().expect("four-byte WOFF2 length"));
    usize::try_from(declared).ok() == Some(bytes.len())
}

fn ensure_exact_file(path: &Path, expected: &[u8]) -> Result<()> {
    if fs::read(path).is_ok_and(|contents| contents == expected) {
        return Ok(());
    }
    atomic_write(path, expected)
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("cache path {} has no parent", path.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary file in {}", parent.display()))?;
    temporary
        .write_all(contents)
        .with_context(|| format!("writing temporary web-font file for {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("syncing temporary web-font file for {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("installing cached web font {}", path.display()))?;
    Ok(())
}

fn sha1_bytes(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::SystemTime,
    };

    use super::*;
    use tempfile::tempdir;

    struct FakeBackend {
        encodes: AtomicUsize,
    }

    impl FakeBackend {
        fn new() -> Self {
            Self {
                encodes: AtomicUsize::new(0),
            }
        }

        fn encode_count(&self) -> usize {
            self.encodes.load(Ordering::SeqCst)
        }
    }

    impl FontBackend for FakeBackend {
        fn validate_source(&self, path: &Path, kind: FaceKind) -> Result<()> {
            let bytes = fs::read(path)?;
            let marker = format!("fake-font:{}:", kind.filename_code());
            if !bytes.starts_with(marker.as_bytes()) {
                bail!("wrong fake internal family or style in {}", path.display());
            }
            Ok(())
        }

        fn encode(&self, source: &[u8]) -> Result<Vec<u8>> {
            self.encodes.fetch_add(1, Ordering::SeqCst);
            let mut bytes = vec![0; 48];
            bytes[..4].copy_from_slice(b"wOF2");
            bytes[4..8].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
            bytes.extend_from_slice(source);
            let length = u32::try_from(bytes.len()).unwrap();
            bytes[8..12].copy_from_slice(&length.to_be_bytes());
            Ok(bytes)
        }
    }

    fn write_family(root: &Path, revision: &str, marker: &str) -> [PathBuf; 4] {
        fs::create_dir_all(root).unwrap();
        FaceKind::ALL.map(|kind| {
            let path = root.join(format!(
                "PragmataPro_Mono_{}_liga_{revision}.ttf",
                kind.filename_code()
            ));
            fs::write(
                &path,
                format!("fake-font:{}:{marker}", kind.filename_code()),
            )
            .unwrap();
            path
        })
    }

    fn modified(path: &Path) -> SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    #[test]
    fn absence_and_incomplete_family_are_quiet_fallbacks() {
        let temp = tempdir().unwrap();
        let fonts = temp.path().join("fonts");
        let cache = temp.path().join("cache");
        let backend = FakeBackend::new();

        assert!(prepare_family(&fonts, &cache, &backend).unwrap().is_none());
        fs::create_dir_all(&fonts).unwrap();
        for kind in [FaceKind::Regular, FaceKind::Bold, FaceKind::Italic] {
            fs::write(
                fonts.join(format!(
                    "PragmataPro_Mono_{}_liga_0903.ttf",
                    kind.filename_code()
                )),
                format!("fake-font:{}:incomplete", kind.filename_code()),
            )
            .unwrap();
        }
        assert!(prepare_family(&fonts, &cache, &backend).unwrap().is_none());
        assert!(!cache.exists());
    }

    #[cfg(unix)]
    #[test]
    fn canonicalizes_symlink_and_selects_newest_valid_complete_revision() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let real_fonts = temp.path().join("real-fonts");
        write_family(&real_fonts, "0902", "older");
        let newest = write_family(&real_fonts, "0903", "newer");
        fs::write(
            &newest[FaceKind::BoldItalic.index()],
            b"fake-font:I:wrong-style",
        )
        .unwrap();
        fs::write(
            real_fonts.join("PragmataProR_liga_9999.ttf"),
            b"proportional",
        )
        .unwrap();
        fs::create_dir(real_fonts.join("nerd")).unwrap();
        fs::write(
            real_fonts.join("nerd/PragmataProMonoLigaNerdFont-Regular.ttf"),
            b"nerd",
        )
        .unwrap();
        let fonts_link = temp.path().join(".fonts");
        symlink(&real_fonts, &fonts_link).unwrap();

        let family = prepare_family(&fonts_link, &temp.path().join("cache"), &FakeBackend::new())
            .unwrap()
            .unwrap();
        assert!(
            family
                .faces
                .iter()
                .all(|face| face.source_path.to_string_lossy().contains("0902"))
        );
    }

    #[test]
    fn cache_hits_reuse_woff2_without_touching_mtime() {
        let temp = tempdir().unwrap();
        let fonts = temp.path().join("fonts");
        let cache = temp.path().join("cache");
        write_family(&fonts, "0903", "first");
        let backend = FakeBackend::new();

        let first = prepare_family(&fonts, &cache, &backend).unwrap().unwrap();
        assert_eq!(backend.encode_count(), 4);
        let mtimes = first
            .faces
            .iter()
            .map(|face| modified(&face.asset.path))
            .collect::<Vec<_>>();
        let stylesheet_mtime = modified(&first.stylesheet.path);

        let second = prepare_family(&fonts, &cache, &backend).unwrap().unwrap();
        assert_eq!(backend.encode_count(), 4);
        assert_eq!(first, second);
        for (face, expected) in second.faces.iter().zip(mtimes) {
            assert_eq!(modified(&face.asset.path), expected);
        }
        assert_eq!(modified(&second.stylesheet.path), stylesheet_mtime);
    }

    #[test]
    fn source_replacement_invalidates_only_that_face_and_removal_disables_family() {
        let temp = tempdir().unwrap();
        let fonts = temp.path().join("fonts");
        let cache = temp.path().join("cache");
        let sources = write_family(&fonts, "0903", "first");
        let backend = FakeBackend::new();
        let first = prepare_family(&fonts, &cache, &backend).unwrap().unwrap();

        fs::write(&sources[0], b"fake-font:R:replacement").unwrap();
        let second = prepare_family(&fonts, &cache, &backend).unwrap().unwrap();
        assert_eq!(backend.encode_count(), 5);
        assert_ne!(
            first.faces[0].asset.file_name,
            second.faces[0].asset.file_name
        );
        assert_eq!(
            first.faces[1].asset.file_name,
            second.faces[1].asset.file_name
        );

        fs::remove_file(&sources[3]).unwrap();
        assert!(prepare_family(&fonts, &cache, &backend).unwrap().is_none());
    }

    #[test]
    fn corrupt_cached_woff2_is_repaired_atomically() {
        let temp = tempdir().unwrap();
        let fonts = temp.path().join("fonts");
        let cache = temp.path().join("cache");
        write_family(&fonts, "0903", "first");
        let backend = FakeBackend::new();
        let first = prepare_family(&fonts, &cache, &backend).unwrap().unwrap();
        let corrupt = &first.faces[2].asset.path;
        fs::write(corrupt, b"wOF2bad-length").unwrap();

        let repaired = prepare_family(&fonts, &cache, &backend).unwrap().unwrap();
        assert_eq!(backend.encode_count(), 5);
        assert!(valid_woff2(
            &fs::read(&repaired.faces[2].asset.path).unwrap()
        ));
    }

    #[test]
    fn generated_stylesheet_and_asset_registry_are_content_addressed() {
        let temp = tempdir().unwrap();
        let fonts = temp.path().join("fonts");
        write_family(&fonts, "0903", "css");
        let family = prepare_family(&fonts, &temp.path().join("cache"), &FakeBackend::new())
            .unwrap()
            .unwrap();
        let css = fs::read_to_string(&family.stylesheet.path).unwrap();

        assert_eq!(css.matches("@font-face").count(), 4);
        assert!(css.contains(FONT_FAMILY_ALIAS));
        assert!(css.contains("font-weight: 400"));
        assert!(css.contains("font-weight: 700"));
        assert!(css.contains("font-style: normal"));
        assert!(css.contains("font-style: italic"));
        for face in &family.faces {
            assert!(css.contains(&face.asset.file_name));
            assert_eq!(family.asset(&face.asset.file_name), Some(&face.asset));
        }
        assert!(
            family
                .stylesheet
                .file_name
                .contains(&sha1_bytes(css.as_bytes()))
        );
        assert_eq!(
            family.stylesheet_href(),
            format!("{FONT_ASSET_PREFIX}{}", family.stylesheet.file_name)
        );
        assert_eq!(family.asset("../anything.woff2"), None);
        assert_eq!(
            family.stylesheet.etag(),
            format!("\"{}\"", family.stylesheet.digest)
        );
    }

    #[test]
    #[ignore = "requires the locally licensed PragmataPro family"]
    fn installed_official_family_converts_to_readable_woff2_when_present() {
        let Some(home) = env::var_os("HOME") else {
            return;
        };
        let fonts = PathBuf::from(home).join(".fonts");
        let cache = tempdir().unwrap();
        let family = prepare_family(&fonts, cache.path(), &ProductionBackend).unwrap();
        let Some(family) = family else {
            return;
        };
        assert_eq!(family.faces.len(), 4);
        for face in family.faces {
            let bytes = fs::read(face.asset.path).unwrap();
            assert!(valid_woff2(&bytes));
        }
    }
}
