//! A service whose `/barrier` answers only once two requests are waiting at
//! the same time, so a test can prove two feature files really overlap.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;

/// How long a request waits for its partner before the answer is 504. Well
/// under the plugin's own request timeout, so a serialised run fails on the
/// status, not on a hung connection.
const PATIENCE: Duration = Duration::from_secs(3);

async fn barrier(State(waiting): State<Arc<AtomicUsize>>) -> StatusCode {
    waiting.fetch_add(1, Ordering::SeqCst);
    let start = Instant::now();
    while start.elapsed() < PATIENCE {
        if waiting.load(Ordering::SeqCst) >= 2 {
            return StatusCode::OK;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Leave the count as it was found, so a late partner does not pass.
    waiting.fetch_sub(1, Ordering::SeqCst);
    StatusCode::GATEWAY_TIMEOUT
}

pub async fn spawn_barrier() -> String {
    let app = Router::new()
        .route("/barrier", get(barrier))
        .with_state(Arc::new(AtomicUsize::new(0)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    format!("http://{addr}/")
}
