//! TideORM statement contracts, exercised without a database.

use tideorm::orm::sql_builder;
use tideorm::orm::{DbBackend, Statement, StatementBuilder, Value, Values};

fn backends() -> [DbBackend; 3] {
    [DbBackend::MySQL, DbBackend::Postgres, DbBackend::SQLite]
}

#[test]
fn every_query_statement_builds_a_statement() {
    fn builder<T: StatementBuilder + Sync>() {}
    builder::<sql_builder::InsertStatement>();
    builder::<sql_builder::SelectStatement>();
    builder::<sql_builder::UpdateStatement>();
    builder::<sql_builder::DeleteStatement>();
    builder::<sql_builder::WithQuery>();
}

#[test]
fn constructors_preserve_no_parameters_versus_explicit_empty_parameters() {
    for backend in backends() {
        let unbound = Statement::from_string(backend, "SELECT 1");
        let bound = Statement::from_sql_and_values(backend, String::from("SELECT 1"), []);
        assert_eq!(unbound.sql, bound.sql);
        assert_eq!(unbound.db_backend, bound.db_backend);
        assert_eq!(unbound.values, None);
        assert_eq!(bound.values, Some(Values(vec![])));
        assert_ne!(unbound, bound);
        assert_eq!(unbound.clone(), unbound);
        assert_eq!(bound.to_string(), "SELECT 1");
    }
}

#[test]
fn display_renders_values_without_changing_bound_sql_or_values() {
    for backend in backends() {
        let sql = if backend == DbBackend::Postgres {
            "SELECT $1, $2, $3"
        } else {
            "SELECT ?, ?, ?"
        };
        let statement = Statement::from_sql_and_values(
            backend,
            sql,
            [42.into(), "hello".into(), Value::Int(None)],
        );
        let original = statement.clone();
        assert_eq!(statement.to_string(), "SELECT 42, 'hello', NULL");
        assert_eq!(statement, original);
    }
}

#[test]
fn backend_helpers_preserve_url_names_and_booleans() {
    for (backend, name, url) in [
        (DbBackend::MySQL, "MySQL", "mysql://localhost/db"),
        (DbBackend::MariaDB, "MariaDB", "MariaDB://localhost/db"),
        (DbBackend::Postgres, "PostgreSQL", "postgres://localhost/db"),
        (DbBackend::SQLite, "SQLite", "sqlite::memory:"),
    ] {
        assert_eq!(backend.as_str(), name);
        assert_eq!(DbBackend::from_url(url), Some(backend));
    }
    assert_eq!(
        DbBackend::from_url("postgresql://localhost/db"),
        Some(DbBackend::Postgres)
    );
    assert_eq!(DbBackend::from_url("https://example.test"), None);
    assert_eq!(DbBackend::from_url("not a URL"), None);
    assert!(DbBackend::Postgres.supports_returning());
    assert!(DbBackend::SQLite.supports_returning());
    assert!(!DbBackend::MySQL.supports_returning());
    assert!(!DbBackend::MariaDB.supports_returning());
}
