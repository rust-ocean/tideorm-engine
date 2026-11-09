use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

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

/// Runs `work` and, when there is an observer, reports to its callback how long
/// the statement took (on the monotonic clock), and its error or the rows
/// `rows` counts in its result.
#[cfg_attr(
    not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
    allow(dead_code)
)]
pub(crate) async fn observed<T>(
    observer: Option<(&Callback, &crate::Statement)>,
    work: impl Future<Output = Result<T, DbErr>>,
    rows: impl FnOnce(&T) -> u64,
) -> Result<T, DbErr> {
    let Some((callback, statement)) = observer else {
        return work.await;
    };
    let start = Instant::now();
    let result = work.await;
    callback(&Info {
        elapsed: start.elapsed(),
        statement,
        error: result.as_ref().err(),
        rows: result.as_ref().ok().map(rows),
    });
    result
}
