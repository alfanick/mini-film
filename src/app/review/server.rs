use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use super::{model::*, prelude::*};

pub(super) fn run_review_listener(server: Server, handle: ReviewHandle) {
    for request in server.incoming_requests() {
        let handle = handle.clone();
        let _ = thread::Builder::new()
            .name("mini-film-review-client".to_string())
            .spawn(move || {
                if let Err(error) = handle_review_request(request, &handle) {
                    eprintln!("review server connection failed: {error:#}");
                }
            });
    }
}

pub(super) fn handle_review_request(mut request: Request, handle: &ReviewHandle) -> Result<()> {
    let path = normalized_request_path(&request);
    if request.method() == &Method::Get && path == "/api/events" {
        let response = event_stream_response(handle)?;
        return request
            .respond(response)
            .context("writing review event stream response");
    }

    let response = route_request(&mut request, handle)?;
    request
        .respond(response.into_tiny())
        .context("writing review HTTP response")
}

pub(super) struct HttpResponse {
    pub(super) status: u16,
    pub(super) content_type: &'static str,
    pub(super) body: Vec<u8>,
}

pub(super) fn route_request(request: &mut Request, handle: &ReviewHandle) -> Result<HttpResponse> {
    let path = normalized_request_path(request);
    match (request.method(), path.as_str()) {
        (&Method::Get, "/") | (&Method::Get, "/review") => Ok(text_response(
            200,
            "text/html; charset=utf-8",
            review_index_html(),
        )),
        (&Method::Get, "/assets/styles.css") => Ok(text_response(
            200,
            "text/css; charset=utf-8",
            review_styles(),
        )),
        (&Method::Get, "/assets/app.js") => Ok(text_response(
            200,
            "application/javascript; charset=utf-8",
            review_script(),
        )),
        (&Method::Get, "/api/state") => match handle.api_state_json() {
            Ok(body) => Ok(text_response(200, "application/json; charset=utf-8", &body)),
            Err(error) => Ok(json_error(500, error)),
        },
        (&Method::Post, "/api/review") => {
            match request_body(request)
                .and_then(|body| {
                    serde_json::from_slice::<ReviewUpdateRequest>(&body)
                        .context("parsing review update")
                })
                .and_then(|update| handle.apply_review_update(update))
                .and_then(|()| handle.api_state_json())
            {
                Ok(body) => Ok(text_response(200, "application/json; charset=utf-8", &body)),
                Err(error) => Ok(json_error(400, error)),
            }
        }
        (&Method::Post, "/api/ui") => match request_body(request)
            .and_then(|body| {
                serde_json::from_slice::<ReviewUiUpdateRequest>(&body)
                    .context("parsing review UI update")
            })
            .and_then(|update| handle.apply_ui_update(update))
            .and_then(|()| handle.api_state_json())
        {
            Ok(body) => Ok(text_response(200, "application/json; charset=utf-8", &body)),
            Err(error) => Ok(json_error(400, error)),
        },
        (&Method::Post, "/api/publish") => match request_body(request)
            .and_then(|body| parse_publish_request(&body))
            .and_then(|request| handle.start_publish_job(request))
            .and_then(|_| handle.api_state_json())
        {
            Ok(body) => Ok(text_response(200, "application/json; charset=utf-8", &body)),
            Err(error) => Ok(json_error(500, error)),
        },
        (&Method::Get, _) if path.starts_with("/media/") => Ok(media_response(&path, handle)),
        (&Method::Get, _) if path.starts_with("/preview/") => Ok(preview_response(&path, handle)),
        _ => Ok(text_response(404, "text/plain; charset=utf-8", "not found")),
    }
}

pub(super) fn review_route_path(path: &str) -> String {
    for marker in ["/api/", "/assets/", "/media/", "/preview/"] {
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
    if !trimmed
        .rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'))
    {
        return "/".to_string();
    }
    path.to_string()
}

pub(super) fn media_response(path: &str, handle: &ReviewHandle) -> HttpResponse {
    let parts = path
        .trim_start_matches("/media/")
        .split('/')
        .collect::<Vec<_>>();
    if parts.len() != 2 {
        return text_response(404, "text/plain; charset=utf-8", "not found");
    }
    let image_id = match parts[0].parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id");
        }
    };
    let profile_index = match parts[1].parse::<usize>() {
        Ok(index) => index,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad profile index");
        }
    };
    match handle
        .media_path(image_id, profile_index)
        .and_then(|path| fs::read(&path).with_context(|| format!("reading {}", path.display())))
    {
        Ok(body) => HttpResponse {
            status: 200,
            content_type: "image/jpeg",
            body,
        },
        Err(error) => json_error(404, error),
    }
}

pub(super) fn preview_response(path: &str, handle: &ReviewHandle) -> HttpResponse {
    let id = path.trim_start_matches("/preview/");
    let image_id = match id.parse::<u64>() {
        Ok(id) => id,
        Err(_) => {
            return text_response(400, "text/plain; charset=utf-8", "bad image id");
        }
    };
    match handle
        .preview_media_path(image_id)
        .and_then(|path| fs::read(&path).with_context(|| format!("reading {}", path.display())))
    {
        Ok(body) => HttpResponse {
            status: 200,
            content_type: "image/jpeg",
            body,
        },
        Err(error) => json_error(404, error),
    }
}

pub(super) fn parse_publish_request(body: &[u8]) -> Result<PublishRequest> {
    if body.is_empty() {
        return Ok(PublishRequest::default());
    }
    serde_json::from_slice(body).context("parsing publish request")
}

fn normalized_request_path(request: &Request) -> String {
    review_route_path(request.url().split('?').next().unwrap_or("/"))
}

fn request_body(request: &mut Request) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    request
        .as_reader()
        .read_to_end(&mut body)
        .context("reading HTTP request body")?;
    Ok(body)
}

fn event_stream_response(handle: &ReviewHandle) -> Result<Response<SseBody>> {
    let receiver = handle.subscribe()?;
    Ok(Response::new(
        StatusCode(200),
        vec![
            header("Content-Type", "text/event-stream"),
            header("Cache-Control", "no-cache"),
            header("Connection", "keep-alive"),
            header("X-Accel-Buffering", "no"),
        ],
        SseBody::new(receiver),
        None,
        None,
    ))
}

fn text_response(status: u16, content_type: &'static str, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        content_type,
        body: body.as_bytes().to_vec(),
    }
}

fn json_error(status: u16, error: anyhow::Error) -> HttpResponse {
    text_response(
        status,
        "application/json; charset=utf-8",
        &json!({"error": error.to_string()}).to_string(),
    )
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid static HTTP header")
}

impl HttpResponse {
    fn into_tiny(self) -> Response<std::io::Cursor<Vec<u8>>> {
        Response::from_data(self.body)
            .with_status_code(StatusCode(self.status))
            .with_header(header("Content-Type", self.content_type))
    }
}

struct SseBody {
    receiver: Receiver<String>,
    buffer: Vec<u8>,
    offset: usize,
}

impl SseBody {
    fn new(receiver: Receiver<String>) -> Self {
        Self {
            receiver,
            buffer: Vec::new(),
            offset: 0,
        }
    }
}

impl Read for SseBody {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        while self.offset >= self.buffer.len() {
            match self.receiver.recv() {
                Ok(state) => {
                    self.buffer = format!("data: {state}\n\n").into_bytes();
                    self.offset = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let remaining = &self.buffer[self.offset..];
        let count = remaining.len().min(output.len());
        output[..count].copy_from_slice(&remaining[..count]);
        self.offset += count;
        if self.offset >= self.buffer.len() {
            self.buffer.clear();
            self.offset = 0;
        }
        Ok(count)
    }
}
