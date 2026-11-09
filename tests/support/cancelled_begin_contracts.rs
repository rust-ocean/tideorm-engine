#![cfg(all(
    feature = "runtime-tokio",
    any(feature = "postgres", feature = "mysql")
))]

use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tideorm_engine::{
    ConnectOptions, ConnectionTrait, Database, DbBackend, Statement, TransactionTrait,
};

/// Holds the first successful BEGIN reply, after the server has entered the
/// transaction and before the driver can increment its depth.
struct ReplyGate {
    armed: AtomicBool,
    captured: AtomicBool,
    release: AtomicBool,
    disconnect: bool,
    stop: AtomicBool,
}

struct Proxy {
    port: u16,
    gate: Arc<ReplyGate>,
    listener: Option<std::thread::JoinHandle<()>>,
}

impl Proxy {
    fn start(host: String, port: u16, postgres: bool, disconnect: bool) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let local_port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let gate = Arc::new(ReplyGate {
            armed: AtomicBool::new(false),
            captured: AtomicBool::new(false),
            release: AtomicBool::new(false),
            disconnect,
            stop: AtomicBool::new(false),
        });
        let control = gate.clone();
        let thread = std::thread::spawn(move || {
            while !control.stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((client, _)) => {
                        // Windows hands the listener's non-blocking mode on.
                        client.set_nonblocking(false).unwrap();
                        let server = TcpStream::connect((host.as_str(), port)).unwrap();
                        client.set_nodelay(true).unwrap();
                        server.set_nodelay(true).unwrap();
                        let mut client_read = client.try_clone().unwrap();
                        let mut server_write = server.try_clone().unwrap();
                        std::thread::spawn(move || {
                            let _ = std::io::copy(&mut client_read, &mut server_write);
                            let _ = server_write.shutdown(Shutdown::Both);
                        });
                        let control = control.clone();
                        std::thread::spawn(move || {
                            forward_replies(client, server, postgres, control)
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("proxy accept failed: {error}"),
                }
            }
        });
        Self {
            port: local_port,
            gate,
            listener: Some(thread),
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.gate.release.store(true, Ordering::SeqCst);
        self.gate.stop.store(true, Ordering::SeqCst);
        self.listener.take().unwrap().join().unwrap();
    }
}

fn forward_replies(
    mut client: TcpStream,
    mut server: TcpStream,
    postgres: bool,
    gate: Arc<ReplyGate>,
) {
    // The proxy ends with an error once either side hangs up.
    let _ = (|| -> std::io::Result<()> {
        loop {
            let mut header = vec![0; if postgres { 5 } else { 4 }];
            server.read_exact(&mut header)?;
            let length = if postgres {
                usize::try_from(u32::from_be_bytes(header[1..5].try_into().unwrap())).unwrap() - 4
            } else {
                usize::try_from(u32::from_le_bytes([header[0], header[1], header[2], 0])).unwrap()
            };
            let mut body = vec![0; length];
            server.read_exact(&mut body)?;
            let entered = if postgres {
                header[0] == b'Z' && body == b"T"
            } else {
                // BEGIN's OK response has zero affected rows and insert id.
                body.len() >= 7 && body[..3] == [0, 0, 0] && body[3] & 1 != 0
            };
            if entered && gate.armed.swap(false, Ordering::SeqCst) {
                gate.captured.store(true, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_secs(20);
                while !gate.release.load(Ordering::SeqCst) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                if gate.disconnect {
                    return Ok(());
                }
            }
            client.write_all(&header)?;
            client.write_all(&body)?;
        }
    })();
    let _ = server.shutdown(Shutdown::Both);
    let _ = client.shutdown(Shutdown::Both);
}

async fn scalar(db: &tideorm_engine::DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(db.get_database_backend(), sql))
        .await
        .unwrap()
        .unwrap()
        .try_get("n")
        .unwrap()
}

async fn cancelled_begin(url: &str, disconnect: bool) {
    let url = url::Url::parse(url).unwrap();
    let postgres = matches!(url.scheme(), "postgres" | "postgresql");
    let proxy = Proxy::start(
        url.host_str().unwrap().to_owned(),
        url.port().unwrap_or(if postgres { 5432 } else { 3306 }),
        postgres,
        disconnect,
    );
    let mut proxied = url.clone();
    proxied.set_host(Some("127.0.0.1")).unwrap();
    proxied.set_port(Some(proxy.port)).unwrap();
    let mut options = ConnectOptions::new(proxied.to_string());
    #[cfg(feature = "postgres")]
    if postgres {
        options = ConnectOptions::new(
            proxied
                .as_str()
                .parse::<tideorm_engine::driver::postgres::PgConnectOptions>()
                .unwrap()
                .ssl_mode(tideorm_engine::driver::postgres::PgSslMode::Disable),
        );
    }
    #[cfg(feature = "mysql")]
    if !postgres {
        options = ConnectOptions::new(
            proxied
                .as_str()
                .parse::<tideorm_engine::driver::mysql::MySqlConnectOptions>()
                .unwrap()
                .ssl_mode(tideorm_engine::driver::mysql::MySqlSslMode::Disabled),
        );
    }
    options
        .max_connections(1)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10));
    let db = Database::connect(options).await.unwrap();
    let observer = Database::connect(url.to_string()).await.unwrap();
    let identity = if postgres {
        "SELECT CAST(pg_backend_pid() AS BIGINT) AS n"
    } else {
        "SELECT CAST(CONNECTION_ID() AS SIGNED) AS n"
    };
    let original_session = scalar(&db, identity).await;
    db.execute_unprepared("DROP TABLE IF EXISTS tx_cancelled_begin")
        .await
        .unwrap();
    db.execute_unprepared("CREATE TABLE tx_cancelled_begin(id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    proxy.gate.armed.store(true, Ordering::SeqCst);
    let captured = async {
        while !proxy.gate.captured.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    let pending_begin = match futures_util::future::select(
        Box::pin(db.begin()),
        Box::pin(tokio::time::timeout(Duration::from_secs(10), captured)),
    )
    .await
    {
        futures_util::future::Either::Left(_) => {
            panic!("BEGIN completed before its reply was released")
        }
        futures_util::future::Either::Right((result, pending)) => {
            result.unwrap();
            pending
        }
    };
    drop(pending_begin);
    proxy.gate.release.store(true, Ordering::SeqCst);
    let next_session = scalar(&db, identity).await;
    if disconnect {
        assert_ne!(next_session, original_session);
    } else {
        assert_eq!(
            next_session, original_session,
            "healthy session must be reconciled and reused"
        );
    }
    db.execute_unprepared("INSERT INTO tx_cancelled_begin VALUES (1)")
        .await
        .unwrap();
    assert_eq!(
        scalar(&observer, "SELECT COUNT(*) AS n FROM tx_cancelled_begin").await,
        1,
        "next borrower's write must commit and be visible from another session"
    );
    let tx = db.begin().await.unwrap();
    tx.execute_unprepared("INSERT INTO tx_cancelled_begin VALUES (2)")
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        scalar(&observer, "SELECT COUNT(*) AS n FROM tx_cancelled_begin").await,
        1
    );
    db.execute_unprepared("DROP TABLE tx_cancelled_begin")
        .await
        .unwrap();
    db.close().await.unwrap();
    observer.close().await.unwrap();
}

fn run_live(variable: &str, backend: DbBackend) {
    let url = std::env::var(variable).ok().or_else(|| {
        std::env::var("ENGINE_PROBE_DATABASE_URL")
            .ok()
            .filter(|url| match backend {
                DbBackend::Postgres => url.starts_with("postgres"),
                _ => url.starts_with("mysql"),
            })
    });
    let Some(url) = url else {
        eprintln!("SKIP cancelled BEGIN: set {variable}");
        return;
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for disconnect in [false, true] {
                tokio::time::timeout(Duration::from_secs(45), cancelled_begin(&url, disconnect))
                    .await
                    .unwrap();
            }
        });
}

mod live {
    use super::*;

    #[cfg(feature = "postgres")]
    #[test]
    fn postgres_cancelled_begin_reconciles_or_closes() {
        run_live("POSTGRESQL_DATABASE_URL", DbBackend::Postgres);
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn mysql_cancelled_begin_reconciles_or_closes() {
        run_live("MYSQL_DATABASE_URL", DbBackend::MySQL);
    }

    #[cfg(feature = "mysql")]
    #[test]
    fn mariadb_cancelled_begin_reconciles_or_closes() {
        run_live("MARIADB_DATABASE_URL", DbBackend::MariaDB);
    }
}
