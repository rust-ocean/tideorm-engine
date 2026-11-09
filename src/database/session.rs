//! A connection of its own to a pool's database, held outside the pool.

use std::sync::Arc;

use driver::Connection;
use futures_util::lock::Mutex;

use crate::{
    DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, ExecResult, QueryResult, Statement,
    TransactionOptions,
    connector::{InnerConnection, backend, driver_error_to_conn_err, on_connection},
    database::SharedConnection,
    metric::Callback,
};

/// One connection of its own, opened by
/// [`DatabaseConnection::pin_session`] with its pool's settings and held
/// until [`close`](DatabaseConnection::close) or until the last clone drops,
/// when it closes. What a statement sets for its session (a `SET`, a
/// `PRAGMA`, a `GET_LOCK`, an advisory lock) holds for every later statement
/// on it, which a pool, handing each statement whichever connection is free,
/// cannot promise. It never reconnects: once the connection is lost, its
/// statements fail.
#[derive(Clone)]
pub struct SessionConnection {
    conn: Arc<SharedConnection>,
    backend: DbBackend,
    metric_callback: Option<Callback>,
    /// The pool it was opened from, which opens the sessions asked of it.
    origin: Arc<DatabaseConnection>,
    /// Held by the transaction begun on the session until it ends; see
    /// [`begin_with`](Self::begin_with).
    transactions: Arc<Mutex<()>>,
}

impl std::fmt::Debug for SessionConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SessionConnection {{ backend: {:?} }}", self.backend)
    }
}

impl From<SessionConnection> for DatabaseConnection {
    fn from(session: SessionConnection) -> Self {
        crate::DatabaseConnectionType::Session(session).into()
    }
}

impl SessionConnection {
    pub(crate) fn new(
        connection: InnerConnection,
        backend: DbBackend,
        metric_callback: Option<Callback>,
        origin: DatabaseConnection,
        identity: u64,
    ) -> Self {
        Self {
            conn: Arc::new(SharedConnection::new(connection, identity)),
            backend,
            metric_callback,
            origin: Arc::new(origin),
            transactions: Arc::default(),
        }
    }

    pub(crate) fn backend(&self) -> DbBackend {
        if self.conn.is_mariadb() {
            DbBackend::MariaDB
        } else {
            self.backend
        }
    }

    /// The pool the session was opened from, always a pool, never a session.
    pub(crate) fn origin(&self) -> &DatabaseConnection {
        &self.origin
    }

    pub(crate) fn identity(&self) -> u64 {
        self.conn.identity()
    }

    pub(crate) async fn execute(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        let connection = &mut *self.conn.lock().await;
        self.conn.ensure_settled(connection).await?;
        on_connection!(connection, connection => {
            backend::execute(connection, &stmt, self.metric_callback.as_ref()).await
        })
    }

    pub(crate) async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        let connection = &mut *self.conn.lock().await;
        self.conn.ensure_settled(connection).await?;
        on_connection!(connection, connection => {
            backend::execute_unprepared(connection, sql, self.metric_callback.as_ref()).await
        })
    }

    pub(crate) async fn query_one(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        let connection = &mut *self.conn.lock().await;
        self.conn.ensure_settled(connection).await?;
        on_connection!(connection, connection => {
            backend::query_one(connection, &stmt, self.metric_callback.as_ref()).await
        })
    }

    pub(crate) async fn query_all(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        let connection = &mut *self.conn.lock().await;
        self.conn.ensure_settled(connection).await?;
        on_connection!(connection, connection => {
            backend::query_all(connection, &stmt, self.metric_callback.as_ref()).await
        })
    }

    /// Begin a transaction on the session's connection once the one begun
    /// there before has ended. Begun beside it, it would open as a savepoint
    /// in it, which one's end would then end in the other's place; a
    /// savepoint is begun on the transaction instead.
    pub(crate) async fn begin_with(
        &self,
        options: TransactionOptions,
    ) -> Result<DatabaseTransaction, DbErr> {
        let slot = Arc::clone(&self.transactions).lock_owned().await;
        DatabaseTransaction::begin(
            Arc::clone(&self.conn),
            self.backend,
            self.metric_callback.clone(),
            options,
            Some(slot),
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

    /// Checks that the session's connection is still alive.
    pub(crate) async fn ping(&self) -> Result<(), DbErr> {
        let connection = &mut *self.conn.lock().await;
        on_connection!(connection, connection => {
            connection.ping().await.map_err(driver_error_to_conn_err)
        })
    }

    /// How many statements the driver keeps prepared on the session's
    /// connection.
    pub(crate) async fn cached_statements(&self) -> Result<usize, DbErr> {
        let connection = &*self.conn.lock().await;
        on_connection!(connection, connection => Ok(connection.cached_statements_size()))
    }

    /// Close the session's connection, once whatever runs on it is done.
    pub(crate) async fn close_by_ref(&self) -> Result<(), DbErr> {
        self.conn.close().await
    }
}
