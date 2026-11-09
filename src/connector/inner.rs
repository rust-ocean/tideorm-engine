#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
use driver::pool::PoolConnection;

/// The pooled connection a [`DatabaseTransaction`](crate::DatabaseTransaction)
/// holds, of whichever driver its pool uses.
#[derive(Debug)]
pub enum InnerConnection {
    /// A MySQL or MariaDB connection.
    #[cfg(feature = "mysql")]
    MySql(PoolConnection<driver::MySql>),
    /// A PostgreSQL connection.
    #[cfg(feature = "postgres")]
    Postgres(PoolConnection<driver::Postgres>),
    /// A SQLite connection.
    #[cfg(feature = "sqlite")]
    Sqlite(PoolConnection<driver::Sqlite>),
    /// A session's connection once it was closed; nothing runs on it.
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(dead_code)
    )]
    Closed,
}

/// Evaluates `$body` with `$connection` bound to the driver's own
/// `&mut PoolConnection<_>` inside `$inner` (a `&mut InnerConnection`), so
/// code generic over [`Driver`](super::Driver) runs on each driver.
/// With no driver compiled in, it evaluates to a "Disconnected" error.
macro_rules! on_connection {
    ($inner:expr, $connection:ident => $body:expr) => {
        match $inner {
            #[cfg(feature = "mysql")]
            $crate::connector::InnerConnection::MySql($connection) => $body,
            #[cfg(feature = "postgres")]
            $crate::connector::InnerConnection::Postgres($connection) => $body,
            #[cfg(feature = "sqlite")]
            $crate::connector::InnerConnection::Sqlite($connection) => $body,
            #[allow(unreachable_patterns)]
            _ => Err($crate::error::conn_err("Disconnected")),
        }
    };
}
pub(crate) use on_connection;
