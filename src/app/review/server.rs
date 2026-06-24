use std::{
    convert::Infallible,
    path::{Path, PathBuf},
};

use async_stream::stream;
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::State,
    http::{Method, Request, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::{model::*, prelude::*};

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
    match (method, path) {
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
            let previous = match handle.api_state_value() {
                Ok(state) => state,
                Err(error) => return json_error(500, error).into_response(),
            };
            match serde_json::from_slice::<ReviewUpdateRequest>(&body)
                .context("parsing review update")
                .and_then(|update| handle.apply_review_update(update))
                .and_then(|()| handle.api_state_patch_json_since(&previous))
            {
                Ok(body) => {
                    text_response(200, "application/json; charset=utf-8", &body).into_response()
                }
                Err(error) => json_error(400, error).into_response(),
            }
        }
        (Method::POST, "/api/ui") => {
            let previous = match handle.api_state_value() {
                Ok(state) => state,
                Err(error) => return json_error(500, error).into_response(),
            };
            match serde_json::from_slice::<ReviewUiUpdateRequest>(&body)
                .context("parsing review UI update")
                .and_then(|update| handle.apply_ui_update(update))
                .and_then(|()| handle.api_state_patch_json_since(&previous))
            {
                Ok(body) => {
                    text_response(200, "application/json; charset=utf-8", &body).into_response()
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
        (Method::GET, _) if path.starts_with("/api/profile/") => {
            profile_hald_response(path, handle).await
        }
        (Method::GET, _) if path.starts_with("/media/") => media_response(path, handle).await,
        (Method::GET, _) if path.starts_with("/outputs/") => outputs_response(path, handle).await,
        (Method::GET, _) if path.starts_with("/preview/") => preview_response(path, handle).await,
        _ => text_response(404, "text/plain; charset=utf-8", "not found").into_response(),
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
    for marker in ["/api/", "/assets/", "/media/", "/outputs/", "/preview/"] {
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

pub(super) async fn media_response(path: &str, handle: &ReviewHandle) -> Response {
    let parts = path
        .trim_start_matches("/media/")
        .split('/')
        .collect::<Vec<_>>();
    if parts.len() != 1 && parts.len() != 2 {
        return text_response(404, "text/plain; charset=utf-8", "not found").into_response();
    }
    let image_id = match parts[0].parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id").into_response();
        }
    };
    if parts.len() == 1 {
        return match handle.preview_media_path(image_id) {
            Ok(path) => serve_review_file(path, "image/jpeg").await,
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
    match handle.media_path(image_id, profile_index) {
        Ok(path) => serve_review_file(path, "image/jpeg").await,
        Err(error) => json_error(404, error).into_response(),
    }
}

async fn profile_hald_response(path: &str, handle: &ReviewHandle) -> Response {
    let suffix = path.trim_start_matches("/api/profile/");
    let parts = suffix.split('/').collect::<Vec<_>>();
    if parts.len() != 2 || parts[1] != "hald" {
        return text_response(404, "text/plain; charset=utf-8", "not found").into_response();
    }
    let profile_index = match parts[0].parse::<usize>() {
        Ok(index) => index,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad profile index")
                .into_response();
        }
    };
    match handle.profile_hald_path(profile_index) {
        Ok(path) => serve_review_file(path, "image/png").await,
        Err(error) => json_error(404, error).into_response(),
    }
}

pub(super) async fn outputs_response(path: &str, handle: &ReviewHandle) -> Response {
    let candidate = path.trim_start_matches("/outputs/");
    let Ok(relative) = sanitize_output_path(candidate) else {
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
