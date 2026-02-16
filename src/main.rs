mod snapshot;

use axum::{Json, Router, extract::State, routing::get};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, RwLock};
use tower_http::{compression::CompressionLayer, cors::CorsLayer};

static TIMEOUT_CACHE_SECONDS: u64 = 60 * 5;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SnapshotContainer {
    update_interval: u64,
    snapshot: snapshot::Snapshot,
}

struct AppState {
    last_update: Mutex<SystemTime>,
    data: RwLock<SnapshotContainer>,
}

async fn index() -> &'static str {
    "Hello, CRAN!"
}

async fn snap(State(state): State<Arc<AppState>>) -> Json<SnapshotContainer> {
    {
        let mut last_update = state.last_update.lock().await;
        let now = SystemTime::now();

        let elapsed = now.duration_since(*last_update).unwrap_or_default();

        if elapsed.as_secs() > TIMEOUT_CACHE_SECONDS {
            tracing::info!("Refreshing cache from CRAN FTP");
            *last_update = now;

            let result = tokio::task::spawn_blocking(snapshot::Snapshot::capture)
                .await
                .expect("Blocking task panicked");

            let mut data = state.data.write().await;
            match result {
                Ok(snap) => data.snapshot = snap,
                Err(err) => tracing::error!("Failed to capture snapshot: {err}"),
            }
        } else {
            tracing::debug!("Serving cached snapshot");
        }
    }

    Json(state.data.read().await.clone())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let state = Arc::new(AppState {
        last_update: Mutex::new(UNIX_EPOCH),
        data: RwLock::new(SnapshotContainer {
            update_interval: TIMEOUT_CACHE_SECONDS,
            snapshot: snapshot::Snapshot::new(),
        }),
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/snap", get(snap))
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("Failed to bind to port 8080");

    tracing::info!("Listening on 0.0.0.0:8080");

    axum::serve(listener, app).await.expect("Server error");
}
