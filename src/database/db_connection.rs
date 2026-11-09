use crate::{
    ConnectionTrait, DatabaseTransaction, DbBackend, ExecResult, QueryResult, Statement,
    TransactionOptions, TransactionTrait, error::*,
};
use std::fmt::Debug;
use tracing::instrument;

/// A handle to a database — implements [`ConnectionTrait`](crate::ConnectionTrait)
/// and [`TransactionTrait`](crate::TransactionTrait) so it works with every
/// query and mutation method in the engine.
///
/// Behind the scenes this is a driver connection pool, or a session
/// ([`pin_session`](Self::pin_session)), so it is cheap to clone — pass
/// `&DbConn` around or `db.clone()` into spawned tasks. Obtain one via
/// [`Database::connect`](crate::Database::connect).
#[derive(Debug, Clone)]
pub struct DatabaseConnection {
    /// The driver's connection pool, or a session.
    pub inner: DatabaseConnectionType,
}

/// The driver-specific connection or pool wrapped by [`DatabaseConnection`].
///
/// Which variants are available depends on enabled feature flags. End users
/// rarely match on this directly; use [`DatabaseConnection`]'s methods
/// instead.
#[derive(Clone)]
pub enum DatabaseConnectionType {
    /// MySQL connection pool (`tideorm-driver-mysql`).
    #[cfg(feature = "mysql")]
    MySqlPoolConnection(crate::MySqlPoolConnection),

    /// PostgreSQL connection pool (`tideorm-driver-postgres`).
    #[cfg(feature = "postgres")]
    PostgresPoolConnection(crate::PostgresPoolConnection),

    /// SQLite connection pool (`tideorm-driver-sqlite`).
    #[cfg(feature = "sqlite")]
    SqlitePoolConnection(crate::SqlitePoolConnection),

    /// One connection of its own, from
    /// [`DatabaseConnection::pin_session`].
    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    Session(crate::SessionConnection),
}

/// Short alias for [`DatabaseConnection`].
pub type DbConn = DatabaseConnection;

impl From<DatabaseConnectionType> for DatabaseConnection {
    fn from(inner: DatabaseConnectionType) -> Self {
        Self { inner }
    }
}

impl Debug for DatabaseConnectionType {
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(unreachable_code)
    )]
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            #[cfg(feature = "mysql")]
            Self::MySqlPoolConnection(_) => "MySqlPoolConnection",
            #[cfg(feature = "postgres")]
            Self::PostgresPoolConnection(_) => "PostgresPoolConnection",
            #[cfg(feature = "sqlite")]
            Self::SqlitePoolConnection(_) => "SqlitePoolConnection",
            #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
            Self::Session(_) => "Session",
            #[cfg(not(any(feature = "postgres", feature = "mysql", feature = "sqlite")))]
            empty => match *empty {},
        })
    }
}

/// Evaluates `$body` with `$pool` bound to the connection's driver pool or
/// session, which answer the same methods.
macro_rules! on_pool {
    ($inner:expr, $pool:ident => $body:expr) => {
        match $inner {
            #[cfg(feature = "mysql")]
            DatabaseConnectionType::MySqlPoolConnection($pool) => $body,
            #[cfg(feature = "postgres")]
            DatabaseConnectionType::PostgresPoolConnection($pool) => $body,
            #[cfg(feature = "sqlite")]
            DatabaseConnectionType::SqlitePoolConnection($pool) => $body,
            #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
            DatabaseConnectionType::Session($pool) => $body,
            // Without a backend feature there is no connection to match.
            #[cfg(not(any(feature = "postgres", feature = "mysql", feature = "sqlite")))]
            empty => match *empty {},
        }
    };
}

impl ConnectionTrait for DatabaseConnection {
    fn get_database_backend(&self) -> DbBackend {
        self.get_database_backend()
    }

    #[instrument(level = "trace", skip(stmt))]
    #[allow(unused_variables)]
    async fn execute_raw(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        on_pool!(&self.inner, pool => pool.execute(stmt).await)
    }

    #[instrument(level = "trace", skip(sql))]
    #[allow(unused_variables)]
    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        on_pool!(&self.inner, pool => pool.execute_unprepared(sql).await)
    }

    #[instrument(level = "trace", skip(stmt))]
    #[allow(unused_variables)]
    async fn query_one_raw(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        on_pool!(&self.inner, pool => pool.query_one(stmt).await)
    }

    #[instrument(level = "trace", skip(stmt))]
    #[allow(unused_variables)]
    async fn query_all_raw(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        on_pool!(&self.inner, pool => pool.query_all(stmt).await)
    }
}

impl TransactionTrait for DatabaseConnection {
    type Transaction = DatabaseTransaction;

    #[instrument(level = "trace")]
    async fn begin(&self) -> Result<DatabaseTransaction, DbErr> {
        self.begin_with_options(TransactionOptions::default()).await
    }

    #[instrument(level = "trace")]
    #[allow(unused_variables)]
    async fn begin_with_options(
        &self,
        options: TransactionOptions,
    ) -> Result<DatabaseTransaction, DbErr> {
        on_pool!(&self.inner, pool => pool.begin_with(options).await)
    }
}

impl DatabaseConnection {
    /// Get the database backend for this connection
    pub fn get_database_backend(&self) -> DbBackend {
        on_pool!(&self.inner, pool => pool.backend())
    }

    /// Sets a callback to metric this connection
    #[allow(unused_variables)]
    pub fn set_metric_callback<F>(&mut self, callback: F)
    where
        F: Fn(&crate::metric::Info<'_>) + Send + Sync + 'static,
    {
        on_pool!(&mut self.inner, pool => pool.set_metric_callback(callback))
    }

    /// A process-unique number naming this pool, or session. Its clones and
    /// the transactions begun on it share it; a session from
    /// [`pin_session`](Self::pin_session) has one of its own, and one from
    /// [`pin_session_sharing_identity`](Self::pin_session_sharing_identity)
    /// this one's. Never handed out again in the process, so it can key what
    /// is remembered per pool.
    pub fn identity(&self) -> u64 {
        on_pool!(&self.inner, pool => pool.identity())
    }

    /// A session on this connection's database: one connection of its own,
    /// opened with the pool's settings but outside it, so it takes none of
    /// the pool's connections. Everything a statement sets for its session
    /// holds for the statements after it, and the connection closes with
    /// [`close`](Self::close) or once the last clone drops. On a session, it
    /// opens another from the pool that one came from. Fails on an SQLite
    /// database private to each connection (in memory with `cache=private`,
    /// or temporary), where a connection of its own would open an empty one.
    pub async fn pin_session(&self) -> Result<DatabaseConnection, DbErr> {
        self.pin_session_as(super::new_identity()).await
    }

    /// [`pin_session`](Self::pin_session) under this connection's
    /// [`identity`](Self::identity), for a session that stands in for its
    /// pool.
    pub async fn pin_session_sharing_identity(&self) -> Result<DatabaseConnection, DbErr> {
        self.pin_session_as(self.identity()).await
    }

    #[allow(unused_variables)]
    async fn pin_session_as(&self, identity: u64) -> Result<DatabaseConnection, DbErr> {
        match &self.inner {
            #[cfg(feature = "mysql")]
            DatabaseConnectionType::MySqlPoolConnection(pool) => {
                Ok(pool.pin_session(identity).await?.into())
            }
            #[cfg(feature = "postgres")]
            DatabaseConnectionType::PostgresPoolConnection(pool) => {
                Ok(pool.pin_session(identity).await?.into())
            }
            #[cfg(feature = "sqlite")]
            DatabaseConnectionType::SqlitePoolConnection(pool) => {
                pool.ensure_sessions_share_the_database()?;
                Ok(pool.pin_session(identity).await?.into())
            }
            #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
            DatabaseConnectionType::Session(session) => {
                Box::pin(session.origin().pin_session_as(identity)).await
            }
            // Without a backend feature there is no connection to match.
            #[cfg(not(any(feature = "postgres", feature = "mysql", feature = "sqlite")))]
            empty => match *empty {},
        }
    }

    /// Checks if a connection to the database is still valid.
    pub async fn ping(&self) -> Result<(), DbErr> {
        on_pool!(&self.inner, pool => pool.ping().await)
    }

    /// How many statements the driver keeps prepared on one connection: the
    /// one the pool hands out next, or a session's own. A connection prepares
    /// each parameterized statement once and keeps up to its statement cache
    /// capacity of them (100 by default), dropping the least recently used.
    pub async fn cached_statements(&self) -> Result<usize, DbErr> {
        on_pool!(&self.inner, pool => pool.cached_statements().await)
    }

    /// Explicitly close the database connection.
    /// See [`Self::close_by_ref`] for usage with references.
    pub async fn close(self) -> Result<(), DbErr> {
        self.close_by_ref().await
    }

    /// Explicitly close the database connection
    pub async fn close_by_ref(&self) -> Result<(), DbErr> {
        on_pool!(&self.inner, pool => pool.close_by_ref().await)
    }
}

impl DatabaseConnection {
    /// The [driver::MySqlPool], or `None` for another backend. A session
    /// answers with the pool it was opened from.
    #[cfg(feature = "mysql")]
    pub fn get_mysql_connection_pool(&self) -> Option<&driver::MySqlPool> {
        match &self.inner {
            DatabaseConnectionType::MySqlPoolConnection(conn) => Some(&conn.pool),
            DatabaseConnectionType::Session(session) => {
                session.origin().get_mysql_connection_pool()
            }
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// The [driver::PgPool], or `None` for another backend. A session
    /// answers with the pool it was opened from.
    #[cfg(feature = "postgres")]
    pub fn get_postgres_connection_pool(&self) -> Option<&driver::PgPool> {
        match &self.inner {
            DatabaseConnectionType::PostgresPoolConnection(conn) => Some(&conn.pool),
            DatabaseConnectionType::Session(session) => {
                session.origin().get_postgres_connection_pool()
            }
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// The [driver::SqlitePool], or `None` for another backend. A session
    /// answers with the pool it was opened from.
    #[cfg(feature = "sqlite")]
    pub fn get_sqlite_connection_pool(&self) -> Option<&driver::SqlitePool> {
        match &self.inner {
            DatabaseConnectionType::SqlitePoolConnection(conn) => Some(&conn.pool),
            DatabaseConnectionType::Session(session) => {
                session.origin().get_sqlite_connection_pool()
            }
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}
