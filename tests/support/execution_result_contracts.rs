//! Execution results: conversions from each driver's result, ids and debug output.

#[cfg(any(feature = "postgres", feature = "sqlite"))]
use tideorm::orm::DbErr;
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
use tideorm::orm::ExecResult;

#[cfg(any(feature = "postgres", feature = "sqlite"))]
fn panic_message(f: impl FnOnce() -> u64 + std::panic::UnwindSafe) -> String {
    let error = std::panic::catch_unwind(f).expect_err("expected baseline panic");
    error
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| error.downcast_ref::<&str>().unwrap().to_string())
}

#[test]
#[cfg(feature = "postgres")]
fn postgres_result_conversion_and_unsupported_id() {
    let result: ExecResult = tideorm::orm::driver::postgres::PgQueryResult::default().into();
    assert_eq!(result.rows_affected(), 0);
    assert_eq!(
        format!("{result:?}"),
        "ExecResult { result: Postgres(PgQueryResult { rows_affected: 0 }) }"
    );
    assert_eq!(
        panic_message(|| result.last_insert_id()),
        "Should not retrieve last_insert_id this way"
    );
    assert_eq!(result.try_last_insert_id(), Err(DbErr::UnpackInsertId));
}

#[test]
#[cfg(feature = "mysql")]
fn mysql_result_conversion_and_zero_id() {
    let result: ExecResult = tideorm::orm::driver::mysql::MySqlQueryResult::default().into();
    assert_eq!((result.rows_affected(), result.last_insert_id()), (0, 0));
    assert_eq!(result.try_last_insert_id(), Ok(0));
    assert_eq!(
        format!("{result:?}"),
        "ExecResult { result: MySql(MySqlQueryResult { rows_affected: 0, last_insert_id: 0 }) }"
    );
}

#[test]
#[cfg(feature = "sqlite")]
fn sqlite_result_conversion_and_zero_id() {
    let result: ExecResult = tideorm::orm::driver::sqlite::SqliteQueryResult::default().into();
    assert_eq!((result.rows_affected(), result.last_insert_id()), (0, 0));
    assert_eq!(result.try_last_insert_id(), Ok(0));
    assert_eq!(
        format!("{result:?}"),
        "ExecResult { result: Sqlite(SqliteQueryResult { changes: 0, last_insert_rowid: 0 }) }"
    );
}

#[cfg(all(
    feature = "sqlite",
    any(feature = "runtime-tokio", feature = "runtime-async-std")
))]
async fn sqlite_results() {
    use tideorm::orm::driver::{self, Connection, Row};
    let mut connection = driver::SqliteConnection::connect("sqlite::memory:")
        .await
        .unwrap();
    let cases = [
        (
            "CREATE TABLE results(id INTEGER PRIMARY KEY, value INTEGER)",
            0,
            0,
        ),
        ("INSERT INTO results VALUES(7, 1)", 1, 7),
        ("INSERT INTO results VALUES(8, 2), (9, 3)", 2, 9),
        ("UPDATE results SET value = value + 10", 3, 9),
        ("UPDATE results SET value = value", 3, 9),
        ("UPDATE results SET value = 0 WHERE id = 99", 0, 9),
        ("DELETE FROM results WHERE id = 7", 1, 9),
        ("DELETE FROM results WHERE id = 99", 0, 9),
        ("INSERT INTO results VALUES(-8, 4)", 1, -8),
        ("INSERT INTO results VALUES(0, 5)", 1, 0),
        (
            "INSERT INTO results VALUES(9223372036854775807, 6)",
            1,
            i64::MAX,
        ),
    ];
    for (sql, rows, id) in cases {
        let outcome = driver::query(sql).execute(&mut connection).await.unwrap();
        assert_eq!(
            (outcome.rows_affected(), outcome.last_insert_rowid()),
            (rows, id),
            "{sql}"
        );
        let debug = format!("ExecResult {{ result: Sqlite({outcome:?}) }}");
        let result: ExecResult = outcome.into();
        assert_eq!(format!("{result:?}"), debug);
        assert_eq!(result.rows_affected(), rows);
        if id < 0 {
            assert_eq!(
                panic_message(|| result.last_insert_id()),
                "internal error: entered unreachable code: negative last_insert_rowid"
            );
            assert_eq!(result.try_last_insert_id(), Err(DbErr::UnpackInsertId));
        } else {
            assert_eq!(result.last_insert_id(), id as u64);
            assert_eq!(result.try_last_insert_id(), Ok(id as u64));
        }
    }
    let rows: Vec<(i64, i64)> = driver::query("SELECT id, value FROM results ORDER BY id")
        .fetch_all(&mut connection)
        .await
        .unwrap()
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    assert_eq!(rows, vec![(-8, 4), (0, 5), (8, 12), (9, 13), (i64::MAX, 6)]);
    connection.close().await.unwrap();
}

#[test]
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
fn sqlite_results_tokio() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(sqlite_results());
}

#[test]
#[cfg(all(feature = "sqlite", feature = "runtime-async-std"))]
fn sqlite_results_async_std() {
    async_std::task::block_on(sqlite_results());
}
