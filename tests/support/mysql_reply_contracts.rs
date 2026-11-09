#![cfg(all(feature = "runtime-tokio", feature = "mysql"))]

//! A MySQL connection reads each reply as the statement it answers, after a
//! statement given the wrong number of values and after one cancelled while
//! its prepare is answered.

use futures_util::future::{Either, select};
use std::{
    future::Future,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tideorm_engine::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, Statement, TransactionTrait,
    sql_builder::Value,
};

/// What the proxy lets through from the server: everything, or while `hold`
/// is set, as many packets as `allowed`.
#[derive(Default)]
struct Gate {
    hold: AtomicBool,
    allowed: AtomicUsize,
    /// Packets read from the server and not forwarded yet.
    waiting: AtomicUsize,
    stop: AtomicBool,
}

impl Gate {
    /// Takes one of the packets allowed through, if any is left.
    fn let_one_through(&self) -> bool {
        let allowed = self.allowed.load(Ordering::SeqCst);
        allowed > 0
            && self
                .allowed
                .compare_exchange(allowed, allowed - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
    }
}

struct Proxy {
    port: u16,
    gate: Arc<Gate>,
    listener: Option<std::thread::JoinHandle<()>>,
}

impl Proxy {
    fn start(host: String, port: u16) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let local_port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let gate = Arc::new(Gate::default());
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
                        std::thread::spawn(move || forward_replies(client, server, &control));
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
        self.gate.hold.store(false, Ordering::SeqCst);
        self.gate.stop.store(true, Ordering::SeqCst);
        self.listener.take().unwrap().join().unwrap();
    }
}

fn forward_replies(mut client: TcpStream, mut server: TcpStream, gate: &Gate) {
    // The proxy ends with an error once either side hangs up.
    let _ = (|| -> std::io::Result<()> {
        loop {
            let mut header = [0; 4];
            server.read_exact(&mut header)?;
            let length = u32::from_le_bytes([header[0], header[1], header[2], 0]);
            let mut body = vec![0; usize::try_from(length).unwrap()];
            server.read_exact(&mut body)?;
            gate.waiting.fetch_add(1, Ordering::SeqCst);
            while gate.hold.load(Ordering::SeqCst) && !gate.let_one_through() {
                std::thread::sleep(Duration::from_millis(1));
            }
            gate.waiting.fetch_sub(1, Ordering::SeqCst);
            client.write_all(&header)?;
            client.write_all(&body)?;
        }
    })();
    let _ = server.shutdown(Shutdown::Both);
    let _ = client.shutdown(Shutdown::Both);
}

const IDENTITY: &str = "SELECT CAST(CONNECTION_ID() AS SIGNED) AS n";

async fn scalar(db: &impl ConnectionTrait, sql: &str, values: Vec<Value>) -> i64 {
    let statement = Statement::from_sql_and_values(db.get_database_backend(), sql, values);
    db.query_one_raw(statement)
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index(0)
        .unwrap()
}

/// How many statements the server holds prepared, on every connection.
async fn prepared_statements(db: &DatabaseConnection) -> i64 {
    let sql = "SHOW GLOBAL STATUS LIKE 'Prepared_stmt_count'";
    let row = db
        .query_one_raw(Statement::from_string(db.get_database_backend(), sql))
        .await
        .unwrap()
        .unwrap();
    row.try_get_by_index::<String>(1).unwrap().parse().unwrap()
}

/// A statement given the wrong number of values fails before it runs, and
/// the pool's only connection answers the next statement, in a transaction
/// too. An uncached statement is not left prepared on the server.
async fn wrong_value_count(url: &str) {
    for capacity in [None, Some(0)] {
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(10));
        if let Some(capacity) = capacity {
            options.statement_cache_capacity(capacity);
        }
        let db = Database::connect(options).await.unwrap();
        let session = scalar(&db, IDENTITY, vec![]).await;
        let prepared = prepared_statements(&db).await;
        let wrong = || {
            Statement::from_sql_and_values(
                db.get_database_backend(),
                "SELECT CAST(? AS SIGNED) AS n, ? AS m",
                [1i64.into()],
            )
        };
        for _ in 0..5 {
            assert!(db.query_one_raw(wrong()).await.is_err());
        }
        assert_eq!(
            scalar(&db, "SELECT CAST(? AS SIGNED) AS n", vec![7i64.into()]).await,
            7
        );
        let transaction = db.begin().await.unwrap();
        assert!(transaction.query_one_raw(wrong()).await.is_err());
        assert_eq!(scalar(&transaction, IDENTITY, vec![]).await, session);
        transaction.rollback().await.unwrap();
        assert_eq!(scalar(&db, IDENTITY, vec![]).await, session);
        if capacity == Some(0) {
            let now = prepared_statements(&db).await;
            assert!(
                now < prepared + 5,
                "{prepared} prepared before, {now} after"
            );
        }
        db.close().await.unwrap();
    }
}

/// Polls `future` until `done` holds, which it must not complete before.
async fn poll_until<F: Future + Unpin>(future: &mut F, done: impl Fn() -> bool) {
    let wait = async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    let wait = tokio::time::timeout(Duration::from_secs(10), wait);
    match select(future, Box::pin(wait)).await {
        Either::Left(_) => panic!("the statement completed while its replies were held"),
        Either::Right((waited, _)) => waited.unwrap(),
    }
}

/// Whether `future` completes within `time`.
async fn completes_within<F: Future + Unpin>(future: &mut F, time: Duration) -> bool {
    let sleep = Box::pin(tokio::time::sleep(time));
    matches!(select(future, sleep).await, Either::Left(_))
}

/// A statement cancelled after each packet of its prepare's response in
/// turn, and then while its execute is answered, leaves the pool's
/// connection closed or read back into step: the next statements get their
/// own replies, and an update runs exactly once. Each statement is one the
/// connection has not prepared, so its prepare is sent.
async fn cancelled_prepare(url: &str) {
    let parsed = url::Url::parse(url).unwrap();
    let proxy = Proxy::start(
        parsed.host_str().unwrap().to_owned(),
        parsed.port().unwrap_or(3306),
    );
    let mut proxied = parsed.clone();
    proxied.set_host(Some("127.0.0.1")).unwrap();
    proxied.set_port(Some(proxy.port)).unwrap();
    let mut options = ConnectOptions::new(
        proxied
            .as_str()
            .parse::<tideorm_engine::driver::mysql::MySqlConnectOptions>()
            .unwrap()
            .ssl_mode(tideorm_engine::driver::mysql::MySqlSslMode::Disabled),
    );
    // No checkout ping, whose reply the gate would hold.
    options
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .test_before_acquire(false);
    let db = Database::connect(options).await.unwrap();
    let observer = Database::connect(url).await.unwrap();
    for sql in [
        "DROP TABLE IF EXISTS cancelled_prepare",
        "CREATE TABLE cancelled_prepare(id INTEGER PRIMARY KEY, n INTEGER NOT NULL)",
        "INSERT INTO cancelled_prepare VALUES (1, 0)",
    ] {
        observer.execute_unprepared(sql).await.unwrap();
    }
    let backend = db.get_database_backend();
    let gate = &proxy.gate;
    let mut let_through = 0_i64;
    loop {
        while gate.waiting.load(Ordering::SeqCst) > 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        gate.allowed.store(0, Ordering::SeqCst);
        gate.hold.store(true, Ordering::SeqCst);
        let sql = format!("SELECT CAST(? AS SIGNED) + {let_through} AS a, CAST(? AS SIGNED) AS b");
        let mut statement = Box::pin(db.query_one_raw(Statement::from_sql_and_values(
            backend,
            sql,
            [1i64.into(), 2i64.into()],
        )));
        poll_until(&mut statement, || gate.waiting.load(Ordering::SeqCst) > 0).await;
        let mut completed = false;
        for _ in 0..let_through {
            gate.allowed.fetch_add(1, Ordering::SeqCst);
            if completes_within(&mut statement, Duration::from_millis(50)).await {
                completed = true;
                break;
            }
        }
        drop(statement);
        gate.hold.store(false, Ordering::SeqCst);

        let updated = db
            .execute_raw(Statement::from_sql_and_values(
                backend,
                "UPDATE cancelled_prepare SET n = n + ? WHERE id = ?",
                [1i64.into(), 1i64.into()],
            ))
            .await
            .unwrap();
        assert_eq!(updated.rows_affected(), 1);
        let own = 40 + let_through;
        assert_eq!(
            scalar(&db, "SELECT CAST(? AS SIGNED) AS n", vec![own.into()]).await,
            own
        );
        let sql = "SELECT CAST(n AS SIGNED) AS n FROM cancelled_prepare WHERE id = 1";
        assert_eq!(scalar(&observer, sql, vec![]).await, let_through + 1);
        if completed {
            // The prepare's response alone is five packets: its `PrepareOk`
            // and two definitions each of the parameters and the columns.
            assert!(let_through > 5, "completed after {let_through} packets");
            break;
        }
        let_through += 1;
    }
    observer
        .execute_unprepared("DROP TABLE cancelled_prepare")
        .await
        .unwrap();
    db.close().await.unwrap();
    observer.close().await.unwrap();
}

fn run_live(variable: &str) {
    let url = std::env::var(variable).ok().or_else(|| {
        std::env::var("ENGINE_PROBE_DATABASE_URL")
            .ok()
            .filter(|url| url.starts_with("mysql"))
    });
    let Some(url) = url else {
        eprintln!("SKIP MySQL reply contracts: set {variable}");
        return;
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(60), wrong_value_count(&url))
                .await
                .expect("a statement waited for a reply that never comes");
            tokio::time::timeout(Duration::from_secs(120), cancelled_prepare(&url))
                .await
                .expect("a statement waited for a reply that never comes");
        });
}

mod live {
    use super::*;

    #[test]
    fn mysql_replies_stay_in_step() {
        run_live("MYSQL_DATABASE_URL");
    }

    #[test]
    fn mariadb_replies_stay_in_step() {
        run_live("MARIADB_DATABASE_URL");
    }
}
