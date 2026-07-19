use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use axum::body::Body;

/// Phase 1 no-op auth middleware.
///
/// The dashboard is self-hosted (behind your own network), so we skip real
/// authentication for now. Every request passes through unchanged.
///
/// When auth is needed later, replace the body with session/cookie checks.
pub async fn require_auth(req: Request<Body>, next: Next) -> Result<Response, StatusCode> {
    Ok(next.run(req).await)
}
