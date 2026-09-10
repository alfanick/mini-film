//! Serve one embedded asset through negotiated representations and content-based revalidation.
//! Compression is cached once per process; application state and event streams never use this cache policy.

use std::io::Write;

use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use flate2::{Compression, write::GzEncoder};
use sha1::{Digest, Sha1};

/// Immutable bytes and validators for the two representations of the same logical JavaScript or CSS asset.
pub(super) struct StaticAsset {
    identity: Bytes,
    gzip: Bytes,
    identity_etag: HeaderValue,
    gzip_etag: HeaderValue,
    content_type: &'static str,
}

impl StaticAsset {
    /// Prepare gzip once without involving filesystem assets or changing Cargo's single-bundle contract.
    pub(super) fn new(source: &'static str, content_type: &'static str) -> Self {
        let identity = Bytes::from_static(source.as_bytes());
        let mut compressor = GzEncoder::new(Vec::new(), Compression::best());
        compressor
            .write_all(&identity)
            .expect("compressing an embedded asset into memory");
        let gzip = Bytes::from(
            compressor
                .finish()
                .expect("finishing in-memory compression"),
        );
        Self {
            identity_etag: etag(&identity),
            gzip_etag: etag(&gzip),
            identity,
            gzip,
            content_type,
        }
    }

    /// Revalidate the selected representation, including weak If-None-Match values used by intermediaries.
    pub(super) fn response(&self, request: &HeaderMap) -> Response {
        let (gzip_quality, identity_quality) = encoding_qualities(request);
        let gzip = gzip_quality > 0.0 && gzip_quality >= identity_quality;
        let (bytes, validator) = if gzip {
            (&self.gzip, &self.gzip_etag)
        } else {
            (&self.identity, &self.identity_etag)
        };
        let acceptable = gzip || identity_quality > 0.0;
        let unchanged = acceptable && matches_validator(request, validator);
        let mut response = Response::new(if acceptable && !unchanged {
            Body::from(bytes.clone())
        } else {
            Body::empty()
        });
        *response.status_mut() = if !acceptable {
            StatusCode::NOT_ACCEPTABLE
        } else if unchanged {
            StatusCode::NOT_MODIFIED
        } else {
            StatusCode::OK
        };
        let headers = response.headers_mut();
        headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(self.content_type),
        );
        if acceptable {
            headers.insert(header::ETAG, validator.clone());
            if gzip {
                headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
            }
        }
        response
    }
}

/// Hash representation bytes, not the application version, so unversioned development rebuilds cannot go stale.
fn etag(bytes: &[u8]) -> HeaderValue {
    let hex: String = Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    HeaderValue::from_str(&format!("\"{hex}\""))
        .expect("hexadecimal content digest is a valid HTTP header")
}

/// Prefer gzip on equal quality while respecting explicit exclusions and the wildcard's identity exception.
fn encoding_qualities(headers: &HeaderMap) -> (f32, f32) {
    let mut gzip = None;
    let mut identity = None;
    let mut wildcard = None;
    for value in headers.get_all(header::ACCEPT_ENCODING) {
        let Ok(value) = value.to_str() else {
            continue;
        };
        for entry in value.split(',') {
            let mut parts = entry.split(';');
            let name = parts.next().unwrap_or_default().trim();
            let mut quality = 1.0;
            for parameter in parts {
                if let Some((key, value)) = parameter.trim().split_once('=')
                    && key.trim().eq_ignore_ascii_case("q")
                {
                    quality = value.trim().parse::<f32>().unwrap_or(0.0);
                    if !(0.0..=1.0).contains(&quality) {
                        quality = 0.0;
                    }
                }
            }
            if name.eq_ignore_ascii_case("gzip") {
                gzip = Some(quality);
            } else if name.eq_ignore_ascii_case("identity") {
                identity = Some(quality);
            } else if name == "*" {
                wildcard = Some(quality);
            }
        }
    }
    (
        gzip.or(wildcard).unwrap_or(0.0),
        identity.unwrap_or(if wildcard == Some(0.0) { 0.0 } else { 1.0 }),
    )
}

/// GET revalidation uses weak comparison but must never confuse compressed and uncompressed representations.
fn matches_validator(headers: &HeaderMap, validator: &HeaderValue) -> bool {
    let expected = validator.to_str().expect("ASCII content validator");
    headers.get_all(header::IF_NONE_MATCH).iter().any(|value| {
        value.to_str().is_ok_and(|value| {
            value.split(',').any(|candidate| {
                let candidate = candidate.trim();
                candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == expected
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use flate2::read::GzDecoder;
    use std::io::Read;

    /// Encoding weights and explicit prohibitions must not be reduced to a substring search for gzip.
    #[test]
    fn encoding_negotiation_respects_quality_and_exclusions() {
        for (value, expected) in [
            ("gzip, br", (1.0, 1.0)),
            ("gzip;q=0", (0.0, 1.0)),
            ("*;q=0", (0.0, 0.0)),
            ("*;q=0, gzip;q=1", (1.0, 0.0)),
            ("gzip;q=0.3, identity;q=0.1", (0.3, 0.1)),
            ("GZIP;Q=NaN", (0.0, 1.0)),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static(value));
            assert_eq!(encoding_qualities(&headers), expected);
        }
        assert_eq!(encoding_qualities(&HeaderMap::new()), (0.0, 1.0));
    }

    /// Decoded bytes are identical and each encoding's own validator produces a bodyless 304 response.
    #[tokio::test]
    async fn static_asset_roundtrips_and_revalidates_each_representation() {
        let asset = StaticAsset::new("const message = 'embedded';", "application/javascript");
        let raw = asset.response(&HeaderMap::new());
        let raw_validator = raw.headers()[header::ETAG].clone();
        assert_eq!(raw.headers()[header::CACHE_CONTROL], "no-cache");
        let raw_bytes = to_bytes(raw.into_body(), 4096).await.unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        let compressed = asset.response(&headers);
        let compressed_validator = compressed.headers()[header::ETAG].clone();
        assert_ne!(raw_validator, compressed_validator);
        assert_eq!(compressed.headers()[header::CONTENT_ENCODING], "gzip");
        assert_eq!(compressed.headers()[header::VARY], "Accept-Encoding");
        let compressed_bytes = to_bytes(compressed.into_body(), 4096).await.unwrap();
        let mut decoded = Vec::new();
        GzDecoder::new(compressed_bytes.as_ref())
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, raw_bytes);
        headers.insert(header::IF_NONE_MATCH, raw_validator.clone());
        assert_eq!(asset.response(&headers).status(), StatusCode::OK);
        headers.insert(header::IF_NONE_MATCH, compressed_validator);
        let unchanged = asset.response(&headers);
        assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
        assert!(
            to_bytes(unchanged.into_body(), 4096)
                .await
                .unwrap()
                .is_empty()
        );
        headers.remove(header::ACCEPT_ENCODING);
        headers.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_str(&format!(
                "\"unrelated\", W/{}",
                raw_validator.to_str().unwrap()
            ))
            .unwrap(),
        );
        assert_eq!(asset.response(&headers).status(), StatusCode::NOT_MODIFIED);
        headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("*;q=0"));
        assert_eq!(
            asset.response(&headers).status(),
            StatusCode::NOT_ACCEPTABLE
        );
    }
}
