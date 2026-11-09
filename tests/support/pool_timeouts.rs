use std::time::{Duration, Instant};
use tideorm_engine::{ConnectOptions, Database};

pub(super) fn run(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
async fn stalled_handshake(scheme: &str) {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tideorm_engine::{DbErr, RuntimeErr, driver};
    for minimum in [0, 2] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicBool::new(false));
        let captured = accepted.clone();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            captured.store(true, Ordering::SeqCst);
            std::future::pending::<()>().await;
        });
        let mut options = ConnectOptions::new(format!("{scheme}://fixture@127.0.0.1:{port}/x"));
        options
            .min_connections(minimum)
            .connect_timeout(Duration::from_millis(80))
            .acquire_timeout(Duration::from_secs(2));
        let start = Instant::now();
        let error = tokio::time::timeout(Duration::from_secs(3), Database::connect(options))
            .await
            .unwrap()
            .unwrap_err();
        assert!(
            matches!(error, DbErr::Conn(RuntimeErr::DriverError(ref source))
            if matches!(source.as_ref(), driver::Error::PoolTimedOut)),
            "{error:?}"
        );
        assert!(accepted.load(Ordering::SeqCst), "must reach the handshake");
        assert!(start.elapsed() >= Duration::from_millis(80));
        assert!(start.elapsed() < Duration::from_secs(1));
        server.abort();
        let _ = server.await;
    }
}

#[cfg(feature = "postgres")]
#[test]
fn postgres_connect_timeout_bounds_an_unanswered_handshake() {
    run(stalled_handshake("postgres"));
}

#[cfg(feature = "mysql")]
#[test]
fn mysql_connect_timeout_bounds_an_unanswered_handshake() {
    run(stalled_handshake("mysql"));
}

#[cfg(feature = "sqlite")]
#[test]
fn saturated_sqlite_checkout_uses_acquire_timeout() {
    run(async {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options
            .max_connections(1)
            .connect_timeout(Duration::from_secs(2))
            .acquire_timeout(Duration::from_millis(80));
        let db = Database::connect(options).await.unwrap();
        let pool = db.get_sqlite_connection_pool().unwrap();
        let held = pool.acquire().await.unwrap();
        let start = Instant::now();
        let error = pool.acquire().await.unwrap_err();
        assert!(matches!(error, tideorm_engine::driver::Error::PoolTimedOut));
        assert!(start.elapsed() >= Duration::from_millis(80));
        assert!(start.elapsed() < Duration::from_secs(1));
        held.close().await.unwrap();
        db.close().await.unwrap();
    });
}
