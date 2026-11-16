use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use tracing::{Instrument, Span};

use crate::DbErr;

pub(crate) type Callback = Arc<dyn Fn(&Info<'_>) + Send + Sync>;

#[derive(Debug)]
/// Information about a single query execution, passed to the callback
/// registered via [`DatabaseConnection::set_metric_callback`](crate::DatabaseConnection::set_metric_callback).
pub struct Info<'a> {
    /// Query execution duration
    pub elapsed: Duration,
    /// SQL statement with parameters
    pub statement: &'a crate::Statement,
    /// The error the statement failed with
    pub error: Option<&'a DbErr>,
    /// The rows a statement that succeeded affected (a write) or returned (a
    /// query)
    pub rows: Option<u64>,
}

/// Runs `work` inside `span`, recording its rows or its failure there, and,
/// when there is an observer, reports to its callback how long the statement
/// took (on the monotonic clock), and its error or the rows `rows` counts in
/// its result.
#[cfg_attr(
    not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
    allow(dead_code)
)]
pub(crate) async fn observed<T>(
    span: Span,
    observer: Option<(&Callback, &crate::Statement)>,
    work: impl Future<Output = Result<T, DbErr>>,
    rows: impl FnOnce(&T) -> u64,
) -> Result<T, DbErr> {
    let start = observer.map(|_| Instant::now());
    let result = work.instrument(span.clone()).await;
    let counted = result.as_ref().ok().map(rows);
    match counted {
        Some(rows) => span.record("db.response.returned_rows", rows),
        None => span.record("otel.status_code", "ERROR"),
    };
    if let (Some((callback, statement)), Some(start)) = (observer, start) {
        callback(&Info {
            elapsed: start.elapsed(),
            statement,
            error: result.as_ref().err(),
            rows: counted,
        });
    }
    result
}

/// The span a statement runs in, under target `tideorm::query` at `DEBUG`,
/// with OpenTelemetry's database client fields: the server's
/// `db.system.name`, the statement's `db.query.text` (with markers for its
/// bound values; `None` for unprepared SQL, whose text holds them), then
/// `db.response.returned_rows` or an `ERROR` `otel.status_code`.
#[cfg_attr(
    not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
    allow(dead_code)
)]
pub(crate) fn span(server: crate::DbServer, sql: Option<&str>) -> Span {
    tracing::debug_span!(
        target: "tideorm::query",
        "db.query",
        otel.kind = "client",
        db.system.name = server.system_name(),
        db.query.text = sql,
        db.response.returned_rows = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    )
}
