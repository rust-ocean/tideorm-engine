//! Fallible argument encoding, compared with the legacy binder.
#![cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]

#[path = "support/legacy_binder.rs"]
mod legacy_binder;

use driver::{Arguments, Execute};
use legacy_binder::LegacyValues;
use sql_builder::{Enum, OptionEnum};
use tideorm_engine::{DbBackend, Statement, Value, Values};

/// The legacy binder, which panics on some values and drops others; the
/// engine's own binder is checked against it.
trait LegacyBinder {
    fn build_driver_query<DB>(&self) -> driver::query::Query<'_, DB, LegacyValues>
    where
        DB: driver::Database,
        LegacyValues: driver::IntoArguments<DB>;
}

impl LegacyBinder for Statement {
    fn build_driver_query<DB>(&self) -> driver::query::Query<'_, DB, LegacyValues>
    where
        DB: driver::Database,
        LegacyValues: driver::IntoArguments<DB>,
    {
        let values = self.values.clone().unwrap_or(Values(Vec::new()));
        driver::query_with(
            driver::AssertSqlSafe(self.sql.as_str()),
            LegacyValues(values),
        )
    }
}

fn scalar_values() -> Vec<Value> {
    use chrono::{Local, TimeZone, Utc};
    let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
    let time = chrono::NaiveTime::from_hms_opt(12, 34, 56).unwrap();
    let datetime = date.and_time(time);
    let utc = Utc.from_utc_datetime(&datetime);
    vec![
        true.into(),
        i8::MIN.into(),
        i16::MIN.into(),
        i32::MIN.into(),
        i64::MIN.into(),
        u8::MAX.into(),
        u16::MAX.into(),
        u32::MAX.into(),
        (i64::MAX as u64).into(),
        1.5_f32.into(),
        (-2.75_f64).into(),
        "λ'\0; --".into(),
        Value::Enum(OptionEnum::Some(Box::new(Enum {
            type_name: "kind".into(),
            value: "one'".into(),
        }))),
        'λ'.into(),
        vec![0_u8, 255, 127].into(),
        serde_json::json!({"values": [null, true, "λ"]}).into(),
        date.into(),
        time.into(),
        datetime.into(),
        utc.into(),
        utc.with_timezone(&Local).into(),
        utc.with_timezone(&chrono::FixedOffset::east_opt(7200).unwrap())
            .into(),
        rust_decimal::Decimal::new(1234500, 4).into(),
        uuid::Uuid::nil().into(),
    ]
}

fn statement(backend: DbBackend, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(backend, "SQL text is not changed or validated", values)
}

/// `statement` as the legacy binder must be given it to bind what the engine
/// binds: on MySQL an offset timestamp goes out as the UTC `DATETIME` it holds
/// (the legacy binder sent text), and on PostgreSQL an `i8` as a `SMALLINT`
/// (the legacy binder sent the one-byte `"char"`).
fn legacy_equivalent(statement: &Statement) -> Statement {
    let mut statement = statement.clone();
    let backend = statement.db_backend;
    if let Some(values) = statement.values.as_mut() {
        for value in &mut values.0 {
            *value = match (backend, &*value) {
                (DbBackend::MySQL, Value::ChronoDateTimeWithTimeZone(at)) => {
                    Value::ChronoDateTime(at.map(|at| at.naive_utc()))
                }
                (DbBackend::Postgres, Value::TinyInt(v)) => Value::SmallInt(v.map(i16::from)),
                _ => continue,
            };
        }
    }
    statement
}

macro_rules! parity {
    ($statement:expr, $database:ty, $method:ident) => {{
        let statement = $statement;
        let saved = statement.clone();
        let expected = statement.values.as_ref().map_or(0, |values| values.0.len());
        for _ in 0..2 {
            let legacy = legacy_equivalent(&statement)
                .build_driver_query::<$database>()
                .take_arguments()
                .unwrap()
                .unwrap();
            let candidate = statement.$method().unwrap();
            assert_eq!(legacy.len(), expected);
            assert_eq!(candidate.len(), expected);
            // All three drivers' argument types show the encoded buffer and its
            // type and null metadata in Debug.
            assert_eq!(format!("{candidate:?}"), format!("{legacy:?}"));
        }
        assert_eq!(statement, saved);
    }};
}

macro_rules! scalar_test {
    ($test:ident, $backend:ident, $database:ty, $method:ident) => {
        #[test]
        fn $test() {
            let values = scalar_values();
            assert_eq!(values.len(), 24);
            for value in values {
                parity!(
                    statement(DbBackend::$backend, vec![value.clone(), value.as_null()]),
                    $database,
                    $method
                );
            }
            for values in [None, Some(Values(vec![]))] {
                parity!(
                    Statement {
                        sql: "unbound".into(),
                        values,
                        db_backend: DbBackend::$backend,
                        log: true,
                    },
                    $database,
                    $method
                );
            }
            parity!(
                statement(DbBackend::$backend, scalar_values()),
                $database,
                $method
            );
            // Raw binding must not acquire model-only finite/decimal guards.
            let raw = statement(
                DbBackend::$backend,
                vec![
                    f32::NAN.into(),
                    f32::INFINITY.into(),
                    f64::NEG_INFINITY.into(),
                    rust_decimal::Decimal::MAX.into(),
                ],
            );
            let legacy = raw
                .build_driver_query::<$database>()
                .take_arguments()
                .unwrap()
                .unwrap();
            let candidate = raw.$method().unwrap();
            assert_eq!(candidate.len(), 4);
            assert_eq!(format!("{candidate:?}"), format!("{legacy:?}"));
        }
    };
}

#[cfg(feature = "postgres")]
scalar_test!(
    postgres_scalars,
    Postgres,
    driver::Postgres,
    try_postgres_arguments
);
#[cfg(feature = "mysql")]
scalar_test!(mysql_scalars, MySQL, driver::MySql, try_mysql_arguments);
#[cfg(feature = "sqlite")]
scalar_test!(sqlite_scalars, SQLite, driver::Sqlite, try_sqlite_arguments);

#[cfg(any(feature = "postgres", feature = "sqlite"))]
macro_rules! overflow_test {
    ($test:ident, $backend:ident, $database:ty, $method:ident) => {
        #[test]
        fn $test() {
            for value in [i64::MAX as u64 + 1, u64::MAX] {
                let stmt = statement(
                    DbBackend::$backend,
                    vec![7_i32.into(), value.into(), "after".into()],
                );
                let saved = stmt.clone();
                assert!(
                    std::panic::catch_unwind(|| {
                        stmt.build_driver_query::<$database>().take_arguments()
                    })
                    .is_err()
                );
                let error = stmt.$method().unwrap_err();
                assert!(matches!(error, driver::Error::Encode(_)));
                assert!(std::error::Error::source(&error).is_some());
                assert_eq!(stmt, saved);
            }
        }
    };
}

#[cfg(feature = "postgres")]
overflow_test!(
    postgres_overflow_returns_error,
    Postgres,
    driver::Postgres,
    try_postgres_arguments
);
#[cfg(feature = "sqlite")]
overflow_test!(
    sqlite_overflow_returns_error,
    SQLite,
    driver::Sqlite,
    try_sqlite_arguments
);

#[cfg(feature = "mysql")]
#[test]
fn mysql_preserves_full_unsigned_range() {
    parity!(
        statement(
            DbBackend::MySQL,
            vec![u64::MAX.into(), Value::BigUnsigned(None)]
        ),
        driver::MySql,
        try_mysql_arguments
    );
}

#[cfg(feature = "mysql")]
#[test]
fn mysql_driver_encoding_error_is_not_discarded() {
    // The driver rejects years that cannot fit the MySQL protocol's unsigned field.
    // The legacy adapter discards that error and silently omits the parameter.
    let date = chrono::NaiveDate::from_ymd_opt(-1, 1, 1).unwrap();
    for value in [
        Value::from(date),
        Value::from(date.and_hms_opt(0, 0, 0).unwrap()),
    ] {
        let stmt = statement(DbBackend::MySQL, vec![7_i32.into(), value, "after".into()]);
        let saved = stmt.clone();
        let legacy = stmt
            .build_driver_query::<driver::MySql>()
            .take_arguments()
            .unwrap()
            .unwrap();
        assert_eq!(
            legacy.len(),
            2,
            "legacy silently drops the invalid middle parameter"
        );
        let mut arguments = driver::mysql::MySqlArguments::default();
        let driver_error = arguments.add(date).unwrap_err();
        let error = stmt.try_mysql_arguments().unwrap_err();
        match error {
            driver::Error::Encode(source) => {
                assert_eq!(source.to_string(), driver_error.to_string())
            }
            other => panic!("expected original encoding error, got {other}"),
        }
        assert_eq!(stmt, saved);
    }
}

#[cfg(feature = "postgres")]
#[test]
fn postgres_arrays_match_legacy_encodings() {
    use sql_builder::ArrayType;
    for value in scalar_values() {
        let array_type = match &value {
            Value::Enum(OptionEnum::Some(value)) => {
                ArrayType::Enum(Box::new(value.type_name.clone()))
            }
            _ => value.array_type(),
        };
        // One-byte integers bind widened, as their scalars do; the legacy
        // binder sent `"char"[]` and a byte string. Tested separately.
        if matches!(array_type, ArrayType::TinyInt | ArrayType::TinyUnsigned) {
            continue;
        }
        for values in [
            None,
            Some(Box::new(vec![])),
            Some(Box::new(vec![value.clone(), value.clone()])),
        ] {
            parity!(
                statement(
                    DbBackend::Postgres,
                    vec![Value::Array(array_type.clone(), values)]
                ),
                driver::Postgres,
                try_postgres_arguments
            );
        }
    }
}

#[cfg(feature = "postgres")]
#[test]
fn postgres_invalid_arrays_return_errors() {
    use sql_builder::ArrayType;
    let cases = [
        (ArrayType::Int, vec![1_i32.into(), "wrong type".into()]),
        (ArrayType::Int, vec![Value::Int(None)]),
        (ArrayType::BigUnsigned, vec![1_u64.into(), u64::MAX.into()]),
        (
            ArrayType::Enum(Box::new("kind".into())),
            vec!["not an enum".into()],
        ),
        (
            ArrayType::Enum(Box::new("kind".into())),
            vec![Value::Enum(OptionEnum::None("kind".into()))],
        ),
        (
            ArrayType::Int,
            vec![Value::Array(
                ArrayType::Int,
                Some(Box::new(vec![1_i32.into()])),
            )],
        ),
    ];
    for (ty, values) in cases {
        let stmt = statement(
            DbBackend::Postgres,
            vec![
                7_i32.into(),
                Value::Array(ty, Some(Box::new(values))),
                "after".into(),
            ],
        );
        let saved = stmt.clone();
        assert!(
            std::panic::catch_unwind(|| stmt
                .build_driver_query::<driver::Postgres>()
                .take_arguments())
            .is_err()
        );
        assert!(matches!(
            stmt.try_postgres_arguments(),
            Err(driver::Error::Encode(_))
        ));
        assert_eq!(stmt, saved);
    }
}

#[cfg(feature = "postgres")]
#[test]
fn postgres_byte_arrays_use_widened_elements() {
    use sql_builder::ArrayType;
    let stmt = statement(
        DbBackend::Postgres,
        vec![Value::Array(
            ArrayType::TinyInt,
            Some(Box::new(vec![i8::MIN.into(), i8::MAX.into()])),
        )],
    );
    let mut expected = driver::postgres::PgArguments::default();
    expected.add(Some(vec![-128_i16, 127])).unwrap();
    assert_eq!(
        format!("{:?}", stmt.try_postgres_arguments().unwrap()),
        format!("{expected:?}")
    );
    for values in [
        None,
        Some(Box::new(vec![])),
        Some(Box::new(vec![0_u8.into(), u8::MAX.into()])),
    ] {
        let stmt = statement(
            DbBackend::Postgres,
            vec![Value::Array(ArrayType::TinyUnsigned, values)],
        );
        assert!(
            std::panic::catch_unwind(|| stmt
                .build_driver_query::<driver::Postgres>()
                .take_arguments())
            .is_err()
        );
        let candidate = stmt.try_postgres_arguments().unwrap();
        assert_eq!(candidate.len(), 1);
    }
    let stmt = statement(
        DbBackend::Postgres,
        vec![Value::Array(
            ArrayType::TinyUnsigned,
            Some(Box::new(vec![0_u8.into(), u8::MAX.into()])),
        )],
    );
    let mut expected = driver::postgres::PgArguments::default();
    expected.add(Some(vec![0_i16, 255])).unwrap();
    assert_eq!(
        format!("{:?}", stmt.try_postgres_arguments().unwrap()),
        format!("{expected:?}")
    );
}

#[cfg(all(feature = "postgres", any(feature = "mysql", feature = "sqlite")))]
#[test]
fn non_postgres_arrays_return_errors() {
    use sql_builder::ArrayType;
    for values in [
        None,
        Some(Box::new(vec![])),
        Some(Box::new(vec![1_i32.into()])),
    ] {
        #[cfg(feature = "mysql")]
        {
            let stmt = statement(
                DbBackend::MySQL,
                vec![Value::Array(ArrayType::Int, values.clone())],
            );
            assert!(
                std::panic::catch_unwind(|| stmt
                    .build_driver_query::<driver::MySql>()
                    .take_arguments())
                .is_err()
            );
            assert!(matches!(
                stmt.try_mysql_arguments(),
                Err(driver::Error::Encode(_))
            ));
        }
        #[cfg(feature = "sqlite")]
        {
            let stmt = statement(
                DbBackend::SQLite,
                vec![Value::Array(ArrayType::Int, values.clone())],
            );
            assert!(
                std::panic::catch_unwind(|| stmt
                    .build_driver_query::<driver::Sqlite>()
                    .take_arguments())
                .is_err()
            );
            assert!(matches!(
                stmt.try_sqlite_arguments(),
                Err(driver::Error::Encode(_))
            ));
        }
    }
}

#[cfg(all(
    feature = "sqlite",
    any(feature = "runtime-tokio", feature = "runtime-async-std")
))]
#[path = "support/sqlite_binding_contracts.rs"]
mod sqlite_fixture;

#[cfg(all(
    feature = "sqlite",
    any(feature = "runtime-tokio", feature = "runtime-async-std")
))]
async fn sqlite_bindings() {
    use driver::Row;
    let pool = driver::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlite_fixture::sqlite_binding_contracts(|sql, values| {
        let pool = pool.clone();
        async move {
            let statement = Statement {
                sql,
                values,
                db_backend: DbBackend::SQLite,
                log: true,
            };
            let before = statement.clone();
            let row = driver::query_with(
                driver::AssertSqlSafe(statement.sql.as_str()),
                statement.try_sqlite_arguments().unwrap(),
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(statement, before);
            row.try_get::<Option<String>, _>("value").unwrap()
        }
    })
    .await;
    pool.close().await;
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[test]
fn sqlite_fallible_bindings_tokio() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(sqlite_bindings());
}

#[cfg(all(feature = "sqlite", feature = "runtime-async-std"))]
#[test]
fn sqlite_fallible_bindings_async_std() {
    async_std::task::block_on(sqlite_bindings());
}

/// The engine's own statements bind through the fallible binder: a value the
/// driver cannot encode fails that statement, on the pool and in a
/// transaction, is reported to the metric callback as failed, and leaves the
/// connection and the transaction usable.
#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[test]
fn unencodable_values_fail_their_statement() {
    use std::sync::{Arc, Mutex};
    use tideorm_engine::{ConnectOptions, ConnectionTrait, DbErr, RuntimeErr, TransactionTrait};

    fn is_encode_error(error: &DbErr) -> bool {
        matches!(
            error,
            DbErr::Exec(RuntimeErr::DriverError(error)) | DbErr::Query(RuntimeErr::DriverError(error))
                if matches!(**error, driver::Error::Encode(_))
        )
    }
    let overflowing =
        |sql: &str| Statement::from_sql_and_values(DbBackend::SQLite, sql, [Value::from(u64::MAX)]);
    let count = |db: &'_ tideorm_engine::DatabaseConnection| {
        let db = db.clone();
        async move {
            let rows = db
                .query_all_raw(Statement::from_string(
                    DbBackend::SQLite,
                    "SELECT COUNT(*) AS n FROM t",
                ))
                .await
                .unwrap();
            rows[0].try_get::<i64>("n").unwrap()
        }
    };

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut options = ConnectOptions::new("sqlite::memory:");
            options.max_connections(1).min_connections(1);
            let mut db = tideorm_engine::Database::connect(options).await.unwrap();
            let reported = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&reported);
            db.set_metric_callback(move |info| {
                log.lock()
                    .unwrap()
                    .push((info.statement.sql.clone(), info.error.is_some()));
            });
            db.execute_unprepared("CREATE TABLE t(id INTEGER)")
                .await
                .unwrap();

            let error = db
                .execute_raw(overflowing("INSERT INTO t VALUES (?)"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, DbErr::Exec(_)) && is_encode_error(&error),
                "{error:?}"
            );
            let error = db
                .query_all_raw(overflowing("SELECT ? AS value"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, DbErr::Query(_)) && is_encode_error(&error),
                "{error:?}"
            );
            let error = db
                .query_one_raw(overflowing("SELECT ? AS value"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, DbErr::Query(_)) && is_encode_error(&error),
                "{error:?}"
            );
            assert_eq!(count(&db).await, 0);

            let transaction = db.begin().await.unwrap();
            let error = transaction
                .execute_raw(overflowing("INSERT INTO t VALUES (?)"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, DbErr::Exec(_)) && is_encode_error(&error),
                "{error:?}"
            );
            let error = transaction
                .query_all_raw(overflowing("SELECT ? AS value"))
                .await
                .unwrap_err();
            assert!(
                matches!(error, DbErr::Query(_)) && is_encode_error(&error),
                "{error:?}"
            );
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::SQLite,
                    "INSERT INTO t VALUES (?)",
                    [Value::from(i64::MAX as u64)],
                ))
                .await
                .unwrap();
            transaction.commit().await.unwrap();
            assert_eq!(count(&db).await, 1);

            let reported = reported.lock().unwrap().clone();
            let failed: Vec<_> = reported.iter().filter(|(_, failed)| *failed).collect();
            assert_eq!(failed.len(), 5, "{reported:?}");
            assert!(
                failed
                    .iter()
                    .all(|(sql, _)| sql.starts_with("INSERT") || sql.starts_with("SELECT ?"))
            );
        });
}
