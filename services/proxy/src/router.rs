use std::sync::Arc;

use axum::Router;
use axum::routing::any;

use crate::handler::{proxy_handler, ProxyState};

pub fn proxy_router(state: Arc<ProxyState>) -> Router {
    Router::new()
        .route("/v1/messages", any(proxy_handler))
        .route("/v1/{path:.*}", any(proxy_handler))
        .with_state(state)
}
