//! How the engine renders query statements and conditions, without a database.

use tideorm_engine::sql_builder::{Alias, Condition, Expr, ExprTrait, Query, RenderError};
use tideorm_engine::{DbBackend, Value, Values};

#[test]
fn a_limit_is_written_into_the_sql_and_the_offset_stays_bound() {
    let select = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("example"))
        .and_where(Expr::col(Alias::new("name")).eq("ada"))
        .limit(20)
        .offset(40)
        .to_owned();

    for (backend, sql) in [
        (
            DbBackend::Postgres,
            "SELECT \"id\" FROM \"example\" WHERE \"name\" = $1 LIMIT 20 OFFSET $2",
        ),
        (
            DbBackend::MySQL,
            "SELECT `id` FROM `example` WHERE `name` = ? LIMIT 20 OFFSET ?",
        ),
        (
            DbBackend::SQLite,
            "SELECT \"id\" FROM \"example\" WHERE \"name\" = ? LIMIT 20 OFFSET ?",
        ),
    ] {
        let statement = backend.build(&select).unwrap();
        assert_eq!(statement.sql, sql);
        assert_eq!(
            statement.values,
            Some(Values(vec!["ada".into(), Value::BigUnsigned(Some(40))]))
        );
    }
}

#[test]
fn a_limit_stays_ahead_of_the_lock_clause_and_inside_subqueries() {
    let inner = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("example"))
        .limit(5)
        .to_owned();
    let select = Query::select()
        .column(Alias::new("id"))
        .from(Alias::new("example"))
        .and_where(Expr::col(Alias::new("id")).in_subquery(inner))
        .and_where(Expr::col(Alias::new("id")).gt(3))
        .limit(1)
        .lock_exclusive()
        .to_owned();

    let statement = DbBackend::Postgres.build(&select).unwrap();
    assert_eq!(
        statement.sql,
        "SELECT \"id\" FROM \"example\" WHERE \"id\" IN (SELECT \"id\" FROM \"example\" LIMIT 5) \
         AND \"id\" > $1 LIMIT 1 FOR UPDATE"
    );
    assert_eq!(statement.values, Some(Values(vec![3.into()])));
}

#[test]
fn a_condition_renders_as_a_predicate_with_its_values() {
    let condition = Condition::all().add(Expr::col(Alias::new("id")).eq(7)).add(
        Condition::any()
            .add(Expr::col(Alias::new("name")).eq("ada"))
            .add(Expr::col(Alias::new("name")).is_null()),
    );

    for (backend, sql) in [
        (
            DbBackend::Postgres,
            "\"id\" = $1 AND (\"name\" = $2 OR \"name\" IS NULL)",
        ),
        (
            DbBackend::MySQL,
            "`id` = ? AND (`name` = ? OR `name` IS NULL)",
        ),
        (
            DbBackend::SQLite,
            "\"id\" = ? AND (\"name\" = ? OR \"name\" IS NULL)",
        ),
    ] {
        let (rendered, values) = backend
            .build_condition(&condition)
            .unwrap()
            .build(backend.dialect());
        assert_eq!(rendered, sql);
        let values = Values(values);
        assert_eq!(values, Values(vec![7.into(), "ada".into()]));

        let select = Query::select()
            .expr(Expr::cust("1"))
            .cond_where(condition.clone())
            .to_owned();
        let statement = backend.build(&select).unwrap();
        assert_eq!(statement.sql, format!("SELECT 1 WHERE {rendered}"));
        assert_eq!(statement.values, Some(values));
    }
}

#[test]
fn a_condition_the_backend_cannot_render_is_an_error() {
    let any =
        Expr::col(Alias::new("id")).eq(Expr::any(Query::select().expr(Expr::cust("1")).to_owned()));
    let condition = Condition::all().add(any);
    assert!(DbBackend::SQLite.build_condition(&condition).is_err());
    assert!(DbBackend::Postgres.build_condition(&condition).is_ok());

    let missing = RenderError::MissingValue { index: 1, count: 0 };
    let invalid = Condition::all().add(Expr::Invalid(missing.clone()));
    for backend in [DbBackend::Postgres, DbBackend::MySQL, DbBackend::SQLite] {
        let error = backend.build_condition(&invalid).unwrap_err();
        assert!(error.to_string().contains(&missing.to_string()));
    }
}

#[test]
fn malformed_statement_display_keeps_placeholders_and_bindings() {
    use tideorm_engine::Statement;
    for (backend, sql, values) in [
        (DbBackend::Postgres, "SELECT $0", vec![1.into()]),
        (DbBackend::Postgres, "SELECT $2", vec![1.into()]),
        (
            DbBackend::Postgres,
            "SELECT $999999999999999999999999999",
            vec![1.into()],
        ),
        (DbBackend::MySQL, "SELECT ? + ?", vec![1.into()]),
        (DbBackend::SQLite, "SELECT 1", vec![1.into()]),
    ] {
        let statement = Statement::from_sql_and_values(backend, sql, values);
        let original = statement.clone();
        let shown = statement.to_string();
        assert!(
            shown.starts_with(&format!("{sql} /* values not inlined: ")),
            "{shown}"
        );
        assert_eq!(statement, original);
    }
}

#[test]
fn unicode_statement_display_preserves_bound_characters() {
    use tideorm_engine::Statement;
    for backend in [DbBackend::MySQL, DbBackend::Postgres, DbBackend::SQLite] {
        for character in ['\u{03bb}', '\u{e9}', '\'', '\u{1f642}'] {
            let sql = if backend == DbBackend::Postgres {
                "SELECT $1"
            } else {
                "SELECT ?"
            };
            let statement = Statement::from_sql_and_values(backend, sql, [character.into()]);
            let expected =
                Statement::from_sql_and_values(backend, sql, [character.to_string().into()]);
            assert_eq!(statement.to_string(), expected.to_string());
            assert_eq!(statement.values, Some(Values(vec![character.into()])));
        }
    }
}
