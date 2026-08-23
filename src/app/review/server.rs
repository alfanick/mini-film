use std::{
    convert::Infallible,
    path::{Path, PathBuf},
};

use async_stream::stream;
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::State,
    http::{HeaderValue, Method, Request, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::{gallery_download::build_gallery_archive, model::*, prelude::*, sampler::*};

pub(super) fn run_review_listener(
    listener: std::net::TcpListener,
    handle: ReviewHandle,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("mini-film-review-async")
        .enable_all()
        .build()
        .context("building review HTTP runtime")?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener)
            .context("creating async review listener")?;
        axum::serve(listener, review_router(handle))
            .await
            .context("running review HTTP server")
    })
}

fn review_router(handle: ReviewHandle) -> Router {
    Router::new().fallback(review_request).with_state(handle)
}

async fn review_request(State(handle): State<ReviewHandle>, request: Request<Body>) -> Response {
    let (parts, body) = request.into_parts();
    let path = review_route_path(parts.uri.path());
    if parts.method == Method::GET && path == "/api/events" {
        return event_stream_response(handle);
    }

    let body = match to_bytes(body, 16 * 1024 * 1024).await {
        Ok(body) => body,
        Err(error) => {
            return json_error(400, anyhow!("reading HTTP request body: {error}")).into_response();
        }
    };
    route_request(parts.method, &path, body, &handle).await
}

pub(super) struct HttpResponse {
    pub(super) status: u16,
    pub(super) content_type: &'static str,
    pub(super) body: Vec<u8>,
}

pub(super) async fn route_request(
    method: Method,
    path: &str,
    body: Bytes,
    handle: &ReviewHandle,
) -> Response {
    match (method.clone(), path) {
        (Method::GET, "/") | (Method::GET, "/review") => {
            text_response(200, "text/html; charset=utf-8", &review_index_html()).into_response()
        }
        (Method::GET, "/tv") => {
            text_response(200, "text/html; charset=utf-8", &review_tv_html()).into_response()
        }
        (Method::GET, "/assets/styles.css") => {
            text_response(200, "text/css; charset=utf-8", review_styles()).into_response()
        }
        (Method::GET, "/assets/app.js") => text_response(
            200,
            "application/javascript; charset=utf-8",
            review_script(),
        )
        .into_response(),
        (Method::GET, _) if path.starts_with("/assets/vendor/") => {
            let asset_path = path.trim_start_matches("/assets/");
            match review_text_asset(asset_path) {
                Some(body) => {
                    text_response(200, review_asset_content_type(asset_path), body).into_response()
                }
                None => {
                    text_response(404, "text/plain; charset=utf-8", "not found").into_response()
                }
            }
        }
        (Method::GET, "/api/state") => match handle.api_state_json() {
            Ok(body) => {
                text_response(200, "application/json; charset=utf-8", &body).into_response()
            }
            Err(error) => json_error(500, error).into_response(),
        },
        (Method::POST, "/api/review") => {
            if let Err(error) = handle.ensure_database_healthy() {
                return json_error(503, error).into_response();
            }
            let previous = match handle.api_state_value() {
                Ok(state) => state,
                Err(error) => return json_error(500, error).into_response(),
            };
            let result = match serde_json::from_slice::<ReviewUpdateRequest>(&body)
                .context("parsing review update")
            {
                Ok(update) => handle
                    .apply_review_update_async(update)
                    .await
                    .and_then(|()| handle.api_state_patch_json_since(&previous)),
                Err(error) => Err(error),
            };
            match result {
                Ok(body) => {
                    text_response(200, "application/json; charset=utf-8", &body).into_response()
                }
                Err(error) if handle.ensure_database_healthy().is_err() => {
                    json_error(503, error).into_response()
                }
                Err(error) => json_error(400, error).into_response(),
            }
        }
        (Method::POST, "/api/ui") => {
            if let Err(error) = handle.ensure_database_healthy() {
                return json_error(503, error).into_response();
            }
            let previous = match handle.api_state_value() {
                Ok(state) => state,
                Err(error) => return json_error(500, error).into_response(),
            };
            let result = match serde_json::from_slice::<ReviewUiUpdateRequest>(&body)
                .context("parsing review UI update")
            {
                Ok(update) => handle
                    .apply_ui_update_async(update)
                    .await
                    .and_then(|()| handle.api_state_patch_json_since(&previous)),
                Err(error) => Err(error),
            };
            match result {
                Ok(body) => {
                    text_response(200, "application/json; charset=utf-8", &body).into_response()
                }
                Err(error) if handle.ensure_database_healthy().is_err() => {
                    json_error(503, error).into_response()
                }
                Err(error) => json_error(400, error).into_response(),
            }
        }
        (Method::PATCH, _) if path.starts_with("/api/bursts/") => {
            if let Err(error) = handle.ensure_database_healthy() {
                return json_error(503, error).into_response();
            }
            let burst_id = path.trim_start_matches("/api/bursts/");
            if burst_id.is_empty() || burst_id.contains('/') {
                return json_error(404, anyhow!("review burst was not found")).into_response();
            }
            let previous = match handle.api_state_value() {
                Ok(state) => state,
                Err(error) => return json_error(500, error).into_response(),
            };
            let result = match serde_json::from_slice::<ReviewBurstExpansionRequest>(&body)
                .context("parsing burst expansion update")
            {
                Ok(update) => handle
                    .apply_burst_expansion_async(burst_id, update)
                    .await
                    .and_then(|()| handle.api_state_patch_json_since(&previous)),
                Err(error) => Err(error),
            };
            match result {
                Ok(body) => {
                    text_response(200, "application/json; charset=utf-8", &body).into_response()
                }
                Err(error) if handle.ensure_database_healthy().is_err() => {
                    json_error(503, error).into_response()
                }
                Err(error) => json_error(400, error).into_response(),
            }
        }
        (Method::POST, "/api/publish") => {
            let previous = match handle.api_state_value() {
                Ok(state) => state,
                Err(error) => return json_error(500, error).into_response(),
            };
            match parse_publish_request(&body)
                .and_then(|request| handle.start_publish_job(request))
                .and_then(|_| handle.api_state_patch_json_since(&previous))
            {
                Ok(body) => {
                    text_response(200, "application/json; charset=utf-8", &body).into_response()
                }
                Err(error) => json_error(500, error).into_response(),
            }
        }
        (Method::POST, "/api/sampler/jobs") => sampler_start_response(&body, handle).await,
        (Method::GET, _) | (Method::POST, _) if path.starts_with("/api/sampler/jobs/") => {
            sampler_job_response(method, path, &body, handle).await
        }
        (Method::POST, "/api/diffusion/jobs") => diffusion_start_response(&body, handle).await,
        (Method::GET, _) if path.starts_with("/api/diffusion/jobs/") => {
            diffusion_job_response(path, handle).await
        }
        (Method::GET, _) if path.starts_with("/diffusion-preview/") => {
            diffusion_preview_media_response(path, handle).await
        }
        (Method::POST, "/api/diffusion/settings") | (Method::DELETE, "/api/diffusion/settings") => {
            diffusion_settings_response(method, &body, handle).await
        }
        (Method::POST, "/api/panoramas") => panorama_create_response(&body, handle).await,
        (Method::PATCH, _) | (Method::POST, _) if path.starts_with("/api/panoramas/") => {
            panorama_project_response(method, path, &body, handle).await
        }
        (Method::GET, _) if path.starts_with("/api/publish/") => {
            gallery_archive_response(path, handle).await
        }
        (Method::GET, _) if path.starts_with("/api/profile/") => {
            profile_asset_response(path, handle).await
        }
        (Method::GET, _) if path.starts_with("/media/") => media_response(path, handle).await,
        (Method::GET, _) if path.starts_with("/crop-source/") => {
            crop_source_response(path, handle).await
        }
        (Method::GET, _) if path.starts_with("/original/") => original_response(path, handle).await,
        (Method::GET, _) if path.starts_with("/full-preview/") => {
            full_preview_response(path, handle).await
        }
        (Method::GET, _) if path.starts_with("/outputs/") => outputs_response(path, handle).await,
        (Method::GET, _) if path.starts_with("/thumbnail/") => {
            thumbnail_response(path, handle).await
        }
        (Method::GET, _) if path.starts_with("/preview/") => preview_response(path, handle).await,
        (Method::GET, _) if path.starts_with("/panorama-preview/") => {
            panorama_preview_response(path, handle).await
        }
        (Method::GET, _) if path.starts_with("/sampler-media/") => {
            sampler_media_response(path, handle).await
        }
        _ => text_response(404, "text/plain; charset=utf-8", "not found").into_response(),
    }
}

async fn gallery_archive_response(path: &str, handle: &ReviewHandle) -> Response {
    let parts = path
        .trim_start_matches("/api/publish/")
        .split('/')
        .collect::<Vec<_>>();
    let [job_id, "gallery.zip"] = parts.as_slice() else {
        return text_response(404, "text/plain; charset=utf-8", "not found").into_response();
    };
    let job_id = match job_id.parse::<u64>() {
        Ok(job_id) => job_id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad publish job id")
                .into_response();
        }
    };
    let spec = match handle.gallery_archive_spec(job_id) {
        Ok(spec) => spec,
        Err(error) => return json_error(404, error).into_response(),
    };
    let download_name = spec.download_name().to_string();
    match tokio::task::spawn_blocking(move || build_gallery_archive(&spec)).await {
        Ok(Ok(path)) => serve_gallery_archive(path, &download_name).await,
        Ok(Err(error)) => json_error(500, error).into_response(),
        Err(error) => json_error(500, anyhow!("building gallery archive: {error}")).into_response(),
    }
}

pub(super) fn review_asset_content_type(path: &str) -> &'static str {
    if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else {
        "text/plain; charset=utf-8"
    }
}

pub(super) fn review_route_path(path: &str) -> String {
    for marker in [
        "/api/",
        "/assets/",
        "/media/",
        "/crop-source/",
        "/original/",
        "/full-preview/",
        "/outputs/",
        "/thumbnail/",
        "/preview/",
        "/diffusion-preview/",
        "/panorama-preview/",
        "/sampler-media/",
    ] {
        if let Some(index) = path.find(marker) {
            return path[index..].to_string();
        }
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    if trimmed.ends_with("/review") {
        return "/review".to_string();
    }
    if trimmed.ends_with("/tv") {
        return "/tv".to_string();
    }
    if !trimmed
        .rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'))
    {
        return "/".to_string();
    }
    path.to_string()
}

async fn sampler_start_response(body: &[u8], handle: &ReviewHandle) -> Response {
    if let Err(error) = handle.ensure_database_healthy() {
        return json_error(503, error).into_response();
    }
    let result = serde_json::from_slice::<ReviewSamplerStartRequest>(body)
        .context("parsing sampler request")
        .and_then(|request| handle.start_sampler_job(request.image_id))
        .and_then(|snapshot| serde_json::to_string(&snapshot).context("serializing sampler job"));
    match result {
        Ok(body) => text_response(202, "application/json; charset=utf-8", &body).into_response(),
        Err(error) => json_error(400, error).into_response(),
    }
}

async fn diffusion_start_response(body: &[u8], handle: &ReviewHandle) -> Response {
    if let Err(error) = handle.ensure_database_healthy() {
        return json_error(503, error).into_response();
    }
    let result = serde_json::from_slice::<ReviewDiffusionJobRequest>(body)
        .context("parsing diffusion preview request")
        .and_then(|request| handle.start_diffusion_job(request))
        .and_then(|job| serde_json::to_string(&job).context("serializing diffusion job"));
    match result {
        Ok(body) => text_response(202, "application/json; charset=utf-8", &body).into_response(),
        Err(error) => json_error(400, error).into_response(),
    }
}

async fn diffusion_job_response(path: &str, handle: &ReviewHandle) -> Response {
    let job_id = path.trim_start_matches("/api/diffusion/jobs/");
    let Some(job_id) = (!job_id.is_empty() && !job_id.contains('/'))
        .then(|| job_id.parse::<u64>().ok())
        .flatten()
    else {
        return text_response(400, "text/plain; charset=utf-8", "bad diffusion job id")
            .into_response();
    };
    match handle
        .diffusion_job_snapshot(job_id)
        .and_then(|job| serde_json::to_string(&job).context("serializing diffusion job"))
    {
        Ok(body) => text_response(200, "application/json; charset=utf-8", &body).into_response(),
        Err(error) => json_error(404, error).into_response(),
    }
}

async fn diffusion_preview_media_response(path: &str, handle: &ReviewHandle) -> Response {
    let parts = path
        .trim_start_matches("/diffusion-preview/")
        .split('/')
        .collect::<Vec<_>>();
    let [job_id, side] = parts.as_slice() else {
        return text_response(404, "text/plain; charset=utf-8", "not found").into_response();
    };
    let Some(job_id) = job_id.parse::<u64>().ok() else {
        return text_response(400, "text/plain; charset=utf-8", "bad diffusion job id")
            .into_response();
    };
    let after = match *side {
        "before" => false,
        "after" => true,
        _ => return text_response(404, "text/plain; charset=utf-8", "not found").into_response(),
    };
    match handle.diffusion_preview_media_path(job_id, after) {
        Ok(path) => serve_review_file(path, "image/jpeg").await,
        Err(error) => json_error(404, error).into_response(),
    }
}

async fn diffusion_settings_response(
    method: Method,
    body: &[u8],
    handle: &ReviewHandle,
) -> Response {
    if let Err(error) = handle.ensure_database_healthy() {
        return json_error(503, error).into_response();
    }
    let previous = match handle.api_state_value() {
        Ok(state) => state,
        Err(error) => return json_error(500, error).into_response(),
    };
    let result = match method {
        Method::POST => match serde_json::from_slice::<ReviewDiffusionSettingsRequest>(body)
            .context("parsing diffusion settings")
        {
            Ok(request) => handle.apply_diffusion_settings_async(request).await,
            Err(error) => Err(error),
        },
        Method::DELETE => {
            match serde_json::from_slice::<ReviewDiffusionSettingsResetRequest>(body)
                .context("parsing diffusion settings reset")
            {
                Ok(request) => handle.reset_diffusion_settings_async(request).await,
                Err(error) => Err(error),
            }
        }
        _ => unreachable!(),
    };
    match result.and_then(|()| handle.api_state_patch_json_since(&previous)) {
        Ok(body) => text_response(200, "application/json; charset=utf-8", &body).into_response(),
        Err(error) if handle.ensure_database_healthy().is_err() => {
            json_error(503, error).into_response()
        }
        Err(error) => json_error(400, error).into_response(),
    }
}

async fn sampler_job_response(
    method: Method,
    path: &str,
    body: &[u8],
    handle: &ReviewHandle,
) -> Response {
    let parts = path
        .trim_start_matches("/api/sampler/jobs/")
        .split('/')
        .collect::<Vec<_>>();
    let Some(job_id) = parts.first().and_then(|value| value.parse::<u64>().ok()) else {
        return text_response(400, "text/plain; charset=utf-8", "bad sampler job id")
            .into_response();
    };
    let result = match (method, parts.as_slice()) {
        (Method::GET, [_]) => handle.sampler_job_snapshot(job_id),
        (Method::POST, [_, "priority"]) => {
            match serde_json::from_slice::<ReviewSamplerPriorityRequest>(body)
                .context("parsing sampler priority update")
            {
                Ok(request) => handle.prioritize_sampler_job(job_id, request),
                Err(error) => Err(error),
            }
        }
        (Method::POST, [_, "profiles", entry_key]) => {
            match serde_json::from_slice::<ReviewSamplerSelectionRequest>(body)
                .context("parsing sampler profile selection")
            {
                Ok(request) => {
                    handle
                        .apply_sampler_selection_async(job_id, entry_key, request)
                        .await
                }
                Err(error) => Err(error),
            }
        }
        _ => return text_response(404, "text/plain; charset=utf-8", "not found").into_response(),
    };
    match result
        .and_then(|snapshot| serde_json::to_string(&snapshot).context("serializing sampler job"))
    {
        Ok(body) => text_response(200, "application/json; charset=utf-8", &body).into_response(),
        Err(error) => json_error(400, error).into_response(),
    }
}

async fn sampler_media_response(path: &str, handle: &ReviewHandle) -> Response {
    let parts = path
        .trim_start_matches("/sampler-media/")
        .split('/')
        .collect::<Vec<_>>();
    let [job_id, key] = parts.as_slice() else {
        return text_response(404, "text/plain; charset=utf-8", "not found").into_response();
    };
    let Some(job_id) = job_id.parse::<u64>().ok() else {
        return text_response(400, "text/plain; charset=utf-8", "bad sampler job id")
            .into_response();
    };
    match handle.sampler_media_path(job_id, key) {
        Ok(path) => serve_review_file(path, "image/jpeg").await,
        Err(error) => json_error(404, error).into_response(),
    }
}

async fn panorama_create_response(body: &[u8], handle: &ReviewHandle) -> Response {
    let previous = match handle.api_state_value() {
        Ok(state) => state,
        Err(error) => return json_error(500, error).into_response(),
    };
    let result = async {
        let request = serde_json::from_slice::<ReviewPanoramaCreateRequest>(body)
            .context("parsing panorama project")?;
        handle.create_panorama_project_async(request).await?;
        handle.api_state_patch_json_since(&previous)
    }
    .await;
    match result {
        Ok(body) => text_response(201, "application/json; charset=utf-8", &body).into_response(),
        Err(error) => json_error(400, error).into_response(),
    }
}

async fn panorama_project_response(
    method: Method,
    path: &str,
    body: &[u8],
    handle: &ReviewHandle,
) -> Response {
    let parts = path
        .trim_start_matches("/api/panoramas/")
        .split('/')
        .collect::<Vec<_>>();
    let Some(project_id) = parts.first().and_then(|value| value.parse::<u64>().ok()) else {
        return text_response(400, "text/plain; charset=utf-8", "bad panorama project id")
            .into_response();
    };
    let previous = match handle.api_state_value() {
        Ok(state) => state,
        Err(error) => return json_error(500, error).into_response(),
    };
    let result = match (method, parts.as_slice()) {
        (Method::PATCH, [_]) => match serde_json::from_slice::<ReviewPanoramaUpdateRequest>(body)
            .context("parsing panorama project update")
        {
            Ok(request) => {
                handle
                    .update_panorama_project_async(project_id, request)
                    .await
            }
            Err(error) => Err(error),
        },
        (Method::POST, [_, "previews"]) => {
            let request = if body.is_empty() {
                Ok(ReviewPanoramaPreviewRequest::default())
            } else {
                serde_json::from_slice(body).context("parsing panorama preview request")
            };
            match request {
                Ok(request) => {
                    handle
                        .start_panorama_previews_async(project_id, request)
                        .await
                }
                Err(error) => Err(error),
            }
        }
        (Method::POST, [_, "render"]) => {
            let request = if body.is_empty() {
                Ok(ReviewPanoramaRenderRequest::default())
            } else {
                serde_json::from_slice(body).context("parsing panorama render request")
            };
            match request {
                Ok(request) => {
                    handle
                        .start_panorama_render_async(project_id, request)
                        .await
                }
                Err(error) => Err(error),
            }
        }
        _ => return text_response(404, "text/plain; charset=utf-8", "not found").into_response(),
    };
    match result.and_then(|()| handle.api_state_patch_json_since(&previous)) {
        Ok(body) => text_response(202, "application/json; charset=utf-8", &body).into_response(),
        Err(error) => json_error(400, error).into_response(),
    }
}

async fn panorama_preview_response(path: &str, handle: &ReviewHandle) -> Response {
    let parts = path
        .trim_start_matches("/panorama-preview/")
        .split('/')
        .collect::<Vec<_>>();
    let [project_id, matching, projection] = parts.as_slice() else {
        return text_response(404, "text/plain; charset=utf-8", "not found").into_response();
    };
    let result = (|| {
        let project_id = project_id
            .parse::<u64>()
            .context("parsing panorama project id")?;
        let matching = parse_panorama_matching(matching)?;
        let projection = parse_panorama_projection(projection)?;
        handle.panorama_preview_media_path(project_id, matching, projection)
    })();
    match result {
        Ok(path) => serve_review_file(path, "image/jpeg").await,
        Err(error) => json_error(404, error).into_response(),
    }
}

fn parse_panorama_matching(value: &str) -> Result<PanoramaMatchingMode> {
    match value {
        "automatic" => Ok(PanoramaMatchingMode::Automatic),
        "sequential" => Ok(PanoramaMatchingMode::Sequential),
        "multi-row" => Ok(PanoramaMatchingMode::MultiRow),
        "flat-mosaic" => Ok(PanoramaMatchingMode::FlatMosaic),
        _ => bail!("invalid panorama matching mode {value:?}"),
    }
}

fn parse_panorama_projection(value: &str) -> Result<PanoramaProjection> {
    match value {
        "rectilinear" => Ok(PanoramaProjection::Rectilinear),
        "cylindrical" => Ok(PanoramaProjection::Cylindrical),
        "equirectangular" => Ok(PanoramaProjection::Equirectangular),
        "panini" => Ok(PanoramaProjection::Panini),
        _ => bail!("invalid panorama projection {value:?}"),
    }
}

pub(super) async fn media_response(path: &str, handle: &ReviewHandle) -> Response {
    let parts = path
        .trim_start_matches("/media/")
        .split('/')
        .collect::<Vec<_>>();
    if parts.len() != 1 && parts.len() != 2 && parts.len() != 3 {
        return text_response(404, "text/plain; charset=utf-8", "not found").into_response();
    }
    let image_id = match parts[0].parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id").into_response();
        }
    };
    if parts.len() == 1 {
        return match handle.full_media_path(image_id) {
            Ok(path) => {
                let content_type = review_media_content_type(&path);
                serve_review_file(path, content_type).await
            }
            Err(error) => json_error(404, error).into_response(),
        };
    }
    let profile_index = match parts[1].parse::<usize>() {
        Ok(index) => index,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad profile index")
                .into_response();
        }
    };
    let result = if parts.len() == 3 {
        if parts[2] != "base" {
            return text_response(404, "text/plain; charset=utf-8", "not found").into_response();
        }
        handle.profile_base_media_path(image_id, profile_index)
    } else {
        handle.media_path(image_id, profile_index)
    };
    match result {
        Ok(path) => {
            let content_type = review_media_content_type(&path);
            serve_review_file(path, content_type).await
        }
        Err(error) => json_error(404, error).into_response(),
    }
}

pub(super) async fn crop_source_response(path: &str, handle: &ReviewHandle) -> Response {
    let id = path.trim_start_matches("/crop-source/");
    let image_id = match id.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id").into_response();
        }
    };
    match handle.crop_source_media_path(image_id) {
        Ok(path) => serve_review_file(path, "image/jpeg").await,
        Err(error) => json_error(404, error).into_response(),
    }
}

pub(super) async fn original_response(path: &str, handle: &ReviewHandle) -> Response {
    let id = path.trim_start_matches("/original/");
    let image_id = match id.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id").into_response();
        }
    };
    match handle.original_media_path(image_id) {
        Ok(path) => {
            let content_type = review_media_content_type(&path);
            serve_review_file(path, content_type).await
        }
        Err(error) => json_error(404, error).into_response(),
    }
}

fn review_media_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension)
            if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") =>
        {
            "image/jpeg"
        }
        Some(extension) if extension.eq_ignore_ascii_case("heic") => "image/heic",
        Some(extension) if extension.eq_ignore_ascii_case("heif") => "image/heif",
        Some(extension)
            if extension.eq_ignore_ascii_case("tif") || extension.eq_ignore_ascii_case("tiff") =>
        {
            "image/tiff"
        }
        _ => "application/octet-stream",
    }
}

async fn full_preview_response(path: &str, handle: &ReviewHandle) -> Response {
    let id = path.trim_start_matches("/full-preview/");
    let image_id = match id.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id").into_response();
        }
    };
    let handle = handle.clone();
    match tokio::task::spawn_blocking(move || handle.rendered_full_preview_media_path(image_id))
        .await
    {
        Ok(Ok(path)) => serve_review_file(path, "image/jpeg").await,
        Ok(Err(error)) => json_error(404, error).into_response(),
        Err(error) => {
            json_error(500, anyhow!("creating full TIFF preview: {error}")).into_response()
        }
    }
}

async fn profile_asset_response(path: &str, handle: &ReviewHandle) -> Response {
    let suffix = path.trim_start_matches("/api/profile/");
    let parts = suffix.split('/').collect::<Vec<_>>();
    let Some(profile_index) = parts
        .first()
        .and_then(|profile_index| profile_index.parse::<usize>().ok())
    else {
        return text_response(400, "text/plain; charset=utf-8", "bad profile index")
            .into_response();
    };

    match parts.as_slice() {
        [_, "hald"] => match handle.profile_hald_path(profile_index) {
            Ok(path) => serve_review_file(path, "image/png").await,
            Err(error) => json_error(404, error).into_response(),
        },
        [_, "pp3", image_id] => profile_pp3_response(profile_index, image_id, handle),
        _ => text_response(404, "text/plain; charset=utf-8", "not found").into_response(),
    }
}

fn profile_pp3_response(profile_index: usize, image_id: &str, handle: &ReviewHandle) -> Response {
    let image_id = match image_id.parse::<u64>() {
        Ok(index) => index,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id").into_response();
        }
    };
    match handle.profile_pp3_text(image_id, profile_index) {
        Ok(text) => {
            let mut response =
                text_response(200, "text/plain; charset=utf-8", &text).into_response();
            response.headers_mut().insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment"),
            );
            response
        }
        Err(error) => json_error(404, error).into_response(),
    }
}

pub(super) async fn outputs_response(path: &str, handle: &ReviewHandle) -> Response {
    let candidate = path.trim_start_matches("/outputs/");
    let Ok(candidate) = decode_output_path(candidate) else {
        return text_response(400, "text/plain; charset=utf-8", "bad path").into_response();
    };
    let Ok(relative) = sanitize_output_path(&candidate) else {
        return text_response(404, "text/plain; charset=utf-8", "not found").into_response();
    };
    let request_path = handle.output_root().join(relative);
    if request_path.is_dir() {
        return serve_review_path(request_path.join("index.html")).await;
    }
    serve_review_path(request_path).await
}

pub(super) async fn preview_response(path: &str, handle: &ReviewHandle) -> Response {
    let id = path.trim_start_matches("/preview/");
    let image_id = match id.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id").into_response();
        }
    };
    match handle.preview_media_path(image_id) {
        Ok(path) => serve_review_file(path, "image/jpeg").await,
        Err(error) => json_error(404, error).into_response(),
    }
}

pub(super) async fn thumbnail_response(path: &str, handle: &ReviewHandle) -> Response {
    let id = path.trim_start_matches("/thumbnail/");
    let image_id = match id.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id").into_response();
        }
    };
    match handle.thumbnail_media_path(image_id) {
        Ok(path) => serve_review_file(path, "image/jpeg").await,
        Err(error) => json_error(404, error).into_response(),
    }
}

fn sanitize_output_path(candidate: &str) -> Result<PathBuf, ()> {
    if candidate.is_empty() {
        return Err(());
    }
    let mut safe = PathBuf::new();
    let mut has_segment = false;
    for component in Path::new(candidate).components() {
        match component {
            std::path::Component::Normal(component) => {
                if component.is_empty() {
                    return Err(());
                }
                safe.push(component);
                has_segment = true;
            }
            std::path::Component::ParentDir => return Err(()),
            std::path::Component::CurDir => {}
            _ => return Err(()),
        }
    }
    if !has_segment {
        return Err(());
    }
    Ok(safe)
}

fn decode_output_path(candidate: &str) -> Result<String, ()> {
    let bytes = candidate.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(());
            }
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| ())
}

fn hex_value(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(()),
    }
}

async fn serve_review_file(path: PathBuf, content_type: &'static str) -> Response {
    let mime = match content_type.parse() {
        Ok(mime) => mime,
        Err(error) => {
            return json_error(500, anyhow!("invalid review media content type: {error}"))
                .into_response();
        }
    };
    let request = match Request::builder()
        .method(Method::GET)
        .uri("/")
        .body(Body::empty())
    {
        Ok(request) => request,
        Err(error) => {
            return json_error(500, anyhow!("building review media request: {error}"))
                .into_response();
        }
    };
    match ServeFile::new_with_mime(path, &mime).oneshot(request).await {
        Ok(response) => response.map(Body::new).into_response(),
        Err(error) => {
            json_error(500, anyhow!("serving review media from disk: {error}")).into_response()
        }
    }
}

async fn serve_gallery_archive(path: PathBuf, download_name: &str) -> Response {
    let mut response = serve_review_file(path, "application/zip").await;
    if response.status().is_success() {
        let disposition = format!("attachment; filename=\"{download_name}\"");
        match HeaderValue::from_str(&disposition) {
            Ok(disposition) => {
                response
                    .headers_mut()
                    .insert(header::CONTENT_DISPOSITION, disposition);
            }
            Err(error) => {
                return json_error(500, anyhow!("building gallery download header: {error}"))
                    .into_response();
            }
        }
    }
    response
}

pub(super) fn parse_publish_request(body: &[u8]) -> Result<PublishRequest> {
    if body.is_empty() {
        return Ok(PublishRequest::default());
    }
    serde_json::from_slice(body).context("parsing publish request")
}

fn event_stream_response(handle: ReviewHandle) -> Response {
    let mut receiver = handle.subscribe();
    let mut keepalive = tokio::time::interval(Duration::from_secs(5));
    let initial_state = handle.api_state_json();
    let stream = stream! {
        match initial_state {
            Ok(state) => yield Ok::<_, Infallible>(Event::default().data(state)),
            Err(error) => yield Ok(Event::default().event("error").data(error.to_string())),
        }
        loop {
            tokio::select! {
                received = receiver.recv() => {
                    match received {
                        Ok(state) => yield Ok(Event::default().data(state)),
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if let Ok(state) = handle.api_state_json() {
                                yield Ok(Event::default().data(state));
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = keepalive.tick() => {
                    let data = json!({
                        "type": "keepalive",
                        "datetime": chrono::Utc::now().to_rfc3339(),
                        "version": env!("CARGO_PKG_VERSION"),
                    });
                    yield Ok(Event::default().event("keepalive").data(data.to_string()));
                }
            }
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

fn text_response(status: u16, content_type: &'static str, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        content_type,
        body: body.as_bytes().to_vec(),
    }
}

async fn serve_review_path(path: PathBuf) -> Response {
    let request = match Request::builder()
        .method(Method::GET)
        .uri("/")
        .body(Body::empty())
    {
        Ok(request) => request,
        Err(error) => {
            return json_error(500, anyhow!("building review file request: {error}"))
                .into_response();
        }
    };
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => response.map(Body::new).into_response(),
        Err(error) => {
            json_error(500, anyhow!("serving review file from disk: {error}")).into_response()
        }
    }
}

fn json_error(status: u16, error: anyhow::Error) -> HttpResponse {
    text_response(
        status,
        "application/json; charset=utf-8",
        &json!({"error": error.to_string()}).to_string(),
    )
}

impl IntoResponse for HttpResponse {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (
            status,
            [
                (header::CONTENT_TYPE, self.content_type),
                (header::CACHE_CONTROL, "no-store, max-age=0"),
            ],
            self.body,
        )
            .into_response()
    }
}
