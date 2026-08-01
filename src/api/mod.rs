//! REST API route handlers for the Codasaurus web dashboard.
//!
//! Each sub-module exposes a `pub fn router() -> Router<AppState>` that its
//! handlers use, and they are merged by `api::router()` below.

pub mod audit;
pub mod auth;
pub mod errors;
pub mod github;
pub mod learning;
pub mod repos;
pub mod reviews;
pub mod settings;
pub mod setup;
pub mod stats;

use axum::middleware;
use axum::Router;

use crate::db::DbPool;

/// Shared application state available to every handler via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
}

/// Public routes (setup wizard + auth). No session required.
pub fn public_router() -> Router<AppState> {
    Router::new()
        .nest("/api/setup", setup::router())
        .nest("/api/auth", auth::router())
}

/// Authenticated dashboard routes.
pub fn protected_router() -> Router<AppState> {
    Router::new()
        .nest("/api/stats", stats::router())
        .nest("/api/repos", repos::router())
        .nest("/api/reviews", reviews::router())
        .nest("/api/settings", settings::router())
        .nest("/api/learning", learning::router())
        .nest("/api/github", github::router())
        .nest("/api/audit", audit::router())
}

/// Build the full API route tree. Prefer [`build_router`] when you have state
/// so auth middleware can be applied correctly.
pub fn router() -> Router<AppState> {
    public_router().merge(protected_router())
}

/// Build API router with auth middleware bound to `state`.
pub fn build_router(state: AppState) -> Router {
    let protected = protected_router()
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .with_state(state.clone());

    public_router().with_state(state).merge(protected)
}
