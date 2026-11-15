use std::sync::{Arc, OnceLock};

use driver::{Connection, pool::PoolConnection};
use tracing::instrument;

use super::{Driver, backend, common::*};
use crate::{
    DatabaseConnection, DatabaseTransaction, DbErr, DbServer, ExecResult, QueryResult,
    SessionConnection, Statement, TransactionOptions, metric::Callback,
};

/// A driver connection pool the engine runs statements on, reporting each to
/// the connection's metric callback. Named per driver:
/// [`SqlitePoolConnection`](crate::SqlitePoolConnection),
/// [`PostgresPoolConnection`](crate::PostgresPoolConnection) and
/// [`MySqlPoolConnection`](crate::MySqlPoolConnection).
pub struct DriverPoolConnection<DB: Driver> {
    pub(crate) pool: driver::Pool<DB>,
    metric_callback: Option<Callback>,
    /// The server the first connection taken from the pool said it reaches.
    server: Arc<OnceLock<DbServer>>,
    /// See [`DatabaseConnection::identity`].
    identity: u64,
}

impl<DB: Driver> Clone for DriverPoolConnection<DB> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            metric_callback: self.metric_callback.clone(),
            server: self.server.clone(),
            identity: self.identity,
        }
    }
}

impl<DB: Driver> std::fmt::Debug for DriverPoolConnection<DB> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {{ pool: {:?} }}", DB::POOL_NAME, self.pool)
    }
}

impl<DB: Driver> From<driver::Pool<DB>> for DriverPoolConnection<DB> {
    fn from(pool: driver::Pool<DB>) -> Self {
        Self {
            pool,
            metric_callback: None,
            // SQLite is only ever itself.
            server: Arc::new(if DB::BACKEND == crate::DbBackend::SQLite {
                OnceLock::from(DbServer::Sqlite)
            } else {
                OnceLock::new()
            }),
            identity: crate::database::new_identity(),
        }
    }
}

impl<DB: Driver> From<driver::Pool<DB>> for DatabaseConnection {
    fn from(pool: driver::Pool<DB>) -> Self {
        DB::into_connection(pool.into())
    }
}

impl<DB: Driver> DriverPoolConnection<DB> {
    pub(crate) fn backend(&self) -> crate::DbBackend {
        self.server().map_or(DB::BACKEND, DbServer::backend)
    }

    pub(super) async fn acquire(&self) -> Result<PoolConnection<DB>, DbErr> {
        let connection = self.pool.acquire().await.map_err(driver_conn_acquire_err)?;
        self.server.get_or_init(|| DB::server(&connection));
        Ok(connection)
    }

    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    /// The server the pool reaches, as its connections said when they
    /// opened; `None` until one was taken from it, but for SQLite.
    pub(crate) fn server(&self) -> Option<DbServer> {
        self.server.get().copied()
    }

    /// Execute a [Statement]
    #[instrument(level = "trace", skip(stmt))]
    pub async fn execute(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        let mut connection = self.acquire().await?;
        backend::execute(&mut connection, &stmt, self.metric_callback.as_ref()).await
    }

    /// Execute an unprepared SQL statement
    #[instrument(level = "trace", skip(sql))]
    pub async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        let mut connection = self.acquire().await?;
        backend::execute_unprepared(&mut connection, sql, self.metric_callback.as_ref()).await
    }

    /// Get one result from a SQL query. Returns [Option::None] if no match was found
    #[instrument(level = "trace", skip(stmt))]
    pub async fn query_one(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        let mut connection = self.acquire().await?;
        backend::query_one(&mut connection, &stmt, self.metric_callback.as_ref()).await
    }

    /// Get the results of a query returning them as a Vec<[QueryResult]>
    #[instrument(level = "trace", skip(stmt))]
    pub async fn query_all(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        let mut connection = self.acquire().await?;
        backend::query_all(&mut connection, &stmt, self.metric_callback.as_ref()).await
    }

    /// A session on the pool's database, with `identity`: a connection of its
    /// own, opened with the pool's settings and hooks but outside it, so it
    /// takes none of the pool's connections, and closed when the session
    /// drops.
    pub(crate) async fn pin_session(&self, identity: u64) -> Result<SessionConnection, DbErr> {
        let session = self
            .pool
            .options()
            .clone()
            .max_connections(1)
            .min_connections(0)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_lazy_with((*self.pool.connect_options()).clone());
        let mut connection = session.acquire().await.map_err(driver_conn_acquire_err)?;
        self.server.get_or_init(|| DB::server(&connection));
        connection.close_on_drop();
        Ok(SessionConnection::new(
            DB::into_inner(connection),
            DB::BACKEND,
            self.metric_callback.clone(),
            DB::into_connection(self.clone()),
            identity,
        ))
    }

    /// Begin a transaction on a connection of the pool, with as much of
    /// `options` as the backend supports.
    pub(crate) async fn begin_with(
        &self,
        options: TransactionOptions,
    ) -> Result<DatabaseTransaction, DbErr> {
        let connection = self.acquire().await?;
        DatabaseTransaction::begin(
            Arc::new(crate::database::SharedConnection::new(
                DB::into_inner(connection),
                self.identity,
            )),
            DB::BACKEND,
            self.metric_callback.clone(),
            options,
            None,
            None,
        )
        .await
    }

    pub(crate) fn set_metric_callback<F>(&mut self, callback: F)
    where
        F: Fn(&crate::metric::Info<'_>) + Send + Sync + 'static,
    {
        self.metric_callback = Some(Arc::new(callback));
    }

    /// Checks if a connection to the database is still valid.
    pub async fn ping(&self) -> Result<(), DbErr> {
        let mut connection = self.acquire().await?;
        connection.ping().await.map_err(driver_error_to_conn_err)
    }

    /// How many statements the driver keeps prepared on the connection the
    /// pool hands out next; each connection has a cache of its own.
    pub async fn cached_statements(&self) -> Result<usize, DbErr>
    where
        DB: driver::database::HasStatementCache,
    {
        Ok(self.acquire().await?.cached_statements_size())
    }

    /// Explicitly close the connection pool.
    /// See [`Self::close_by_ref`] for usage with references.
    pub async fn close(self) -> Result<(), DbErr> {
        self.close_by_ref().await
    }

    /// Explicitly close the connection pool
    pub async fn close_by_ref(&self) -> Result<(), DbErr> {
        self.pool.close().await;
        Ok(())
    }
}
