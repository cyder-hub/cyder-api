use axum::{
    body::Body,
    extract::{OriginalUri, Request},
    http::{
        HeaderMap, HeaderName, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    middleware::Next,
    response::Response,
};

pub const MANAGER_CONTENT_SECURITY_POLICY: &str = "default-src 'none'; base-uri 'none'; connect-src 'self'; font-src 'self' data:; form-action 'self'; frame-ancestors 'none'; frame-src 'none'; img-src 'self' data:; manifest-src 'self'; media-src 'none'; object-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; style-src-elem 'self'; style-src-attr 'unsafe-inline'; worker-src 'none'";

pub const MANAGER_PERMISSIONS_POLICY: &str =
    "camera=(), microphone=(), geolocation=(), payment=(), usb=()";

const NO_STORE: &str = "no-store";
const HTML_NO_CACHE: &str = "no-cache, no-store, must-revalidate";
const IMMUTABLE_ASSET: &str = "public, max-age=31536000, immutable";
const STATIC_NO_CACHE: &str = "no-cache";

const CONTENT_SECURITY_POLICY: HeaderName = HeaderName::from_static("content-security-policy");
const PERMISSIONS_POLICY: HeaderName = HeaderName::from_static("permissions-policy");
const REFERRER_POLICY: HeaderName = HeaderName::from_static("referrer-policy");
const X_CONTENT_TYPE_OPTIONS: HeaderName = HeaderName::from_static("x-content-type-options");
const X_FRAME_OPTIONS: HeaderName = HeaderName::from_static("x-frame-options");

pub async fn manager_web_security_middleware(request: Request<Body>, next: Next) -> Response {
    let path = request
        .extensions()
        .get::<OriginalUri>()
        .map(|uri| uri.0.path())
        .unwrap_or_else(|| request.uri().path())
        .to_string();
    let mut response = next.run(request).await;
    apply_manager_security_headers(response.headers_mut());

    let cache_control = manager_cache_control(
        &path,
        response.status(),
        response.headers().get(CONTENT_TYPE),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(cache_control));
    response
}

fn apply_manager_security_headers(headers: &mut HeaderMap) {
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(
        PERMISSIONS_POLICY,
        HeaderValue::from_static(MANAGER_PERMISSIONS_POLICY),
    );
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(MANAGER_CONTENT_SECURITY_POLICY),
    );
}

fn manager_cache_control(
    path: &str,
    status: StatusCode,
    content_type: Option<&HeaderValue>,
) -> &'static str {
    if manager_route_suffix(path, "/manager/api").is_some() {
        return NO_STORE;
    }
    if !status.is_success() {
        return NO_STORE;
    }
    if content_type
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/html"))
        })
    {
        return HTML_NO_CACHE;
    }
    if manager_route_suffix(path, "/manager/ui/assets")
        .is_some_and(|suffix| suffix.starts_with('/'))
    {
        return IMMUTABLE_ASSET;
    }
    STATIC_NO_CACHE
}

fn manager_route_suffix<'a>(path: &'a str, route: &str) -> Option<&'a str> {
    path.rmatch_indices(route).find_map(|(index, matched)| {
        let suffix = &path[index + matched.len()..];
        (suffix.is_empty() || suffix.starts_with('/')).then_some(suffix)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_web_security_cache_classifier_is_status_and_content_type_aware() {
        let html = HeaderValue::from_static("text/html; charset=utf-8");
        let javascript = HeaderValue::from_static("text/javascript");

        assert_eq!(
            manager_cache_control("/manager/api/test", StatusCode::OK, Some(&html)),
            NO_STORE
        );
        assert_eq!(
            manager_cache_control(
                "/ai/manager/api/auth/login",
                StatusCode::OK,
                Some(&HeaderValue::from_static("application/json"))
            ),
            NO_STORE
        );
        assert_eq!(
            manager_cache_control("/manager/ui/route", StatusCode::OK, Some(&html)),
            HTML_NO_CACHE
        );
        assert_eq!(
            manager_cache_control(
                "/manager/ui/assets/app-ABC123.js",
                StatusCode::OK,
                Some(&javascript)
            ),
            IMMUTABLE_ASSET
        );
        assert_eq!(
            manager_cache_control(
                "/ai/manager/ui/assets/app-ABC123.js",
                StatusCode::OK,
                Some(&javascript)
            ),
            IMMUTABLE_ASSET
        );
        assert_eq!(
            manager_cache_control(
                "/manager/ui/robots.txt",
                StatusCode::OK,
                Some(&HeaderValue::from_static("text/plain"))
            ),
            STATIC_NO_CACHE
        );
        assert_eq!(
            manager_cache_control(
                "/manager/ui/assets/missing.js",
                StatusCode::NOT_FOUND,
                Some(&javascript)
            ),
            NO_STORE
        );
    }
}
