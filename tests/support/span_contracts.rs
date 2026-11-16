#![cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
//! Every statement runs in a `db.query` span carrying OpenTelemetry's
//! database client fields.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use tideorm::orm::{ConnectOptions, Database, DatabaseConnectionType, DbBackend, Statement};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

type Fields = BTreeMap<String, String>;

/// Records every span's target and fields, and what is recorded into it later.
#[derive(Clone, Default)]
struct Spans(Arc<Mutex<Vec<(String, Fields)>>>);

#[derive(Default)]
struct Visited(Fields);

impl Visit for Visited {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
}

impl Subscriber for Spans {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &Attributes<'_>) -> Id {
        let mut visited = Visited::default();
        span.record(&mut visited);
        let mut spans = self.0.lock().unwrap();
        spans.push((span.metadata().target().to_owned(), visited.0));
        Id::from_u64(spans.len() as u64)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut visited = Visited::default();
        values.record(&mut visited);
        self.0.lock().unwrap()[span.into_u64() as usize - 1]
            .1
            .extend(visited.0);
    }

    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

async fn run_statements() {
    let db = Database::connect(ConnectOptions::new("sqlite::memory:"))
        .await
        .unwrap();
    let DatabaseConnectionType::SqlitePoolConnection(pool) = &db.inner else {
        panic!("SQLite required");
    };
    let statement = |sql: &str, values: Vec<tideorm::orm::sql_builder::Value>| {
        Statement::from_sql_and_values(DbBackend::SQLite, sql, values)
    };
    pool.execute_unprepared("CREATE TABLE traced (id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    let insert = "INSERT INTO traced VALUES (?)";
    for id in [1_i32, 2] {
        pool.execute(statement(insert, vec![id.into()]))
            .await
            .unwrap();
    }
    pool.execute(statement(insert, vec![1_i32.into()]))
        .await
        .unwrap_err();
    let rows = pool
        .query_all(statement("SELECT id FROM traced", vec![]))
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn statements_run_in_spans_with_the_database_fields() {
    let spans = Spans::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tracing::subscriber::with_default(spans.clone(), || runtime.block_on(run_statements()));

    let queries: Vec<Fields> = spans
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(target, _)| target == "tideorm::query")
        .map(|(_, fields)| fields.clone())
        .collect();
    let field = |span: &Fields, name: &str| span.get(name).cloned();
    let unprepared = queries
        .iter()
        .find(|span| field(span, "db.query.text").is_none())
        .expect("the unprepared statement's span");
    assert_eq!(
        field(unprepared, "db.system.name").as_deref(),
        Some("sqlite")
    );
    assert_eq!(field(unprepared, "otel.kind").as_deref(), Some("client"));

    let of = |sql: &str| -> Vec<(Option<String>, Option<String>)> {
        queries
            .iter()
            .filter(|span| field(span, "db.query.text").as_deref() == Some(sql))
            .map(|span| {
                (
                    field(span, "db.response.returned_rows"),
                    field(span, "otel.status_code"),
                )
            })
            .collect()
    };
    let row = |rows: &str| (Some(rows.to_owned()), None);
    assert_eq!(
        of("INSERT INTO traced VALUES (?)"),
        [row("1"), row("1"), (None, Some("ERROR".to_owned()))]
    );
    assert_eq!(of("SELECT id FROM traced"), [row("2")]);
}
