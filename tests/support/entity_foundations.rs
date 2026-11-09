use sql_builder::{Iden, Query, SelectStatement};
use std::borrow::Cow;
use tideorm::orm::*;

#[derive(Copy, Clone, Debug, Default)]
struct Plain;
#[derive(Copy, Clone, Debug, Default)]
struct Scoped;

macro_rules! identifiers {
    ($($ty:ty),+) => {$(
        impl Iden for $ty {
            fn unquoted(&self) -> &str { "events" }
            fn quoted(&self) -> Cow<'static, str> { Cow::Borrowed("events") }
        }
        impl IdenStatic for $ty {
            fn as_str(&self) -> &'static str { "events" }
        }
    )+};
}
identifiers!(Plain, Scoped);
impl EntityName for Plain {
    fn table_name(&self) -> &'static str {
        "events"
    }
}
impl EntityName for Scoped {
    fn table_name(&self) -> &'static str {
        "events"
    }
    fn schema_name(&self) -> Option<&str> {
        Some("tenant")
    }
}

#[test]
fn entity_table_metadata() {
    assert_eq!(Plain.table_name(), "events");
    assert_eq!(Plain.schema_name(), None);
    assert_eq!(Scoped.schema_name(), Some("tenant"));
    for backend in [DbBackend::Postgres, DbBackend::MySQL, DbBackend::SQLite] {
        let plain = Query::select()
            .column("id")
            .from(Plain.table_ref())
            .to_owned();
        let scoped = Query::select()
            .column("id")
            .from(Scoped.table_ref())
            .to_owned();
        let quote = if matches!(backend, DbBackend::MySQL | DbBackend::MariaDB) {
            '`'
        } else {
            '"'
        };
        assert_eq!(
            backend.build(&plain).unwrap().sql,
            format!("SELECT {quote}id{quote} FROM {quote}events{quote}")
        );
        assert_eq!(
            backend.build(&scoped).unwrap().sql,
            format!("SELECT {quote}id{quote} FROM {quote}tenant{quote}.{quote}events{quote}")
        );
    }
}

#[derive(Clone)]
struct Builder(SelectStatement);
impl QueryTrait for Builder {
    type QueryStatement = SelectStatement;
    fn query(&mut self) -> &mut Self::QueryStatement {
        &mut self.0
    }
    fn as_query(&self) -> &Self::QueryStatement {
        &self.0
    }
    fn into_query(self) -> Self::QueryStatement {
        self.0
    }
}

#[test]
fn apply_if_calls_its_closure_only_for_some() {
    let query = Builder(
        Query::select()
            .column("id")
            .from(Plain.table_ref())
            .to_owned(),
    )
    .apply_if(None::<u64>, |_, _| {
        panic!("absent option must not call closure")
    })
    .apply_if(Some(3u64), |mut builder, limit| {
        builder.query().limit(limit);
        builder
    });
    assert_eq!(
        query.build(DbBackend::SQLite).unwrap().sql,
        r#"SELECT "id" FROM "events" LIMIT 3"#
    );
}
