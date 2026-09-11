//! Resolve existing media routes for authorized local clients without transferring image bytes or editing review data.

use super::*;
#[cfg(unix)]
use std::io::{Read, Write};
use std::net::SocketAddr;

/// A private filesystem capability prevents browsers and reverse proxies from discovering local media paths.
#[derive(Clone)]
pub(super) struct LocalMediaAccess {
    token: Option<String>,
}

impl LocalMediaAccess {
    /// Enable local lookup only when a secure per-catalog capability can be created or reused.
    pub(super) fn prepare(handle: &ReviewHandle) -> Self {
        match capability(&handle.cache_root) {
            Ok(token) => Self { token: Some(token) },
            Err(error) => {
                #[cfg(unix)]
                eprintln!("Local media-path access unavailable: {error}");
                #[cfg(not(unix))]
                let _ = error;
                Self { token: None }
            }
        }
    }

    /// Require actual loopback transport, an unforwarded non-browser JSON request, and the filesystem capability.
    fn permits(&self, peer: Option<SocketAddr>, headers: &HeaderMap) -> bool {
        let Some(expected) = self.token.as_deref() else {
            return false;
        };
        if !peer.is_some_and(|peer| peer.ip().is_loopback())
            || headers.keys().any(|name| {
                let name = name.as_str();
                name == "origin"
                    || name == "forwarded"
                    || name.starts_with("x-forwarded-")
                    || name.starts_with("sec-fetch-")
            })
            || headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_none_or(|value| {
                    value.split(';').next().unwrap_or("").trim() != "application/json"
                })
        {
            return false;
        }
        let Some(supplied) = headers
            .get("x-mini-film-local-access")
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };
        supplied.len() == expected.len()
            && supplied
                .bytes()
                .zip(expected.bytes())
                .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
                == 0
    }
}

/// Resolve on a blocking worker because existing lazy preview helpers may materialize their normal render cache.
pub(super) async fn response(
    handle: &ReviewHandle,
    access: &LocalMediaAccess,
    peer: Option<SocketAddr>,
    headers: &HeaderMap,
    body: &[u8],
) -> Response {
    if !access.permits(peer, headers) {
        return json_error(403, anyhow!("local media access denied")).into_response();
    }
    let request = match serde_json::from_slice::<wire::ReviewMediaPathRequest>(body) {
        Ok(request) => request,
        Err(_) => return json_error(400, anyhow!("invalid local media request")).into_response(),
    };
    let handle = handle.clone();
    match tokio::task::spawn_blocking(move || resolve(&handle, &request)).await {
        Ok(Ok(response)) => match serde_json::to_string(&response) {
            Ok(body) => {
                let mut response =
                    text_response(200, "application/json; charset=utf-8", &body).into_response();
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                response
            }
            Err(error) => json_error(500, error.into()).into_response(),
        },
        Ok(Err(_)) => {
            json_error(404, anyhow!("matching local media is unavailable")).into_response()
        }
        Err(error) => {
            json_error(500, anyhow!("local media lookup failed: {error}")).into_response()
        }
    }
}

/// Match only explicit media routes; URLs and traversal never become arbitrary filesystem lookup requests.
fn resolve(
    handle: &ReviewHandle,
    request: &wire::ReviewMediaPathRequest,
) -> Result<wire::ReviewMediaPathResponse> {
    let catalog = handle.state_path().canonicalize()?;
    if Path::new(&request.catalog_path).canonicalize()? != catalog {
        bail!("catalog mismatch");
    }
    let path = request.path.trim_start_matches('/');
    if path.is_empty() || request.path.starts_with("//") || path.contains(['?', '#', '\\', '\0']) {
        bail!("invalid media route");
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        bail!("invalid media route component");
    }
    let file = match parts.as_slice() {
        ["media", id] => handle.full_media_path(id.parse()?)?,
        ["media", id, index] => handle.media_path(id.parse()?, index.parse()?)?,
        ["media", id, index, "base"] => {
            handle.profile_base_media_path(id.parse()?, index.parse()?)?
        }
        ["preview", id] => handle.preview_media_path(id.parse()?)?,
        ["thumbnail", id] => handle.thumbnail_media_path(id.parse()?)?,
        ["original", id] => handle.original_media_path(id.parse()?)?,
        ["crop-source", id] => handle.crop_source_media_path(id.parse()?)?,
        ["full-preview", id] => handle.rendered_full_preview_media_path(id.parse()?)?,
        ["api", "profile", index, "hald"] => handle.profile_hald_path(index.parse()?)?,
        ["diffusion-preview", id, "before"] => {
            handle.diffusion_preview_media_path(id.parse()?, false)?
        }
        ["diffusion-preview", id, "after"] => {
            handle.diffusion_preview_media_path(id.parse()?, true)?
        }
        ["sampler-media", id, key] => handle.sampler_media_path(id.parse()?, key)?,
        ["panorama-preview", id, mode, projection] => handle.panorama_preview_media_path(
            id.parse()?,
            parse_panorama_matching(mode)?,
            parse_panorama_projection(projection)?,
        )?,
        ["outputs", rest @ ..] => {
            let decoded =
                decode_output_path(&rest.join("/")).map_err(|()| anyhow!("invalid output path"))?;
            let relative =
                sanitize_output_path(&decoded).map_err(|()| anyhow!("invalid output path"))?;
            let output = handle.output_root().canonicalize()?;
            let file = output.join(relative).canonicalize()?;
            if !file.starts_with(output)
                || !matches!(
                    review_media_content_type(&file),
                    "image/jpeg" | "image/tiff" | "image/heic" | "image/heif"
                )
            {
                bail!("not an exported image");
            }
            file
        }
        _ => bail!("unknown media route"),
    };
    let file = file.canonicalize()?;
    if !file.is_file() {
        bail!("media is not a regular file");
    }
    Ok(wire::ReviewMediaPathResponse {
        path: file.to_str().context("media path is not UTF-8")?.into(),
        catalog_path: catalog
            .to_str()
            .context("catalog path is not UTF-8")?
            .into(),
    })
}

/// Use operating-system randomness and a mode-0600 regular file; a symlink or permissive preexisting file is rejected.
#[cfg(unix)]
fn capability(cache: &Path) -> Result<String> {
    let path = cache.join(".mini-film-local-media-token");
    let mut random = [0_u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    // Publish only a complete, synced private file; concurrent daemon starts reuse the winner's capability.
    let mut temporary = tempfile::NamedTempFile::new_in(cache)?;
    temporary.write_all(token.as_bytes())?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(&path) {
        Ok(_) => Ok(token),
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            read_capability(&path)
        }
        Err(error) => Err(error.into()),
    }
}

/// Reuse only a complete private regular capability, leaving invalid existing files untouched for diagnosis.
#[cfg(unix)]
fn read_capability(path: &Path) -> Result<String> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        bail!("insecure local media capability file");
    }
    let mut token = String::new();
    std::fs::File::open(path)?
        .take(65)
        .read_to_string(&mut token)?;
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid local media capability file");
    }
    Ok(token)
}

/// The GTK local-filesystem capability is unavailable on platforms without the required private-file semantics.
#[cfg(not(unix))]
fn capability(_cache: &Path) -> Result<String> {
    bail!("local media lookup requires a Unix daemon");
}
