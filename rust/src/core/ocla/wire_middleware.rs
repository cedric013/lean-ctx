//! Tower middleware for the OCLA HTTP wire contract.

use std::{
    convert::Infallible,
    future::Future,
    num::NonZeroUsize,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode},
    response::Response,
};
use lru::LruCache;
use tower::{
    Layer, Service, ServiceBuilder,
    layer::util::{Identity, Stack},
};

/// Maximum request body accepted by the OCLA wire API.
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;

const CACHE_CAPACITY: NonZeroUsize = NonZeroUsize::new(1024).unwrap();
const CACHE_TTL: Duration = Duration::from_mins(5);

#[derive(Clone)]
struct CachedResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
    expires_at: Instant,
}

impl CachedResponse {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::from(self.body));
        *response.status_mut() = self.status;
        *response.headers_mut() = self.headers;
        response
    }
}

#[derive(Clone)]
pub struct IdempotencyLayer {
    cache: Arc<Mutex<LruCache<String, CachedResponse>>>,
}

impl Default for IdempotencyLayer {
    fn default() -> Self {
        Self::new()
    }
}
impl IdempotencyLayer {
    /// Creates an idempotency cache with a 1024-entry, five-minute policy.
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(CACHE_CAPACITY))),
        }
    }

    fn cached_response(&self, key: &str) -> Option<Response> {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cached = cache.get(key).cloned();
        match cached {
            Some(response) if response.expires_at > Instant::now() => {
                Some(response.into_response())
            }
            Some(_) => {
                cache.pop(key);
                None
            }
            None => None,
        }
    }

    fn store_response(&self, key: String, response: &Response, body: &[u8]) {
        let cached = CachedResponse {
            status: response.status(),
            headers: response.headers().clone(),
            body: body.to_vec(),
            expires_at: Instant::now() + CACHE_TTL,
        };
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .put(key, cached);
    }
}

impl<S> Layer<S> for IdempotencyLayer {
    type Service = IdempotencyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        IdempotencyService {
            inner,
            layer: self.clone(),
        }
    }
}

#[derive(Clone)]
pub struct IdempotencyService<S> {
    inner: S,
    layer: IdempotencyLayer,
}

impl<S> Service<Request<Body>> for IdempotencyService<S>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let key = request
            .method()
            .eq(&Method::POST)
            .then(|| request.headers().get("Idempotency-Key"))
            .flatten()
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_owned);

        if let Some(key) = key.as_deref()
            && let Some(response) = self.layer.cached_response(key)
        {
            return Box::pin(async move { Ok(response) });
        }

        let layer = self.layer.clone();
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let response = inner.call(request).await?;
            let Some(key) = key else {
                return Ok(response);
            };

            let (parts, body) = response.into_parts();
            let Ok(body) = to_bytes(body, usize::MAX).await else {
                return Ok(Response::from_parts(parts, Body::empty()));
            };
            let response = Response::from_parts(parts, Body::from(body.clone()));
            layer.store_response(key, &response, &body);
            Ok(response)
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RequestSizeLimit;

impl RequestSizeLimit {
    /// Creates a 256 KiB request-size limit layer.
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestSizeLimit {
    type Service = RequestSizeLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestSizeLimitService { inner }
    }
}

#[derive(Clone)]
pub struct RequestSizeLimitService<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for RequestSizeLimitService<S>
where
    S: Service<Request<Body>, Response = Response, Error = Infallible> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let too_large = request
            .headers()
            .get("Content-Length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|length| length > MAX_REQUEST_BYTES as u64);
        if too_large {
            return Box::pin(async { Ok(payload_too_large()) });
        }

        let (parts, body) = request.into_parts();
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let body = match to_bytes(body, MAX_REQUEST_BYTES + 1).await {
                Ok(body) if body.len() <= MAX_REQUEST_BYTES => body,
                _ => return Ok(payload_too_large()),
            };
            inner
                .call(Request::from_parts(parts, Body::from(body)))
                .await
        })
    }
}

fn payload_too_large() -> Response {
    Response::builder()
        .status(StatusCode::PAYLOAD_TOO_LARGE)
        .body(Body::empty())
        .expect("valid payload-too-large response")
}

/// Builds the OCLA middleware stack: idempotency outside request-size checks.
pub fn ocla_middleware()
-> ServiceBuilder<Stack<IdempotencyLayer, Stack<RequestSizeLimit, Identity>>> {
    ServiceBuilder::new()
        .layer(RequestSizeLimit::new())
        .layer(IdempotencyLayer::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use tower::{ServiceExt, service_fn};

    fn response(body: &'static str) -> Response {
        Response::new(Body::from(body))
    }

    #[tokio::test]
    async fn idempotency_cache_hit_and_miss() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_for_service = calls.clone();
        let service = service_fn(move |_request: Request<Body>| {
            let calls = calls_for_service.clone();
            async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok::<_, Infallible>(response("cached"))
            }
        });
        let mut service = ocla_middleware().service(service);
        let request = |key| {
            let request = Request::builder()
                .method(Method::POST)
                .header("Idempotency-Key", key)
                .body(Body::empty())
                .expect("request");
            request
        };

        for key in ["same-key", "same-key", "new-key"] {
            let response = service
                .ready()
                .await
                .expect("ready")
                .call(request(key))
                .await
                .expect("response");
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap(),
                "cached"
            );
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn request_size_limit_rejects_oversized_body() {
        let service = service_fn(|_request: Request<Body>| async {
            Ok::<_, Infallible>(response("processed"))
        });
        let mut service = RequestSizeLimit::new().layer(service);
        let request = Request::builder()
            .method(Method::POST)
            .body(Body::from(vec![0_u8; MAX_REQUEST_BYTES + 1]))
            .expect("request");

        let response = service
            .ready()
            .await
            .expect("ready")
            .call(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
