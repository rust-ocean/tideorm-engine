use driver::{
    PgPool, Postgres,
    pool::PoolConnection,
    postgres::{PgConnectOptions, PgQueryResult, PgRow},
};
use driver_core::sql_str::SqlSafeStr;
use driver_core::transaction::TransactionManager;
use tracing::instrument;

use crate::{
    ConnectOptions, DatabaseConnection, DatabaseConnectionType, DbBackend, Statement,
    TransactionOptions, error::*, executor::*,
};

use super::{Driver, DriverPoolConnection, InnerConnection, common::*};

/// Defines the [driver::postgres] connector
#[derive(Debug)]
pub struct PostgresConnector;

/// A PostgreSQL pool connection
pub type PostgresPoolConnection = DriverPoolConnection<Postgres>;

impl PostgresConnector {
    /// Opens a pool on `connect` with `options`' pool settings.
    #[instrument(level = "trace", skip_all)]
    pub(crate) async fn connect(
        options: ConnectOptions,
        mut connect: PgConnectOptions,
    ) -> Result<DatabaseConnection, DbErr> {
        if let Some(capacity) = options.statement_cache_capacity {
            connect = connect.statement_cache_capacity(capacity);
        }
        let pool = open_pool(&options, connect).await?;
        Ok(Postgres::into_connection(pool))
    }

    /// Wrap a driver pool in a [DatabaseConnection]
    pub fn from_postgres_pool(pool: PgPool) -> DatabaseConnection {
        pool.into()
    }
}

impl Driver for Postgres {
    const BACKEND: DbBackend = DbBackend::Postgres;
    const POOL_NAME: &'static str = "PostgresPoolConnection";

    fn arguments(statement: &Statement) -> Result<Self::Arguments, driver::Error> {
        statement.try_postgres_arguments()
    }

    fn exec_result(result: PgQueryResult) -> ExecResult {
        result.into()
    }

    fn query_result(row: PgRow) -> QueryResult {
        row.into()
    }

    fn into_inner(connection: PoolConnection<Postgres>) -> InnerConnection {
        InnerConnection::Postgres(connection)
    }

    fn into_connection(pool: PostgresPoolConnection) -> DatabaseConnection {
        DatabaseConnectionType::PostgresPoolConnection(pool).into()
    }

    /// PostgreSQL takes a transaction's isolation level and access mode in its
    /// `BEGIN`.
    async fn begin(
        connection: &mut PoolConnection<Postgres>,
        options: TransactionOptions,
    ) -> Result<(), DbErr> {
        let modes: Vec<String> = options
            .isolation_level
            .map(|level| format!("ISOLATION LEVEL {level}"))
            .into_iter()
            .chain(options.access_mode.map(|mode| mode.to_string()))
            .collect();
        let statement = (!modes.is_empty())
            .then(|| driver::AssertSqlSafe(format!("BEGIN {}", modes.join(", "))).into_sql_str());
        <Postgres as driver::Database>::TransactionManager::begin(connection, statement)
            .await
            .map_err(driver_error_to_query_err)
    }
}

impl From<PgRow> for QueryResult {
    fn from(row: PgRow) -> QueryResult {
        QueryResult {
            row: QueryResultRow::Postgres(row),
        }
    }
}

impl From<PgQueryResult> for ExecResult {
    fn from(result: PgQueryResult) -> ExecResult {
        ExecResult {
            result: ExecResultHolder::Postgres(result),
        }
    }
}
