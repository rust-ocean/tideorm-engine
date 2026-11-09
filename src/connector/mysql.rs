use driver::{
    MySql, MySqlPool,
    mysql::{MySqlConnectOptions, MySqlQueryResult, MySqlRow},
    pool::PoolConnection,
};
use driver_core::sql_str::SqlSafeStr;
use driver_core::transaction::TransactionManager;
use std::fmt::Write as _;
use tracing::instrument;

use crate::{
    ConnectOptions, DatabaseConnection, DatabaseConnectionType, DbBackend, Statement,
    TransactionOptions, error::*, executor::*,
};

use super::{Driver, DriverPoolConnection, InnerConnection, common::*};

/// Defines the [driver::mysql] connector
#[derive(Debug)]
pub struct MySqlConnector;

/// A MySQL pool connection
pub type MySqlPoolConnection = DriverPoolConnection<MySql>;

impl MySqlConnector {
    /// Opens a pool on `connect` with `options`' pool settings and, unless
    /// it is lazy, learns from a connection whether the server is MariaDB.
    #[instrument(level = "trace", skip_all)]
    pub(crate) async fn connect(
        options: ConnectOptions,
        mut connect: MySqlConnectOptions,
    ) -> Result<DatabaseConnection, DbErr> {
        if let Some(capacity) = options.statement_cache_capacity {
            connect = connect.statement_cache_capacity(capacity);
        }
        let pool = open_pool(&options, connect).await?;
        if !options.connect_lazy {
            pool.identify_server().await?;
        }
        Ok(MySql::into_connection(pool))
    }

    /// Wrap a driver pool in a [DatabaseConnection]
    pub fn from_mysql_pool(pool: MySqlPool) -> DatabaseConnection {
        pool.into()
    }
}

impl Driver for MySql {
    const BACKEND: DbBackend = DbBackend::MySQL;
    const POOL_NAME: &'static str = "MySqlPoolConnection";

    fn arguments(statement: &Statement) -> Result<Self::Arguments, driver::Error> {
        statement.try_mysql_arguments()
    }

    fn is_mariadb(connection: &PoolConnection<MySql>) -> bool {
        connection.is_mariadb()
    }

    fn exec_result(result: MySqlQueryResult) -> ExecResult {
        result.into()
    }

    fn query_result(row: MySqlRow) -> QueryResult {
        row.into()
    }

    fn into_inner(connection: PoolConnection<MySql>) -> InnerConnection {
        InnerConnection::MySql(connection)
    }

    fn into_connection(pool: MySqlPoolConnection) -> DatabaseConnection {
        DatabaseConnectionType::MySqlPoolConnection(pool).into()
    }

    /// MySQL takes a transaction's access mode in its `START TRANSACTION`, and
    /// its isolation level from a `SET TRANSACTION` sent with it in one
    /// request. The server runs the two in order and stops at a failure, so a
    /// refused level never carries over to the connection's next transaction.
    async fn begin(
        connection: &mut PoolConnection<MySql>,
        options: TransactionOptions,
    ) -> Result<(), DbErr> {
        let statement =
            (options.isolation_level.is_some() || options.access_mode.is_some()).then(|| {
                let mut sql = options
                    .isolation_level
                    .map(|level| format!("SET TRANSACTION ISOLATION LEVEL {level}; "))
                    .unwrap_or_default();
                sql.push_str("START TRANSACTION");
                if let Some(mode) = options.access_mode {
                    let _ = write!(sql, " {mode}");
                }
                driver::AssertSqlSafe(sql).into_sql_str()
            });
        <MySql as driver::Database>::TransactionManager::begin(connection, statement)
            .await
            .map_err(driver_error_to_query_err)
    }
}

impl From<MySqlRow> for QueryResult {
    fn from(row: MySqlRow) -> QueryResult {
        QueryResult {
            row: QueryResultRow::MySql(row),
        }
    }
}

impl From<MySqlQueryResult> for ExecResult {
    fn from(result: MySqlQueryResult) -> ExecResult {
        ExecResult {
            result: ExecResultHolder::MySql(result),
        }
    }
}
