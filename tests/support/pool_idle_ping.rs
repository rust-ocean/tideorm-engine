use std::sync::{Arc, Mutex};
use std::time::Duration;
use tideorm_engine::{ConnectOptions, Database};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Only startup and Sync are accepted: no query execution is simulated.
async fn ping_server(
    events: Arc<Mutex<Vec<&'static str>>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let startup_len = socket.read_u32().await.unwrap();
        let mut startup = vec![0; usize::try_from(startup_len - 4).unwrap()];
        socket.read_exact(&mut startup).await.unwrap();
        assert_eq!(&startup[..4], &[0, 3, 0, 0]);
        // AuthenticationOk followed by ReadyForQuery (idle).
        socket
            .write_all(b"R\0\0\0\x08\0\0\0\0Z\0\0\0\x05I")
            .await
            .unwrap();
        loop {
            let tag = socket.read_u8().await.unwrap();
            let len = socket.read_u32().await.unwrap();
            assert_eq!(len, 4);
            match tag {
                b'S' => {
                    events.lock().unwrap().push("ping");
                    socket.write_all(b"Z\0\0\0\x05I").await.unwrap();
                }
                b'X' => break,
                other => panic!("unexpected frontend message {other}"),
            }
        }
    });
    (
        format!("postgres://fixture@127.0.0.1:{port}/x?sslmode=disable"),
        task,
    )
}

/// Wait for the pool's connection to go back idle, and check that its
/// release sent nothing: reading what is pending shows a broken connection.
async fn released(num_idle: impl Fn() -> usize, events: &Mutex<Vec<&'static str>>) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while num_idle() != 1 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert!(events.lock().unwrap().is_empty(), "{events:?}");
}

#[test]
fn idle_ping_runs_only_at_the_threshold() {
    super::pool_timeouts::run(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (url, server) = ping_server(events.clone()).await;
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .min_connections(0)
            .idle_timeout(None)
            .max_lifetime(None)
            .test_before_acquire_if_idle_for(Duration::from_millis(300))
            .test_before_acquire(true);
        let db = Database::connect(options).await.unwrap();
        let pool = db.get_postgres_connection_pool().unwrap();
        assert!(pool.options().get_test_before_acquire());
        assert_eq!(
            pool.options().get_test_before_acquire_if_idle_for(),
            Some(Duration::from_millis(300))
        );
        released(|| pool.num_idle(), &events).await;
        let hot = pool.acquire().await.unwrap();
        assert!(events.lock().unwrap().is_empty());
        drop(hot);
        released(|| pool.num_idle(), &events).await;
        tokio::time::sleep(Duration::from_millis(350)).await;
        let cold = pool.acquire().await.unwrap();
        assert_eq!(*events.lock().unwrap(), ["ping"]);
        cold.close().await.unwrap();
        db.close().await.unwrap();
        server.await.unwrap();
    });
}

#[test]
fn zero_idle_threshold_pings_even_with_the_boolean_disabled() {
    super::pool_timeouts::run(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (url, server) = ping_server(events.clone()).await;
        let mut options = ConnectOptions::new(url);
        options
            .test_before_acquire_if_idle_for(Duration::ZERO)
            .test_before_acquire(false);
        let db = Database::connect(options).await.unwrap();
        let pool = db.get_postgres_connection_pool().unwrap();
        released(|| pool.num_idle(), &events).await;
        let connection = pool.acquire().await.unwrap();
        assert_eq!(*events.lock().unwrap(), ["ping"]);
        connection.close().await.unwrap();
        db.close().await.unwrap();
        server.await.unwrap();
    });
}

/// By default a checkout pings only a connection idle for a second: one
/// just released goes out without a round trip.
#[test]
fn default_checkout_pings_only_an_idle_connection() {
    super::pool_timeouts::run(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (url, server) = ping_server(events.clone()).await;
        let mut options = ConnectOptions::new(url);
        options.max_connections(1);
        let db = Database::connect(options).await.unwrap();
        let pool = db.get_postgres_connection_pool().unwrap();
        assert!(pool.options().get_test_before_acquire());
        assert_eq!(
            pool.options().get_test_before_acquire_if_idle_for(),
            Some(Duration::from_secs(1))
        );
        released(|| pool.num_idle(), &events).await;
        let hot = pool.acquire().await.unwrap();
        assert!(events.lock().unwrap().is_empty());
        hot.close().await.unwrap();
        db.close().await.unwrap();
        server.await.unwrap();
    });
}
