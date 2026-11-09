#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub(crate) mod backend;
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
mod common;
mod inner;
#[cfg(feature = "mysql")]
mod mysql;
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
mod pool;
#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub(crate) use backend::Driver;
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub(crate) use common::*;
pub(crate) use inner::{InnerConnection, on_connection};
#[cfg(feature = "mysql")]
pub use mysql::*;
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub(crate) use pool::DriverPoolConnection;
#[cfg(feature = "postgres")]
pub use postgres::*;
#[cfg(feature = "sqlite")]
pub use sqlite::*;
