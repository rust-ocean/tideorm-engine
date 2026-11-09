//! Error contracts frozen against the restored TideORM baseline before adaptation.
//! Keep identical when testing the engine through the test-only spelling adapter.

use std::{error::Error, sync::Arc};
use tideorm::orm::{ConnAcquireErr, DbErr, RuntimeErr, TryGetError};

#[test]
fn all_database_error_variants_keep_messages_and_clone_equality() {
    let cases = [
        (
            DbErr::ConnectionAcquire(ConnAcquireErr::Timeout),
            "Failed to acquire connection from pool: Connection pool timed out",
        ),
        (
            DbErr::ConnectionAcquire(ConnAcquireErr::ConnectionClosed),
            "Failed to acquire connection from pool: Connection closed",
        ),
        (
            DbErr::TryIntoErr {
                from: "a",
                into: "b",
                source: Arc::new(std::io::Error::other("conversion")),
            },
            "Error converting `a` into `b`: conversion",
        ),
        (
            DbErr::Conn(RuntimeErr::Internal("detail".into())),
            "Connection Error: detail",
        ),
        (
            DbErr::Exec(RuntimeErr::Internal("detail".into())),
            "Execution Error: detail",
        ),
        (
            DbErr::Query(RuntimeErr::Internal("detail".into())),
            "Query Error: detail",
        ),
        (
            DbErr::ConvertFromU64("field"),
            "Type 'field' cannot be converted from u64",
        ),
        (DbErr::UnpackInsertId, "Failed to unpack last_insert_id"),
        (
            DbErr::UpdateGetPrimaryKey,
            "Failed to get primary key from model",
        ),
        (
            DbErr::RecordNotFound("detail".into()),
            "RecordNotFound Error: detail",
        ),
        (
            DbErr::AttrNotSet("field".into()),
            "Attribute field is NotSet",
        ),
        (DbErr::Custom("detail".into()), "Custom Error: detail"),
        (DbErr::Type("detail".into()), "Type Error: detail"),
        (DbErr::Json("detail".into()), "Json Error: detail"),
        (
            DbErr::TransactionAborted {
                reason: "detail".into(),
                cause: None,
            },
            "Transaction aborted: detail",
        ),
        (
            DbErr::Decode {
                column: "at".into(),
                source: Arc::new(std::io::Error::other("out of range")),
            },
            "column 'at' cannot be read: out of range",
        ),
        (DbErr::RecordNotInserted, "None of the records are inserted"),
        (DbErr::RecordNotUpdated, "None of the records are updated"),
        (
            DbErr::BackendNotSupported {
                db: "db",
                ctx: "ctx",
            },
            "Operation not supported by backend db: ctx",
        ),
        (
            DbErr::KeyArityMismatch {
                expected: 2,
                received: 1,
            },
            "Key arity mismatch: expected 2, received 1",
        ),
        // Preserve the existing spelling: error text is observable behavior.
        (
            DbErr::PrimaryKeyNotSet { ctx: "ctx" },
            "Primay key not set for ctx",
        ),
    ];
    for (error, message) in cases {
        assert_eq!(error.to_string(), message);
        assert_eq!(error, error.clone());
    }
}

#[test]
fn equality_uses_rendered_text_instead_of_source_identity() {
    let make = |source| DbErr::TryIntoErr {
        from: "a",
        into: "b",
        source,
    };
    let first = make(Arc::new(std::io::Error::other("same")));
    let second = make(Arc::new(std::fmt::Error));
    let same_text = make(Arc::new(std::io::Error::other(std::fmt::Error.to_string())));
    assert_eq!(second, same_text);
    assert_ne!(first, second);
}

#[test]
fn sources_preserve_runtime_acquisition_and_conversion_boundaries() {
    for error in [
        DbErr::Conn(RuntimeErr::Internal("detail".into())),
        DbErr::Exec(RuntimeErr::Internal("detail".into())),
        DbErr::Query(RuntimeErr::Internal("detail".into())),
    ] {
        let source = error.source().unwrap();
        assert!(source.downcast_ref::<RuntimeErr>().is_some());
        assert_eq!(source.to_string(), "detail");
        assert!(source.source().is_none());
    }
    let error = DbErr::ConnectionAcquire(ConnAcquireErr::Timeout);
    assert_eq!(
        error.source().unwrap().downcast_ref::<ConnAcquireErr>(),
        Some(&ConnAcquireErr::Timeout)
    );
    let source: Arc<dyn Error + Send + Sync> = Arc::new(std::io::Error::other("conversion"));
    let error = DbErr::TryIntoErr {
        from: "a",
        into: "b",
        source: source.clone(),
    };
    let exposed = error
        .source()
        .unwrap()
        .downcast_ref::<Arc<dyn Error + Send + Sync>>()
        .unwrap();
    assert!(Arc::ptr_eq(&source, exposed));
    assert!(exposed.as_ref().is::<std::io::Error>());
    let DbErr::TryIntoErr { source: cloned, .. } = error.clone() else {
        panic!("wrong variant")
    };
    assert!(Arc::ptr_eq(&source, &cloned));
    assert!(DbErr::Custom("detail".into()).source().is_none());
}

#[test]
fn decoding_conversions_preserve_null_context_and_inner_errors() {
    let converted: DbErr = TryGetError::Null("model.field".into()).into();
    assert_eq!(
        converted,
        DbErr::Type("A null value was encountered while decoding model.field".into())
    );
    let original = DbErr::Query(RuntimeErr::Internal("decode".into()));
    let wrapped: TryGetError = original.clone().into();
    assert!(matches!(&wrapped, TryGetError::DbErr(DbErr::Query(_))));
    assert_eq!(DbErr::from(wrapped), original);
    assert_eq!(
        format!("{:?}", TryGetError::Null("field".into())),
        "Null(\"field\")"
    );
}

#[test]
fn auxiliary_errors_keep_public_fields_traits_and_module_aliases() {
    fn thread_safe<T: Error + Send + Sync + 'static>() {}
    fn eq_clone<T: Eq + Clone>() {}
    thread_safe::<DbErr>();
    thread_safe::<RuntimeErr>();
    thread_safe::<ConnAcquireErr>();
    eq_clone::<DbErr>();
    let _: tideorm::orm::error::DbErr = DbErr::RecordNotInserted;
    let timeout = ConnAcquireErr::Timeout;
    let copied = timeout;
    assert_eq!(timeout, copied);
    assert_ne!(timeout, ConnAcquireErr::ConnectionClosed);
}

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
#[test]
fn driver_identity_arc_and_source_behavior_remain_unchanged() {
    use tideorm::orm::{DriverError, driver};
    let driver: Arc<DriverError> = Arc::new(driver::Error::RowNotFound);
    let runtime = RuntimeErr::DriverError(driver.clone());
    // RuntimeErr does not expose the driver error via Error::source. Preserve that
    // boundary while keeping the exact shared driver available by matching.
    assert!(runtime.source().is_none());
    assert_eq!(runtime.to_string(), driver.to_string());
    let RuntimeErr::DriverError(cloned) = runtime.clone() else {
        panic!("wrong runtime")
    };
    assert!(Arc::ptr_eq(&driver, &cloned));
    for error in [
        DbErr::Conn(runtime.clone()),
        DbErr::Exec(runtime.clone()),
        DbErr::Query(runtime),
    ] {
        assert!(error.source().unwrap().is::<RuntimeErr>());
    }
    #[cfg(feature = "postgres")]
    fn postgres_identity(
        value: tideorm::orm::PostgresDriverError,
    ) -> driver::postgres::PgDatabaseError {
        value
    }
    #[cfg(feature = "postgres")]
    let _ = postgres_identity;
    #[cfg(feature = "mysql")]
    fn mysql_identity(value: tideorm::orm::MySqlDriverError) -> driver::mysql::MySqlDatabaseError {
        value
    }
    #[cfg(feature = "mysql")]
    let _ = mysql_identity;
    #[cfg(feature = "sqlite")]
    fn sqlite_identity(value: tideorm::orm::SqliteDriverError) -> driver::sqlite::SqliteError {
        value
    }
    #[cfg(feature = "sqlite")]
    let _ = sqlite_identity;
}

#[cfg(all(
    feature = "sqlite",
    any(feature = "runtime-tokio", feature = "runtime-async-std")
))]
async fn sqlite_errors() {
    use tideorm::orm::driver::{self, Connection, error::ErrorKind};
    let mut db = driver::SqliteConnection::connect("sqlite::memory:")
        .await
        .unwrap();
    for statement in [
        "PRAGMA foreign_keys=ON",
        "CREATE TABLE parent(id INTEGER PRIMARY KEY)",
        "CREATE TABLE child(id INTEGER PRIMARY KEY, value TEXT NOT NULL UNIQUE, parent_id INTEGER REFERENCES parent(id), qty INTEGER CHECK(qty > 0))",
        "INSERT INTO parent VALUES(1)",
        "INSERT INTO child VALUES(1, 'first', 1, 1)",
    ] {
        driver::query(statement).execute(&mut db).await.unwrap();
    }
    for (sql, code, kind) in [
        (
            "INSERT INTO child VALUES(1, 'second', 1, 1)",
            "1555",
            ErrorKind::UniqueViolation,
        ),
        (
            "INSERT INTO child VALUES(2, 'first', 1, 1)",
            "2067",
            ErrorKind::UniqueViolation,
        ),
        (
            "INSERT INTO child VALUES(2, 'second', 999, 1)",
            "787",
            ErrorKind::ForeignKeyViolation,
        ),
        (
            "INSERT INTO child VALUES(2, NULL, 1, 1)",
            "1299",
            ErrorKind::NotNullViolation,
        ),
        (
            "INSERT INTO child VALUES(2, 'second', 1, 0)",
            "275",
            ErrorKind::CheckViolation,
        ),
        (
            "SELECT * FROM nonexistent_table",
            "1",
            ErrorKind::UndefinedTable,
        ),
        (
            "SELECT missing_column FROM child",
            "1",
            ErrorKind::UndefinedColumn,
        ),
        ("SELEC 1", "1", ErrorKind::SyntaxError),
    ] {
        let driver = Arc::new(driver::query(sql).execute(&mut db).await.unwrap_err());
        let database = driver.as_database_error().unwrap();
        assert_eq!(database.code().as_deref(), Some(code));
        let concrete: &tideorm::orm::SqliteDriverError = database.try_downcast_ref().unwrap();
        assert_eq!(
            concrete.to_string(),
            format!("(code: {code}) {}", database.message())
        );
        assert_eq!(database.kind(), kind, "{sql}");
        let runtime = RuntimeErr::DriverError(driver.clone());
        for error in [DbErr::Exec(runtime.clone()), DbErr::Query(runtime)] {
            let (DbErr::Exec(RuntimeErr::DriverError(retained))
            | DbErr::Query(RuntimeErr::DriverError(retained))) = error.clone()
            else {
                panic!("wrong error")
            };
            assert!(Arc::ptr_eq(&driver, &retained));
            assert_eq!(
                retained.as_database_error().unwrap().code().as_deref(),
                Some(code)
            );
        }
    }
    db.close().await.unwrap();
}

#[cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
#[test]
fn real_sqlite_classification_on_tokio() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(sqlite_errors());
}

#[cfg(all(feature = "sqlite", feature = "runtime-async-std"))]
#[test]
fn real_sqlite_classification_on_async_std() {
    async_std::task::block_on(sqlite_errors());
}
