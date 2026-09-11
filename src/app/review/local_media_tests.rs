//! Exercise the real capability-protected local-media router, exact rendition lookup, and filesystem confinement.

use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
    response::Response,
};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use tower::ServiceExt;

/// Typed native-consumer request mirrors the serialization side of the deserialize-only server contract.
#[derive(serde::Serialize)]
struct LookupRequest<'a> {
    path: &'a str,
    catalog_path: &'a str,
}

/// Typed native-consumer response rejects missing fields without reaching through an untyped JSON tree.
#[derive(serde::Deserialize)]
struct LookupResponse {
    path: PathBuf,
    catalog_path: PathBuf,
}

/// Keep the isolated database runtime alive outside request futures so teardown never drops it inside Tokio.
struct Fixture {
    directory: tempfile::TempDir,
    handle: ReviewHandle,
    router: Router,
    token: String,
    original: PathBuf,
}

impl Fixture {
    /// Create one registered compressed input, a real catalog, and the actual router's private capability.
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input");
        let output = directory.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();
        let original = input.join("photo.jpg");
        fs::write(&original, b"original fixture bytes").unwrap();
        let handle = test_handle(input, output, vec![profile(0, "Fixture")]);
        handle
            .update_store(|store| {
                store.images.push(priority_image(
                    1,
                    original.to_str().unwrap(),
                    1,
                    4,
                    0,
                    vec![profile_render(0, "Fixture")],
                ));
                store.ui.current_image_id = Some(1);
                Ok(())
            })
            .unwrap();
        let router = local_media_router(handle.clone());
        let token =
            fs::read_to_string(handle.cache_root.join(".mini-film-local-media-token")).unwrap();
        Self {
            directory,
            handle,
            router,
            token,
            original,
        }
    }

    /// Serialize an exact media route and the explicitly selected catalog, with all required native-only headers.
    fn request(&self, path: &str) -> Request<Body> {
        self.request_catalog(path, self.handle.state_path())
    }

    /// Catalog mismatch tests must reach the real resolver rather than replacing its identity check.
    fn request_catalog(&self, path: &str, catalog: &Path) -> Request<Body> {
        let body = serde_json::to_vec(&LookupRequest {
            path,
            catalog_path: catalog.to_str().unwrap(),
        })
        .unwrap();
        let _: crate::review_contract::ReviewMediaPathRequest =
            serde_json::from_slice(&body).unwrap();
        Request::builder()
            .method("POST")
            .uri("/api/media-path")
            .header("content-type", "application/json")
            .header("x-mini-film-local-access", &self.token)
            .body(Body::from(body))
            .unwrap()
    }

    /// Insert the same peer extension supplied by Axum's real socket listener before running all routing middleware.
    async fn send(&self, mut request: Request<Body>, peer: Option<SocketAddr>) -> Response {
        if let Some(peer) = peer {
            request.extensions_mut().insert(ConnectInfo(peer));
        }
        self.router.clone().oneshot(request).await.unwrap()
    }

    /// Assert JSON-only responses resolve the canonical file and never return image bytes or cacheable private paths.
    fn resolves(&self, runtime: &tokio::runtime::Runtime, route: &str, expected: &Path) {
        runtime.block_on(async {
            let response = self.send(self.request(route), Some(loopback())).await;
            assert_eq!(response.status(), StatusCode::OK, "route {route}");
            assert_eq!(
                response.headers()["content-type"],
                "application/json; charset=utf-8"
            );
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert!(
                !response
                    .headers()
                    .contains_key("access-control-allow-origin")
            );
            let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
            let decoded: LookupResponse = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                decoded.path,
                expected.canonicalize().unwrap(),
                "route {route}"
            );
            assert_eq!(
                decoded.catalog_path,
                self.handle.state_path().canonicalize().unwrap()
            );
        });
    }
}

/// Use an actual loopback peer, not a client-controlled forwarding header.
fn loopback() -> SocketAddr {
    "127.0.0.1:45678".parse().unwrap()
}

/// Denials are deliberately generic and must never leak private catalog or media paths.
async fn denied(
    fixture: &Fixture,
    request: Request<Body>,
    peer: Option<SocketAddr>,
    status: StatusCode,
) {
    let response = fixture.send(request, peer).await;
    assert_eq!(response.status(), status);
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains(fixture.directory.path().to_str().unwrap()));
}

/// Real socket identity, token possession, JSON content type, and absence of browser/proxy headers are all required.
#[test]
fn local_media_authorization_requires_private_unforwarded_loopback_json() {
    let fixture = Fixture::new();
    let runtime = test_async_runtime();
    fixture.resolves(&runtime, "original/1", &fixture.original);
    runtime.block_on(async {
        for peer in [
            None,
            Some("192.0.2.1:45678".parse().unwrap()),
            Some("[2001:db8::1]:45678".parse().unwrap()),
        ] {
            denied(
                &fixture,
                fixture.request("original/1"),
                peer,
                StatusCode::FORBIDDEN,
            )
            .await;
        }
        for header in ["x-mini-film-local-access", "content-type"] {
            let mut request = fixture.request("original/1");
            request.headers_mut().remove(header);
            denied(&fixture, request, Some(loopback()), StatusCode::FORBIDDEN).await;
        }
        for (header, value) in [
            ("x-mini-film-local-access", "wrong"),
            ("content-type", "text/plain"),
            ("origin", "null"),
            ("origin", "http://127.0.0.1"),
            ("forwarded", "for=127.0.0.1"),
            ("x-forwarded-for", "127.0.0.1"),
            ("x-forwarded-proto", "http"),
            ("sec-fetch-site", "same-origin"),
            ("sec-fetch-mode", "cors"),
        ] {
            let mut request = fixture.request("original/1");
            request.headers_mut().insert(header, value.parse().unwrap());
            denied(&fixture, request, Some(loopback()), StatusCode::FORBIDDEN).await;
        }
        let mut ipv6 = fixture.request("original/1");
        ipv6.headers_mut().insert(
            "content-type",
            "application/json; charset=utf-8".parse().unwrap(),
        );
        assert_eq!(
            fixture
                .send(ipv6, Some("[::1]:45678".parse().unwrap()))
                .await
                .status(),
            StatusCode::OK
        );
    });
}

/// Canonical catalog matching and explicit route grammar reject arbitrary filesystem and cross-catalog lookup.
#[test]
fn local_media_rejects_unknown_routes_traversal_and_wrong_catalog() {
    let fixture = Fixture::new();
    let runtime = test_async_runtime();
    let other = fixture.directory.path().join("other.sqlite");
    fs::write(&other, b"unrelated catalog").unwrap();
    runtime.block_on(async {
        denied(
            &fixture,
            fixture.request_catalog("original/1", &other),
            Some(loopback()),
            StatusCode::NOT_FOUND,
        )
        .await;
        for path in [
            "",
            "//original/1",
            "original//1",
            "original/../1",
            "original/./1",
            "original/1?x=1",
            "original/1#fragment",
            "original\\1",
            "original/1\0",
            "original/999",
            "original/not-a-number",
            "http://localhost/original/1",
            "/etc/passwd",
            "api/state",
            "api/profile/0/pp3/1",
            "sampler-media/999/source",
            "diffusion-preview/999/after",
            "panorama-preview/999/automatic/rectilinear",
        ] {
            denied(
                &fixture,
                fixture.request(path),
                Some(loopback()),
                StatusCode::NOT_FOUND,
            )
            .await;
        }
        let mut invalid = fixture.request("original/1");
        *invalid.body_mut() = Body::from("{");
        denied(&fixture, invalid, Some(loopback()), StatusCode::BAD_REQUEST).await;
        let mut oversized = fixture.request("original/1");
        *oversized.body_mut() = Body::from(vec![b' '; 16 * 1024 + 1]);
        denied(
            &fixture,
            oversized,
            Some(loopback()),
            StatusCode::BAD_REQUEST,
        )
        .await;
    });
}

/// Existing profile, base, thumbnail, preview, and HALD resolvers preserve exact rendition identity.
#[test]
fn local_media_resolves_registered_renditions_without_changing_review_data() {
    let fixture = Fixture::new();
    let output = fixture.handle.output_root.join("render.jpg");
    let base = retouch_base_output(
        &output,
        &fixture.handle.output_root,
        &fixture.handle.cache_root,
    );
    let preview = fixture
        .handle
        .compressed_display_preview_path_for(&fixture.original, 1);
    let thumbnail = fixture
        .handle
        .compressed_thumbnail_path_for(&fixture.original, 1);
    let crop = fixture
        .handle
        .crop_source_preview_path_for(&fixture.original, 1);
    let hald = fixture.handle.output_root.join("hald.png");
    for path in [&output, &base, &preview, &thumbnail, &crop, &hald] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, path.to_string_lossy().as_bytes()).unwrap();
    }
    fixture
        .handle
        .update_store(|store| {
            let image = &mut store.images[0];
            image.profiles[0].output_path = Some(output.clone());
            image.preview.status = ReviewRenderStatus::Done;
            image.preview.path = Some(output.clone());
            image.sooc_sidecar_path = Some(fixture.original.clone());
            store.profiles[0].hald_path = Some(hald.clone());
            Ok(())
        })
        .unwrap();
    let before = fixture.handle.store_snapshot();
    let runtime = test_async_runtime();
    for (route, path) in [
        ("media/1", &output),
        ("media/1/0", &output),
        ("media/1/0/base", &base),
        ("preview/1", &preview),
        ("thumbnail/1", &thumbnail),
        ("crop-source/1", &crop),
        ("api/profile/0/hald", &hald),
        ("/original/1", &fixture.original),
    ] {
        fixture.resolves(&runtime, route, path);
    }
    assert!(Arc::ptr_eq(&fixture.handle.store_snapshot(), &before));
}

/// Export routes permit existing image files but reject traversal, non-images, and symlinks escaping the output tree.
#[test]
fn local_media_output_paths_stay_inside_the_registered_output_root() {
    let fixture = Fixture::new();
    let exported = fixture.handle.output_root.join("album/photo one.jpg");
    fs::create_dir_all(exported.parent().unwrap()).unwrap();
    fs::write(&exported, b"exported").unwrap();
    fs::write(
        fixture.handle.output_root.join("secret.txt"),
        b"not an image",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        &fixture.original,
        fixture.handle.output_root.join("escape.jpg"),
    )
    .unwrap();
    let runtime = test_async_runtime();
    fixture.resolves(&runtime, "outputs/album/photo%20one.jpg", &exported);
    runtime.block_on(async {
        for path in [
            "outputs/../input/photo.jpg",
            "outputs/%2e%2e/input/photo.jpg",
            "outputs/secret.txt",
            "outputs/escape.jpg",
            "outputs/album",
            "outputs/%2fetc/passwd",
            "outputs/album/missing.jpg",
        ] {
            denied(
                &fixture,
                fixture.request(path),
                Some(loopback()),
                StatusCode::NOT_FOUND,
            )
            .await;
        }
    });
}

/// A TIFF request may materialize its normal cached JPEG proxy once, without changing source bytes or catalog records.
#[test]
fn local_media_lazy_full_preview_uses_existing_cache_helper_once() {
    let mut fixture = Fixture::new();
    let original = fixture.handle.input_root.join("source.tiff");
    fs::write(&original, b"unchanged TIFF fixture").unwrap();
    let converter = fixture.handle.cache_root.join("convert-fixture");
    let script = concat!(
        "#!/bin/sh\n",
        "if [ \"$1\" = \"-list\" ]; then printf 'Threads: 2\\n'; exit 0; fi\n",
        "for output; do :; done\n",
        "root=$(dirname -- \"$0\")\n",
        "case \"$output\" in \"$root\"/*) ;; *) exit 2 ;; esac\n",
        "printf 'proxy fixture' > \"$output\"\n",
        "printf 'called\\n' >> \"$0.calls\"\n",
    );
    fs::write(&converter, script).unwrap();
    fs::set_permissions(&converter, fs::Permissions::from_mode(0o700)).unwrap();
    fixture.handle.convert = converter.clone();
    fixture
        .handle
        .update_store(|store| {
            store.images[0].raw_path = original.clone();
            Ok(())
        })
        .unwrap();
    fixture.router = local_media_router(fixture.handle.clone());
    let expected = fixture.handle.rendered_full_preview_path_for(&original, 1);
    assert!(!expected.exists());
    let before = fixture.handle.store_snapshot();
    let runtime = test_async_runtime();
    fixture.resolves(&runtime, "full-preview/1", &expected);
    fixture.resolves(&runtime, "full-preview/1", &expected);
    assert_eq!(
        fs::read(converter.with_file_name("convert-fixture.calls")).unwrap(),
        b"called\n"
    );
    assert_eq!(fs::read(original).unwrap(), b"unchanged TIFF fixture");
    assert!(Arc::ptr_eq(&fixture.handle.store_snapshot(), &before));
}

/// Positive sampler and diffusion lookups use actual registered job paths, never guessed cache names.
#[test]
fn local_media_job_paths_use_the_exact_registered_source_and_result() {
    let fixture = Fixture::new();
    let (source, sample) = fixture.handle.seed_local_media_test_sampler(7).unwrap();
    let root = fixture
        .handle
        .cache_root
        .join(crate::app::cache::DIFFUSION_PREVIEWS_CACHE_DIR);
    fs::create_dir_all(&root).unwrap();
    let before = root.join("before.jpg");
    let after = root.join("after.jpg");
    fs::write(&before, b"before").unwrap();
    fs::write(&after, b"after").unwrap();
    fixture
        .handle
        .diffusion_jobs
        .lock()
        .unwrap()
        .push(ReviewDiffusionJob {
            id: 9,
            status: ReviewDiffusionJobStatus::Done,
            image_id: 1,
            profile_index: 0,
            settings: DiffusionSettings::default(),
            before_url: Some("diffusion-preview/9/before".into()),
            after_url: Some("diffusion-preview/9/after".into()),
            preview_width: Some(1),
            preview_height: Some(1),
            focus_source: None,
            detail_areas: Vec::new(),
            error: None,
            before_path: Some(before.clone()),
            after_path: Some(after.clone()),
        });
    let runtime = test_async_runtime();
    for (route, path) in [
        ("sampler-media/7/source", &source),
        ("sampler-media/7/fixture-profile", &sample),
        ("diffusion-preview/9/before", &before),
        ("diffusion-preview/9/after", &after),
    ] {
        fixture.resolves(&runtime, route, path);
    }
}

/// Insecure preexisting capabilities fail closed instead of silently weakening the native-only filesystem proof.
#[test]
fn local_media_capability_rejects_world_readable_files_and_symlinks() {
    let mut fixture = Fixture::new();
    let path = fixture
        .handle
        .cache_root
        .join(".mini-film-local-media-token");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    fixture.router = local_media_router(fixture.handle.clone());
    let runtime = test_async_runtime();
    runtime.block_on(denied(
        &fixture,
        fixture.request("original/1"),
        Some(loopback()),
        StatusCode::FORBIDDEN,
    ));
    let saved = fixture.directory.path().join("saved-token");
    fs::rename(&path, &saved).unwrap();
    fs::set_permissions(&saved, fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink(&saved, &path).unwrap();
    fixture.router = local_media_router(fixture.handle.clone());
    runtime.block_on(denied(
        &fixture,
        fixture.request("original/1"),
        Some(loopback()),
        StatusCode::FORBIDDEN,
    ));
}

/// Concurrent listeners publish one complete private capability and all authorize against that same winner.
#[test]
fn local_media_concurrent_startup_reuses_atomic_capability() {
    let mut fixture = Fixture::new();
    let path = fixture
        .handle
        .cache_root
        .join(".mini-film-local-media-token");
    fs::rename(&path, path.with_extension("previous")).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let routers = std::thread::scope(|scope| {
        (0..8)
            .map(|_| {
                let barrier = barrier.clone();
                let handle = fixture.handle.clone();
                scope.spawn(move || {
                    barrier.wait();
                    local_media_router(handle)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    fixture.token = fs::read_to_string(&path).unwrap();
    assert_eq!(fixture.token.len(), 64);
    let runtime = test_async_runtime();
    for router in routers {
        fixture.router = router;
        fixture.resolves(&runtime, "original/1", &fixture.original);
    }
}
