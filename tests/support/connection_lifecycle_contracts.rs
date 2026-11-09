#![cfg(all(
    feature = "sqlite",
    any(feature = "runtime-tokio", feature = "runtime-async-std")
))]
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tideorm::internal::driver::{self, Row, sqlite::SqliteConnectOptions};
use tideorm::orm::{
    ConnectOptions, Database, DatabaseConnection, DatabaseConnectionType, DbBackend, DbErr,
    Statement,
};

/// Driver options reach settings a URL cannot name, and the pool's sizing
/// is applied over them.
async fn configured_pool() {
    let connect: SqliteConnectOptions = "sqlite::memory:".parse().unwrap();
    let mut options = ConnectOptions::new(
        connect
            .foreign_keys(false)
            .busy_timeout(Duration::from_millis(321)),
    );
    options.max_connections(1).min_connections(1);
    let connection = Database::connect(options.clone()).await.unwrap();
    let pool = connection.get_sqlite_connection_pool().unwrap();
    assert_eq!(pool.options().get_max_connections(), 1);
    let foreign_keys: i64 = driver::query("PRAGMA foreign_keys")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    let busy_timeout: i64 = driver::query("PRAGMA busy_timeout")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(foreign_keys, 0);
    assert_eq!(busy_timeout, 321);
    assert_eq!(connection.get_database_backend(), DbBackend::SQLite);
    assert_eq!(
        format!("{connection:?}"),
        "DatabaseConnection { inner: SqlitePoolConnection }"
    );
    let clone = connection.clone();
    assert!(std::ptr::eq(
        pool.options(),
        clone.get_sqlite_connection_pool().unwrap().options()
    ));
    let adopted: DatabaseConnection = pool.clone().into();
    let connector_adopted = tideorm::orm::SqliteConnector::from_sqlite_pool(pool.clone());
    assert!(std::ptr::eq(
        pool.options(),
        adopted.get_sqlite_connection_pool().unwrap().options()
    ));
    connector_adopted.close().await.unwrap();
    assert!(pool.is_closed());
    assert!(clone.get_sqlite_connection_pool().unwrap().is_closed());
    assert!(adopted.get_sqlite_connection_pool().unwrap().is_closed());
    assert!(matches!(
        clone.ping().await,
        Err(DbErr::ConnectionAcquire(_))
    ));
    println!("CONNECTION_LIFECYCLE typed_options_clone_adoption_close");
}

async fn sqlite_lazy() {
    let mut options = ConnectOptions::new("sqlite::memory:");
    options
        .connect_lazy(true)
        .acquire_timeout(Duration::from_secs(3))
        .test_before_acquire_if_idle_for(Duration::ZERO)
        .test_before_acquire(true);
    let connection = Database::connect(options).await.unwrap();
    let pool = connection.get_sqlite_connection_pool().unwrap();
    // The idle threshold takes precedence; the boolean is kept as set.
    assert!(pool.options().get_test_before_acquire());
    assert_eq!(
        pool.options().get_test_before_acquire_if_idle_for(),
        Some(Duration::ZERO)
    );
    // A lazy pool opens no connection before its first statement.
    assert_eq!(pool.size(), 0);
    let one: i64 = driver::query("SELECT 1")
        .fetch_one(pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(one, 1);
    connection.close().await.unwrap();
    println!("CONNECTION_LIFECYCLE sqlite_lazy_idle_threshold");
}

async fn execute(
    connection: &DatabaseConnection,
    sql: &str,
    values: Vec<sql_builder::Value>,
) -> Result<u64, DbErr> {
    match &connection.inner {
        DatabaseConnectionType::SqlitePoolConnection(pool) => pool
            .execute(Statement::from_sql_and_values(
                DbBackend::SQLite,
                sql,
                values,
            ))
            .await
            .map(|result| result.rows_affected()),
        _ => panic!("SQLite required"),
    }
}

async fn metrics_and_execution() {
    let options = ConnectOptions::new("sqlite::memory:");
    let mut connection = Database::connect(options).await.unwrap();
    let before_callback = connection.clone();
    let observations = Arc::new(Mutex::new(vec![]));
    let captured = observations.clone();
    connection.set_metric_callback(move |info| {
        captured.lock().unwrap().push((
            info.statement.sql.clone(),
            info.error.is_some(),
            info.rows,
        ));
    });
    let after_callback = connection.clone();
    let create = "CREATE TABLE metric_values(value INTEGER PRIMARY KEY)";
    match &connection.inner {
        DatabaseConnectionType::SqlitePoolConnection(pool) => {
            pool.execute_unprepared(create).await.unwrap();
        }
        _ => panic!("SQLite required"),
    }
    assert_eq!(
        *observations.lock().unwrap(),
        [(create.to_owned(), false, Some(0))]
    );
    let insert = "INSERT INTO metric_values VALUES(?)";
    assert_eq!(
        execute(&connection, insert, vec![1_i32.into()])
            .await
            .unwrap(),
        1
    );
    assert!(matches!(
        execute(&connection, insert, vec![1_i32.into()]).await,
        Err(DbErr::Exec(_))
    ));
    assert_eq!(
        execute(&before_callback, insert, vec![2_i32.into()])
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        execute(&after_callback, insert, vec![3_i32.into()])
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        *observations.lock().unwrap(),
        [
            (create.to_owned(), false, Some(0)),
            (insert.to_owned(), false, Some(1)),
            (insert.to_owned(), true, None),
            (insert.to_owned(), false, Some(1))
        ]
    );
    let count: i64 = driver::query("SELECT COUNT(*) FROM metric_values")
        .fetch_one(connection.get_sqlite_connection_pool().unwrap())
        .await
        .unwrap()
        .get(0);
    assert_eq!(count, 3);
    after_callback.close().await.unwrap();
    assert!(matches!(
        execute(&connection, insert, vec![4_i32.into()]).await,
        Err(DbErr::ConnectionAcquire(_))
    ));
    assert_eq!(
        observations.lock().unwrap().len(),
        4,
        "acquisition failure occurs before the metric scope"
    );
    println!("CONNECTION_LIFECYCLE execution_metric_success_failure_clone_scope");
}

async fn lifecycle() {
    let work = async {
        configured_pool().await;
        sqlite_lazy().await;
        metrics_and_execution().await;
    };
    use futures_util::future::{Either, select};
    match select(
        Box::pin(work),
        Box::pin(async_io::Timer::after(Duration::from_secs(20))),
    )
    .await
    {
        Either::Left(_) => (),
        Either::Right(_) => panic!("connection lifecycle fixture timed out"),
    }
}

#[cfg(feature = "runtime-tokio")]
#[test]
fn lifecycle_tokio() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(lifecycle());
}
#[cfg(feature = "runtime-async-std")]
#[test]
fn lifecycle_async_std() {
    async_std::task::block_on(lifecycle());
}
