#![cfg(all(feature = "sqlite", feature = "runtime-tokio"))]

use tideorm_engine::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement,
    TransactionTrait,
};

/// A file database removed when the guard drops, so sessions and the pool
/// reach the same one.
struct TempDatabase(std::path::PathBuf);

impl Drop for TempDatabase {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let mut path = self.0.clone().into_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn open(tag: &str) -> (DatabaseConnection, TempDatabase) {
    let path = std::env::temp_dir().join(format!(
        "tideorm_engine_session_{tag}_{}.db",
        std::process::id()
    ));
    let guard = TempDatabase(path.clone());
    let url = format!(
        "sqlite://{}?mode=rwc",
        path.display().to_string().replace('\\', "/")
    );
    let mut options = ConnectOptions::new(url);
    options.max_connections(1);
    (Database::connect(options).await.unwrap(), guard)
}

async fn scalar(connection: &impl ConnectionTrait, sql: &str) -> i64 {
    let row = connection
        .query_one_raw(Statement::from_string(DbBackend::SQLite, sql))
        .await
        .unwrap()
        .unwrap();
    row.try_get_by_index(0).unwrap()
}

async fn run(connection: &impl ConnectionTrait, sql: &str) {
    connection
        .execute_unprepared(sql)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

/// A session takes none of the pool's connections and keeps what its
/// statements set for itself.
async fn keeps_its_settings_outside_the_pool() {
    let (pool, _file) = open("settings").await;
    let session = pool.pin_session().await.unwrap();
    assert_eq!(session.get_database_backend(), DbBackend::SQLite);

    run(&session, "PRAGMA foreign_keys = OFF").await;
    run(&session, "CREATE TEMP TABLE scratch (id INTEGER)").await;
    // The pool's only connection is still free while the session holds its own.
    assert_eq!(scalar(&pool, "PRAGMA foreign_keys").await, 1);
    assert_eq!(scalar(&session, "PRAGMA foreign_keys").await, 0);
    assert_eq!(scalar(&session, "SELECT count(*) FROM scratch").await, 0);
    assert!(
        pool.execute_unprepared("SELECT count(*) FROM scratch")
            .await
            .is_err(),
        "a temp table lives on its own session's connection"
    );

    // A session answers with the pool it came from, and opens another.
    assert_eq!(
        session.get_sqlite_connection_pool().unwrap().size(),
        pool.get_sqlite_connection_pool().unwrap().size()
    );
    let other = session.pin_session().await.unwrap();
    assert_eq!(scalar(&other, "PRAGMA foreign_keys").await, 1);
    other.close().await.unwrap();

    // Each session has an identity of its own, unless it shares its pool's;
    // transactions carry the one they were begun on.
    assert_ne!(session.identity(), pool.identity());
    let sharing = pool.pin_session_sharing_identity().await.unwrap();
    assert_eq!(sharing.identity(), pool.identity());
    assert_eq!(pool.clone().identity(), pool.identity());
    let transaction = sharing.begin().await.unwrap();
    assert_eq!(transaction.identity(), pool.identity());
    transaction.rollback().await.unwrap();
    sharing.close().await.unwrap();
    session.close().await.unwrap();
}

/// Transactions opened on a session run on its connection, and one dropped
/// open is rolled back before the session's next statement.
async fn runs_transactions_on_its_connection() {
    let (pool, _file) = open("transactions").await;
    let session = pool.pin_session().await.unwrap();
    run(&session, "CREATE TEMP TABLE scratch (id INTEGER)").await;

    let transaction = session.begin().await.unwrap();
    run(&transaction, "INSERT INTO scratch VALUES (1)").await;
    let savepoint = transaction.begin().await.unwrap();
    run(&savepoint, "INSERT INTO scratch VALUES (2)").await;
    savepoint.rollback().await.unwrap();
    transaction.commit().await.unwrap();
    assert_eq!(scalar(&session, "SELECT count(*) FROM scratch").await, 1);

    let dropped = session.begin().await.unwrap();
    run(&dropped, "INSERT INTO scratch VALUES (3)").await;
    drop(dropped);
    assert_eq!(scalar(&session, "SELECT count(*) FROM scratch").await, 1);

    // A savepoint's rollback releases it too.
    let outer = session.begin().await.unwrap();
    let inner = outer.begin().await.unwrap();
    run(&inner, "INSERT INTO scratch VALUES (4)").await;
    inner.rollback().await.unwrap();
    assert!(
        outer
            .execute_unprepared("RELEASE SAVEPOINT _tideorm_savepoint_1")
            .await
            .is_err()
    );
    outer.rollback().await.unwrap();
    assert_eq!(scalar(&session, "SELECT count(*) FROM scratch").await, 1);

    // Transactions begun side by side on the session take turns: each ends
    // its own level.
    let begun = &session;
    let write = move |sql: &'static str| async move {
        let transaction = begun.begin().await.unwrap();
        run(&transaction, sql).await;
        transaction.commit().await.unwrap();
    };
    futures_util::future::join(
        write("INSERT INTO scratch VALUES (5)"),
        write("INSERT INTO scratch VALUES (6)"),
    )
    .await;
    assert_eq!(scalar(&session, "SELECT count(*) FROM scratch").await, 3);
    session.close().await.unwrap();
}

/// A transaction ends its own level, never one nested in it: committing it
/// while one is open is refused and rolls both back, as dropping it does,
/// and the nested one, ended with it, refuses everything but its end.
async fn ends_only_its_own_level() {
    let (pool, _file) = open("levels").await;
    let session = pool.pin_session().await.unwrap();
    run(&session, "CREATE TEMP TABLE scratch (id INTEGER)").await;

    let outer = session.begin().await.unwrap();
    run(&outer, "INSERT INTO scratch VALUES (1)").await;
    let inner = outer.begin().await.unwrap();
    run(&inner, "INSERT INTO scratch VALUES (2)").await;
    let refused = outer.commit().await.unwrap_err();
    assert!(
        refused.to_string().contains("nested in it is open"),
        "{refused}"
    );
    for refused in [
        inner
            .execute_unprepared("INSERT INTO scratch VALUES (3)")
            .await
            .unwrap_err(),
        inner.begin().await.unwrap_err(),
    ] {
        assert!(refused.to_string().contains("has ended"), "{refused}");
    }
    inner.rollback().await.unwrap();
    assert_eq!(scalar(&session, "SELECT count(*) FROM scratch").await, 0);

    let outer = session.begin().await.unwrap();
    let inner = outer.begin().await.unwrap();
    run(&inner, "INSERT INTO scratch VALUES (4)").await;
    drop(outer);
    assert!(inner.commit().await.is_err());
    assert_eq!(scalar(&session, "SELECT count(*) FROM scratch").await, 0);

    // The session is outside any transaction again: its next one commits.
    let next = session.begin().await.unwrap();
    run(&next, "INSERT INTO scratch VALUES (5)").await;
    next.commit().await.unwrap();
    assert_eq!(scalar(&session, "SELECT count(*) FROM scratch").await, 1);
    session.close().await.unwrap();
}

/// A savepoint cancelled while it commits ends as SQLite's driver counts it:
/// committed, or rolled back by the rollback its drop queues, and the
/// transaction it was open in goes on. The commit is cancelled after each
/// poll in turn, until one completes.
async fn goes_on_after_a_cancelled_savepoint_commit() {
    use futures_util::FutureExt;
    let (pool, _file) = open("cancelled").await;
    let session = pool.pin_session().await.unwrap();
    run(&session, "CREATE TEMP TABLE scratch (id INTEGER)").await;
    let mut cancelled = false;
    let mut committed = false;
    for polls in 1..=100 {
        run(&session, "DELETE FROM scratch").await;
        let outer = session.begin().await.unwrap();
        run(&outer, "INSERT INTO scratch VALUES (1)").await;
        let inner = outer.begin().await.unwrap();
        run(&inner, "INSERT INTO scratch VALUES (2)").await;
        let mut commit = Box::pin(inner.commit());
        let mut done = false;
        for _ in 0..polls {
            if let Some(result) = (&mut commit).now_or_never() {
                result.unwrap();
                done = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        drop(commit);
        cancelled |= !done;
        committed |= done;
        run(&outer, "INSERT INTO scratch VALUES (3)").await;
        outer.commit().await.unwrap();
        let rows = scalar(&session, "SELECT count(*) FROM scratch").await;
        // Cancelled, the savepoint may have committed or rolled back.
        assert!(rows == 3 || (!done && rows == 2), "{rows} rows");
        if committed {
            break;
        }
    }
    assert!(cancelled && committed);
    session.close().await.unwrap();
}

/// A session's connection would open an empty database of its own where the
/// database is private to each connection, so such a pool refuses one.
async fn refuses_a_private_database() {
    let pool = Database::connect("sqlite::memory:?cache=private")
        .await
        .unwrap();
    let refused = pool.pin_session().await.unwrap_err();
    assert!(
        refused.to_string().contains("cannot pin a session"),
        "{refused}"
    );
    assert_eq!(scalar(&pool, "SELECT 1").await, 1);
}

/// A closed session refuses its statements; the pool goes on.
async fn refuses_statements_once_closed() {
    let (pool, _file) = open("closed").await;
    let session = pool.pin_session().await.unwrap();
    let clone = session.clone();
    session.close().await.unwrap();
    assert!(clone.execute_unprepared("SELECT 1").await.is_err());
    assert!(clone.begin().await.is_err());
    assert!(clone.ping().await.is_err());
    assert_eq!(scalar(&pool, "SELECT 1").await, 1);
    // Closing it again does nothing.
    clone.close().await.unwrap();
}

#[test]
fn session_contracts_tokio() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            keeps_its_settings_outside_the_pool().await;
            runs_transactions_on_its_connection().await;
            ends_only_its_own_level().await;
            goes_on_after_a_cancelled_savepoint_commit().await;
            refuses_a_private_database().await;
            refuses_statements_once_closed().await;
        });
}
