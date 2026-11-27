use std::future::Future;

use driver::{Executor, pool::PoolConnection};
use driver_core::transaction::{TransactionManager, TransactionState};
use futures_util::TryStreamExt;

use super::{DriverPoolConnection, InnerConnection, common::*};
use crate::{
    DatabaseConnection, DbBackend, DbErr, DbServer, ExecResult, QueryResult, Statement,
    TransactionOptions,
    metric::{self, Callback},
};

/// One database driver as the engine uses it: how a statement binds its values,
/// how results come back, and how a transaction begins. SQLite, PostgreSQL
/// and MySQL implement it; running statements and ending transactions is
/// written once, below, for all of them.
pub trait Driver:
    driver::Database<Connection: RunsStatements<Self>, Arguments: driver::IntoArguments<Self>>
{
    /// The dialect this driver's statements are rendered in.
    const BACKEND: DbBackend;

    /// The name the driver's pool connection prints in `Debug` output.
    const POOL_NAME: &'static str;

    /// `statement`'s values, encoded by the engine's binder.
    fn arguments(statement: &Statement) -> Result<Self::Arguments, driver::Error>;

    /// The server `connection` reaches, as it said when it opened.
    fn server(connection: &PoolConnection<Self>) -> DbServer;

    /// The engine's view of a statement's outcome.
    fn exec_result(result: Self::QueryResult) -> ExecResult;

    /// The engine's view of a row.
    fn query_result(row: Self::Row) -> QueryResult;

    /// `connection` as a transaction holds it.
    fn into_inner(connection: PoolConnection<Self>) -> InnerConnection;

    /// `pool` as a [`DatabaseConnection`].
    fn into_connection(pool: DriverPoolConnection<Self>) -> DatabaseConnection;

    /// Begins a transaction on `connection`, or a savepoint when one is open,
    /// with as much of `options` as the backend supports.
    fn begin(
        connection: &mut PoolConnection<Self>,
        options: TransactionOptions,
    ) -> impl Future<Output = Result<(), DbErr>> + Send;
}

/// A connection that lends a mutable executor for each statement, so the
/// generic code below need not repeat that higher-ranked bound.
pub trait RunsStatements<DB: driver::Database> {
    /// Borrow this connection as an executor.
    fn executor(&mut self) -> impl Executor<'_, Database = DB>;
}

impl<DB, C> RunsStatements<DB> for C
where
    DB: driver::Database,
    for<'c> &'c mut C: Executor<'c, Database = DB>,
{
    fn executor(&mut self) -> impl Executor<'_, Database = DB> {
        self
    }
}

/// Runs `statement`, reporting it to `metric`.
pub(crate) async fn execute<DB: Driver>(
    connection: &mut PoolConnection<DB>,
    statement: &Statement,
    metric: Option<&Callback>,
) -> Result<ExecResult, DbErr> {
    let span = metric::span(DB::server(connection), Some(&statement.sql));
    let query = driver_query_with(&statement.sql, DB::arguments(statement));
    metric::observed(
        span,
        metric.zip(Some(statement)),
        async {
            query
                .execute(connection.executor())
                .await
                .map(DB::exec_result)
                .map_err(driver_error_to_exec_err)
        },
        ExecResult::rows_affected,
    )
    .await
}

/// Runs `sql` as it is, without preparing it, reporting it to `metric`.
pub(crate) async fn execute_unprepared<DB: Driver>(
    connection: &mut PoolConnection<DB>,
    sql: &str,
    metric: Option<&Callback>,
) -> Result<ExecResult, DbErr> {
    let statement = metric.map(|_| Statement::from_string(DB::BACKEND, sql));
    let span = metric::span(DB::server(connection), None);
    let observer = metric.zip(statement.as_ref());
    metric::observed(
        span,
        observer,
        async {
            connection
                .executor()
                .execute(driver::AssertSqlSafe(sql.to_owned()))
                .await
                .map(DB::exec_result)
                .map_err(driver_error_to_exec_err)
        },
        ExecResult::rows_affected,
    )
    .await
}

/// The first row `statement` returns, if any, reporting it to `metric`.
pub(crate) async fn query_one<DB: Driver>(
    connection: &mut PoolConnection<DB>,
    statement: &Statement,
    metric: Option<&Callback>,
) -> Result<Option<QueryResult>, DbErr> {
    let span = metric::span(DB::server(connection), Some(&statement.sql));
    let query = driver_query_with(&statement.sql, DB::arguments(statement));
    metric::observed(
        span,
        metric.zip(Some(statement)),
        async {
            query
                .fetch_optional(connection.executor())
                .await
                .map(|row| row.map(DB::query_result))
                .map_err(driver_error_to_query_err)
        },
        |row| u64::from(row.is_some()),
    )
    .await
}

/// Every row `statement` returns, reporting it to `metric`.
pub(crate) async fn query_all<DB: Driver>(
    connection: &mut PoolConnection<DB>,
    statement: &Statement,
    metric: Option<&Callback>,
) -> Result<Vec<QueryResult>, DbErr> {
    let span = metric::span(DB::server(connection), Some(&statement.sql));
    let query = driver_query_with(&statement.sql, DB::arguments(statement));
    metric::observed(
        span,
        metric.zip(Some(statement)),
        async {
            // Mapped as they stream in: one vector of rows, not two.
            query
                .fetch(connection.executor())
                .map_ok(DB::query_result)
                .try_collect()
                .await
                .map_err(driver_error_to_query_err)
        },
        |rows: &Vec<QueryResult>| rows.len() as u64,
    )
    .await
}

/// Begins a transaction on `connection`, or a savepoint inside the one open.
pub(crate) async fn begin<DB: Driver>(
    connection: &mut PoolConnection<DB>,
    options: TransactionOptions,
) -> Result<(), DbErr> {
    let operation = match transaction_depth(connection) {
        0 => "BEGIN",
        _ => "SAVEPOINT",
    };
    let span = metric::control_span(DB::server(connection), operation);
    metric::traced(span, <DB as Driver>::begin(connection, options)).await
}

/// Commits the innermost transaction or savepoint on `connection`, or rolls it back.
pub(crate) async fn end<DB: Driver>(
    connection: &mut PoolConnection<DB>,
    commit: bool,
) -> Result<(), DbErr> {
    let operation = match (commit, transaction_depth(connection) > 1) {
        (true, false) => "COMMIT",
        (true, true) => "RELEASE SAVEPOINT",
        (false, false) => "ROLLBACK",
        (false, true) => "ROLLBACK TO SAVEPOINT",
    };
    let span = metric::control_span(DB::server(connection), operation);
    let work = async {
        if commit {
            DB::TransactionManager::commit(connection).await
        } else {
            DB::TransactionManager::rollback(connection).await
        }
    };
    metric::traced(span, work)
        .await
        .map_err(driver_error_to_query_err)
}

/// Queues a rollback of the innermost transaction or savepoint, which the
/// driver sends before the connection's next use.
pub(crate) fn start_rollback<DB: Driver>(connection: &mut PoolConnection<DB>) {
    DB::TransactionManager::start_rollback(connection);
}

/// How deep in transactions `connection` is: 0 outside one, 1 in a
/// transaction, more inside savepoints.
pub(crate) fn transaction_depth<DB: Driver>(connection: &PoolConnection<DB>) -> usize {
    DB::TransactionManager::get_transaction_depth(connection)
}

/// Whether an owned transaction is no longer present on the server.
pub(crate) fn transaction_ended<DB: Driver>(connection: &PoolConnection<DB>) -> bool {
    transaction_depth(connection) > 0
        && matches!(
            DB::TransactionManager::get_transaction_state(connection),
            TransactionState::Idle | TransactionState::Aborted
        )
}

/// Whether a failure abandoned the whole transaction open on `connection`:
/// SQLite rolled it back by itself, or a statement deadlocked.
pub(crate) fn transaction_aborted<DB: Driver>(connection: &PoolConnection<DB>) -> bool {
    DB::TransactionManager::get_transaction_state(connection) == TransactionState::Aborted
}

/// Whether the server reports the transaction on `connection` failed, once the
/// replies still pending on it are read.
pub(crate) async fn transaction_failed<DB: Driver>(
    connection: &mut PoolConnection<DB>,
) -> Result<bool, DbErr> {
    driver::Connection::flush(&mut **connection)
        .await
        .map_err(driver_error_to_query_err)?;
    Ok(DB::TransactionManager::get_transaction_state(connection) == TransactionState::Failed)
}

/// Whether no transaction is open on `connection`, by the driver's count and
/// by the server, once the replies and rollbacks still pending on it are
/// through.
pub(crate) async fn transaction_idle<DB: Driver>(
    connection: &mut PoolConnection<DB>,
) -> Result<bool, DbErr> {
    driver::Connection::flush(&mut **connection)
        .await
        .map_err(driver_error_to_query_err)?;
    Ok(transaction_depth(connection) == 0
        && DB::TransactionManager::get_transaction_state(connection) == TransactionState::Idle)
}
