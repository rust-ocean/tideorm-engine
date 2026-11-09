use std::future::Future;

use driver::{Executor, pool::PoolConnection};
use driver_core::transaction::{TransactionManager, TransactionState};

use super::{DriverPoolConnection, InnerConnection, common::*};
use crate::{
    DatabaseConnection, DbBackend, DbErr, ExecResult, QueryResult, Statement, TransactionOptions,
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

    /// Whether `connection` reaches MariaDB, as the server said when it
    /// opened: only a MySQL connection can.
    fn is_mariadb(_connection: &PoolConnection<Self>) -> bool {
        false
    }

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
    let query = driver_query_with(&statement.sql, DB::arguments(statement));
    metric::observed(
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
    let observer = metric.zip(statement.as_ref());
    metric::observed(
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
    let query = driver_query_with(&statement.sql, DB::arguments(statement));
    metric::observed(
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
    let query = driver_query_with(&statement.sql, DB::arguments(statement));
    metric::observed(
        metric.zip(Some(statement)),
        async {
            query
                .fetch_all(connection.executor())
                .await
                .map(|rows| rows.into_iter().map(DB::query_result).collect())
                .map_err(driver_error_to_query_err)
        },
        |rows: &Vec<QueryResult>| rows.len() as u64,
    )
    .await
}

/// Commits the innermost transaction or savepoint on `connection`.
pub(crate) async fn commit<DB: Driver>(connection: &mut PoolConnection<DB>) -> Result<(), DbErr> {
    DB::TransactionManager::commit(connection)
        .await
        .map_err(driver_error_to_query_err)
}

/// Rolls back the innermost transaction or savepoint on `connection`.
pub(crate) async fn rollback<DB: Driver>(connection: &mut PoolConnection<DB>) -> Result<(), DbErr> {
    DB::TransactionManager::rollback(connection)
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
