use std::time::Duration;

#[cfg(feature = "mysql")]
use driver::mysql::MySqlConnectOptions;
#[cfg(feature = "postgres")]
use driver::postgres::PgConnectOptions;
#[cfg(feature = "sqlite")]
use driver::sqlite::SqliteConnectOptions;

mod backend;
mod connection;
mod db_connection;
mod executor;
mod masking;
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
mod session;
mod shared_connection;
mod statement;
mod transaction;

pub use backend::DbBackend;
pub use connection::*;
pub use db_connection::*;
pub use executor::*;
pub use masking::mask_url_credentials;
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub use session::SessionConnection;
pub(crate) use shared_connection::SharedConnection;
pub use statement::*;
use tracing::instrument;
pub use transaction::*;

use crate::error::*;

/// A new [`DatabaseConnection::identity`], never handed out before.
pub(crate) fn new_identity() -> u64 {
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    LAST.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// Entry point for opening a [`DatabaseConnection`]; see [`Database::connect`].
#[derive(Debug, Default)]
pub struct Database;

/// What [`Database::connect`] opens: a URL, whose scheme names the backend,
/// or one backend's driver options, which reach the settings a URL cannot
/// (a SQLite collation, client certificates from memory).
// Built once per pool, so the driver options' size costs nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
pub enum ConnectTarget {
    /// A `postgres://`, `postgresql://`, `mysql://`, `mariadb://` or `sqlite:` URL.
    Url(String),
    /// PostgreSQL driver options.
    #[cfg(feature = "postgres")]
    Postgres(PgConnectOptions),
    /// MySQL or MariaDB driver options.
    #[cfg(feature = "mysql")]
    MySql(MySqlConnectOptions),
    /// SQLite driver options.
    #[cfg(feature = "sqlite")]
    Sqlite(SqliteConnectOptions),
}

/// Masks a URL's credentials and leaves driver options out, since they hold
/// the password: `{:?}` is how configuration ends up in logs.
impl std::fmt::Debug for ConnectTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Url(url) => f
                .debug_tuple("Url")
                .field(&mask_url_credentials(url))
                .finish(),
            #[cfg(feature = "postgres")]
            Self::Postgres(_) => f.write_str("Postgres(..)"),
            #[cfg(feature = "mysql")]
            Self::MySql(_) => f.write_str("MySql(..)"),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(_) => f.write_str("Sqlite(..)"),
        }
    }
}

impl From<&str> for ConnectTarget {
    fn from(url: &str) -> Self {
        Self::Url(url.to_owned())
    }
}

impl From<String> for ConnectTarget {
    fn from(url: String) -> Self {
        Self::Url(url)
    }
}

#[cfg(feature = "postgres")]
impl From<PgConnectOptions> for ConnectTarget {
    fn from(options: PgConnectOptions) -> Self {
        Self::Postgres(options)
    }
}

#[cfg(feature = "mysql")]
impl From<MySqlConnectOptions> for ConnectTarget {
    fn from(options: MySqlConnectOptions) -> Self {
        Self::MySql(options)
    }
}

#[cfg(feature = "sqlite")]
impl From<SqliteConnectOptions> for ConnectTarget {
    fn from(options: SqliteConnectOptions) -> Self {
        Self::Sqlite(options)
    }
}

/// Configuration for opening a [`DatabaseConnection`]: what to connect to,
/// and the pool's sizing and timeouts.
///
/// Construct with [`ConnectOptions::new`] (or directly from a URL or driver
/// options), then chain the setter methods before passing to [`Database::connect`].
#[derive(Clone, Debug)]
pub struct ConnectOptions {
    /// What the pool connects to
    pub(crate) target: ConnectTarget,
    /// Maximum number of connections for a pool
    pub(crate) max_connections: Option<u32>,
    /// Minimum number of connections for a pool
    pub(crate) min_connections: Option<u32>,
    /// Deadline for each connection attempt, including driver initialization
    pub(crate) connect_timeout: Option<Duration>,
    /// Maximum idle time for a particular connection to prevent
    /// network resource exhaustion
    pub(crate) idle_timeout: Option<Option<Duration>>,
    /// Set the maximum amount of time to spend waiting for acquiring a connection
    pub(crate) acquire_timeout: Option<Duration>,
    /// Set the maximum lifetime of individual connections
    pub(crate) max_lifetime: Option<Option<Duration>>,
    pub(crate) test_before_acquire: bool,
    /// If set, a pooled connection is pinged before being handed out only when it has been
    /// idle for at least this long (see [`ConnectOptions::test_before_acquire_if_idle_for`]).
    pub(crate) test_before_acquire_if_idle_for: Option<Duration>,
    /// Only establish connections to the DB as needed. If set to `true`, the db connection will
    /// be created with
    /// [`PoolOptions::connect_lazy_with`](driver::pool::PoolOptions::connect_lazy_with).
    pub(crate) connect_lazy: bool,
    /// How many prepared statements each connection keeps; `None` leaves the
    /// driver options' own setting.
    pub(crate) statement_cache_capacity: Option<usize>,
}

impl Database {
    /// Method to create a [DatabaseConnection] on a database. This method will return an error
    /// if the database is not available.
    #[instrument(level = "trace", skip(opt))]
    pub async fn connect<C>(opt: C) -> Result<DatabaseConnection, DbErr>
    where
        C: Into<ConnectOptions>,
    {
        let opt: ConnectOptions = opt.into();
        match opt.target.clone() {
            ConnectTarget::Url(url) => match DbBackend::from_url(&url) {
                #[cfg(feature = "mysql")]
                Some(DbBackend::MySQL | DbBackend::MariaDB) => {
                    crate::MySqlConnector::connect(opt, crate::connector::parse_url(&url)?).await
                }
                #[cfg(feature = "postgres")]
                Some(DbBackend::Postgres) => {
                    crate::PostgresConnector::connect(opt, crate::connector::parse_url(&url)?).await
                }
                #[cfg(feature = "sqlite")]
                Some(DbBackend::SQLite) => {
                    crate::SqliteConnector::connect(opt, crate::connector::parse_url(&url)?).await
                }
                #[allow(unreachable_patterns)]
                Some(backend) => Err(conn_err(format!(
                    "The connection string '{}' is for {backend}, whose driver this build leaves out.",
                    mask_url_credentials(&url)
                ))),
                None => Err(conn_err(format!(
                    "The connection string '{}' is not a postgres://, postgresql://, mysql://, mariadb:// or sqlite: URL.",
                    mask_url_credentials(&url)
                ))),
            },
            #[cfg(feature = "postgres")]
            ConnectTarget::Postgres(connect) => {
                crate::PostgresConnector::connect(opt, connect).await
            }
            #[cfg(feature = "mysql")]
            ConnectTarget::MySql(connect) => crate::MySqlConnector::connect(opt, connect).await,
            #[cfg(feature = "sqlite")]
            ConnectTarget::Sqlite(connect) => crate::SqliteConnector::connect(opt, connect).await,
        }
    }
}

impl<T> From<T> for ConnectOptions
where
    T: Into<ConnectTarget>,
{
    fn from(target: T) -> ConnectOptions {
        ConnectOptions::new(target)
    }
}

impl ConnectOptions {
    /// Create new [ConnectOptions] for a [Database] from a URL or one backend's driver options
    pub fn new<T>(target: T) -> Self
    where
        T: Into<ConnectTarget>,
    {
        Self {
            target: target.into(),
            max_connections: None,
            min_connections: None,
            connect_timeout: None,
            idle_timeout: None,
            acquire_timeout: None,
            max_lifetime: None,
            test_before_acquire: true,
            test_before_acquire_if_idle_for: None,
            connect_lazy: false,
            statement_cache_capacity: None,
        }
    }

    /// Get what the pool connects to
    pub fn get_target(&self) -> &ConnectTarget {
        &self.target
    }

    /// Set the maximum number of connections of the pool
    pub fn max_connections(&mut self, value: u32) -> &mut Self {
        self.max_connections = Some(value);
        self
    }

    /// Get the maximum number of connections of the pool, if set
    pub fn get_max_connections(&self) -> Option<u32> {
        self.max_connections
    }

    /// Set the minimum number of connections of the pool
    pub fn min_connections(&mut self, value: u32) -> &mut Self {
        self.min_connections = Some(value);
        self
    }

    /// Get the minimum number of connections of the pool, if set
    pub fn get_min_connections(&self) -> Option<u32> {
        self.min_connections
    }

    /// Bound each attempt to open a connection, TLS, authentication and
    /// connection hooks included (30 seconds when unset). A checkout still ends at
    /// [`acquire_timeout`](Self::acquire_timeout) if that comes first.
    pub fn connect_timeout(&mut self, value: Duration) -> &mut Self {
        self.connect_timeout = Some(value);
        self
    }

    /// Get the timeout for each connection attempt, if set
    pub fn get_connect_timeout(&self) -> Option<Duration> {
        self.connect_timeout
    }

    /// Close a connection left unused this long (10 minutes when unset; `None` keeps it open).
    pub fn idle_timeout<T>(&mut self, value: T) -> &mut Self
    where
        T: Into<Option<Duration>>,
    {
        self.idle_timeout = Some(value.into());
        self
    }

    /// Get the idle duration before closing a connection, if set
    pub fn get_idle_timeout(&self) -> Option<Option<Duration>> {
        self.idle_timeout
    }

    /// Bound a whole checkout: waiting for a free connection, its ping and
    /// hooks, and opening a new one (30 seconds when unset). Each attempt to open one is also
    /// bounded by [`connect_timeout`](Self::connect_timeout).
    pub fn acquire_timeout(&mut self, value: Duration) -> &mut Self {
        self.acquire_timeout = Some(value);
        self
    }

    /// Get the maximum amount of time to spend waiting for acquiring a connection
    pub fn get_acquire_timeout(&self) -> Option<Duration> {
        self.acquire_timeout
    }

    /// Close a connection this old, however busy (30 minutes when unset; `None` never retires it). Keep it under any idle
    /// or lifetime limit the server, or a proxy in front of it, enforces.
    pub fn max_lifetime<T>(&mut self, lifetime: T) -> &mut Self
    where
        T: Into<Option<Duration>>,
    {
        self.max_lifetime = Some(lifetime.into());
        self
    }

    /// Get the maximum lifetime of individual connections, if set
    pub fn get_max_lifetime(&self) -> Option<Option<Duration>> {
        self.max_lifetime
    }

    /// Ping a pooled connection on checkout when true (default): one idle for
    /// a second or more, or for the threshold
    /// [`test_before_acquire_if_idle_for`](Self::test_before_acquire_if_idle_for)
    /// sets. False pings none, unless a threshold is set.
    pub fn test_before_acquire(&mut self, value: bool) -> &mut Self {
        self.test_before_acquire = value;
        self
    }

    /// Get the checkout ping flag (default true); an idle threshold overrides it.
    pub fn get_test_before_acquire(&self) -> bool {
        self.test_before_acquire
    }

    /// Ping a pooled connection before handing it out, but only once it has been idle for at
    /// least `idle`, in place of the second
    /// [`test_before_acquire`](Self::test_before_acquire) waits by default.
    ///
    /// A connection idle long enough to plausibly have been dropped by the server, a proxy,
    /// or a firewall — the common failure mode — is tested, while hot connections go through
    /// without the round trip. `Duration::ZERO` pings on every checkout.
    ///
    /// The threshold takes precedence over [`test_before_acquire`](Self::test_before_acquire).
    /// A ping runs before the per-backend user hook; a failed ping discards the
    /// connection without calling that hook.
    ///
    /// Applies to pools built through [`Database::connect`]. For adopted pools,
    /// configure `test_before_acquire_if_idle_for` on [`driver::pool::PoolOptions`].
    pub fn test_before_acquire_if_idle_for(&mut self, idle: Duration) -> &mut Self {
        self.test_before_acquire_if_idle_for = Some(idle);
        self
    }

    /// Get the idle threshold set by
    /// [`test_before_acquire_if_idle_for`](Self::test_before_acquire_if_idle_for), if any;
    /// without one, a checkout pings a connection idle for a second.
    pub fn get_test_before_acquire_if_idle_for(&self) -> Option<Duration> {
        self.test_before_acquire_if_idle_for
    }

    /// If set to `true`, the db connection pool is created with
    /// [`PoolOptions::connect_lazy_with`](driver::pool::PoolOptions::connect_lazy_with):
    /// no connection opens until one is needed.
    pub fn connect_lazy(&mut self, value: bool) -> &mut Self {
        self.connect_lazy = value;
        self
    }

    /// Get whether DB connections will be established when the pool is created or only as needed.
    pub fn get_connect_lazy(&self) -> bool {
        self.connect_lazy
    }

    /// How many prepared statements each connection keeps, dropping the least
    /// recently used past that (the drivers keep 100). With 0 none is kept:
    /// every statement is prepared for its run and closed after it.
    pub fn statement_cache_capacity(&mut self, capacity: usize) -> &mut Self {
        self.statement_cache_capacity = Some(capacity);
        self
    }

    /// Get the capacity set by
    /// [`statement_cache_capacity`](Self::statement_cache_capacity), if any.
    pub fn get_statement_cache_capacity(&self) -> Option<usize> {
        self.statement_cache_capacity
    }
}
