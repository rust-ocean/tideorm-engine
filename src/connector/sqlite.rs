use std::path::{Path, PathBuf};
use std::sync::Arc;

use driver::{
    Sqlite, SqlitePool,
    pool::PoolConnection,
    sqlite::{SqliteConnectOptions, SqliteQueryResult, SqliteRow},
};
use driver_core::sql_str::SqlSafeStr;
use driver_core::transaction::TransactionManager;
use tracing::{instrument, warn};

use crate::{
    AccessMode, ConnectOptions, DatabaseConnection, DatabaseConnectionType, DbBackend, Statement,
    TransactionOptions, error::*, executor::*,
};

use super::{Driver, DriverPoolConnection, InnerConnection, common::*};

/// Defines the [driver::sqlite] connector
#[derive(Debug)]
pub struct SqliteConnector;

/// A SQLite pool connection
pub type SqlitePoolConnection = DriverPoolConnection<Sqlite>;

impl SqliteConnector {
    /// Opens a pool on `connect` with `options`' pool settings, sized for
    /// the database `connect` names.
    #[instrument(level = "trace", skip_all)]
    pub(crate) async fn connect(
        mut options: ConnectOptions,
        mut connect: SqliteConnectOptions,
    ) -> Result<DatabaseConnection, DbErr> {
        if let Some(capacity) = options.statement_cache_capacity {
            connect = connect.statement_cache_capacity(capacity);
        }
        size_pool(&connect, &mut options)?;
        let pool = open_pool(&options, connect).await?;
        Ok(Sqlite::into_connection(pool))
    }
}

/// A transient database lives only while a connection to it is open, so its
/// pool keeps one open and closes none for being idle or old. A database
/// private to each connection takes a pool of one: a second connection would
/// open an empty database of its own.
fn size_pool(connect: &SqliteConnectOptions, options: &mut ConnectOptions) -> Result<(), DbErr> {
    if connect.is_connection_private() {
        if options.get_max_connections().is_some_and(|max| max > 1)
            || options.get_min_connections().is_some_and(|min| min > 1)
        {
            return Err(private_database("requires a pool size of one"));
        }
        options.max_connections(1);
    }
    if connect.is_transient() {
        options.min_connections(options.get_min_connections().unwrap_or(0).max(1));
        options.idle_timeout(None).max_lifetime(None);
    }
    Ok(())
}

/// The configuration error for a database private to its connection, which
/// `needs` what only a second connection to it could give.
fn private_database(needs: &str) -> DbErr {
    DbErr::Conn(RuntimeErr::DriverError(Arc::new(
        driver::Error::Configuration(
            format!(
                "an in-memory SQLite database with cache=private, or a temporary one, is \
                 private to its connection and {needs}"
            )
            .into(),
        ),
    )))
}

impl DatabaseConnection {
    /// Where SQLite opened the main database file (an absolute path), as a
    /// connection of the pool recorded when it opened; `None` for an
    /// in-memory or temporary database, or another backend. Takes a
    /// connection from the pool.
    pub async fn sqlite_main_file(&self) -> Result<Option<PathBuf>, DbErr> {
        let Some(pool) = self.get_sqlite_connection_pool() else {
            return Ok(None);
        };
        let connection = pool.acquire().await.map_err(driver_conn_acquire_err)?;
        Ok(connection.main_file().map(Path::to_path_buf))
    }
}

impl SqliteConnector {
    /// Wrap a driver pool in a [DatabaseConnection]
    pub fn from_sqlite_pool(pool: SqlitePool) -> DatabaseConnection {
        pool.into()
    }
}

impl SqlitePoolConnection {
    /// Refuse a session on a database private to each connection, where the
    /// session's connection would open an empty database of its own.
    pub(crate) fn ensure_sessions_share_the_database(&self) -> Result<(), DbErr> {
        if self.pool.connect_options().is_connection_private() {
            return Err(private_database(
                "cannot pin a session: its connection would open an empty database of its own",
            ));
        }
        Ok(())
    }
}

impl Driver for Sqlite {
    const BACKEND: DbBackend = DbBackend::SQLite;
    const POOL_NAME: &'static str = "SqlitePoolConnection";

    fn arguments(statement: &Statement) -> Result<Self::Arguments, driver::Error> {
        statement.try_sqlite_arguments()
    }

    fn exec_result(result: SqliteQueryResult) -> ExecResult {
        result.into()
    }

    fn query_result(row: SqliteRow) -> QueryResult {
        row.into()
    }

    fn into_inner(connection: PoolConnection<Sqlite>) -> InnerConnection {
        InnerConnection::Sqlite(connection)
    }

    fn into_connection(pool: SqlitePoolConnection) -> DatabaseConnection {
        DatabaseConnectionType::SqlitePoolConnection(pool).into()
    }

    /// SQLite has no isolation level to set (its transactions are
    /// serializable) and cannot make a transaction read-only, so
    /// [`AccessMode::ReadOnly`] is refused. A transaction begins in `options`'
    /// SQLite mode.
    async fn begin(
        connection: &mut PoolConnection<Sqlite>,
        options: TransactionOptions,
    ) -> Result<(), DbErr> {
        if options.isolation_level.is_some() {
            warn!("Setting isolation level in a SQLite transaction isn't supported");
        }
        if options.access_mode == Some(AccessMode::ReadOnly) {
            return Err(DbErr::BackendNotSupported {
                db: "SQLite",
                ctx: "read-only transactions",
            });
        }
        let statement = options.sqlite_transaction_mode.map(|mode| {
            driver::AssertSqlSafe(format!("BEGIN {}", mode.sqlite_keyword())).into_sql_str()
        });
        <Sqlite as driver::Database>::TransactionManager::begin(connection, statement)
            .await
            .map_err(driver_error_to_query_err)
    }
}

impl From<SqliteRow> for QueryResult {
    fn from(row: SqliteRow) -> QueryResult {
        QueryResult {
            row: QueryResultRow::Sqlite(row),
        }
    }
}

impl From<SqliteQueryResult> for ExecResult {
    fn from(result: SqliteQueryResult) -> ExecResult {
        ExecResult {
            result: ExecResultHolder::Sqlite(result),
        }
    }
}
