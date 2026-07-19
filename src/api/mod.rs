//! REST API route handlers for the Codasaurus web dashboard.
//!
//! Each sub-module exposes a `pub fn router() -> Router<AppState>` that its
//! handlers use, and they are merged by `api::router()` below.

pub mod audit;
pub mod auth;
pub mod errors;
pub mod github;
pub mod middleware;
pub mod repos;
pub mod reviews;
pub mod settings;
pub mod setup;
pub mod stats;

use axum::Router;

use crate::db::DbPool;

/// Shared application state available to every handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
}

/// Build the full API route tree with full path prefixes.
///
/// Returns `Router<AppState>` — merge this into the top-level server
/// router and call `.with_state(state)` at the outermost level:
/// ```ignore
/// let app = Router::new().merge(api::router()).with_state(state);
/// ```
pub fn router() -> Router<AppState> {
    let mut r = Router::new();
    // Each sub-router uses relative paths (e.g. "/login", "/repos/{id}").
    // We nest them under the full API prefix here so the server can merge
    // these routes at the root level.
    r = r.nest("/api/setup", setup::router());
    r = r.nest("/api/auth", auth::router());
    r = r.nest("/api/stats", stats::router());
    r = r.nest("/api/repos", repos::router());
    r = r.nest("/api/reviews", reviews::router());
    r = r.nest("/api/settings", settings::router());
    r = r.nest("/api/github", github::router());
    r = r.nest("/api/audit", audit::router());
    r
}
