//! Which server a connection reached, beyond the dialect it speaks.

use crate::DbBackend;

/// The server a connection reached, as it said when the connection opened.
/// Several servers speak PostgreSQL's or MySQL's protocol and render that
/// backend's SQL ([`DbBackend`]); this names which one it is, for what it
/// does differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DbServer {
    /// PostgreSQL.
    Postgres,
    /// CockroachDB, speaking PostgreSQL's protocol.
    CockroachDb,
    /// YugabyteDB's YSQL, speaking PostgreSQL's protocol.
    YugabyteDb,
    /// MySQL.
    MySql,
    /// MariaDB.
    MariaDb,
    /// TiDB, speaking MySQL's protocol.
    TiDb,
    /// Vitess's `vtgate`, speaking MySQL's protocol.
    Vitess,
    /// SQLite.
    Sqlite,
}

impl DbServer {
    /// The dialect the server's statements are rendered in.
    pub fn backend(self) -> DbBackend {
        match self {
            Self::Postgres | Self::CockroachDb | Self::YugabyteDb => DbBackend::Postgres,
            Self::MySql | Self::TiDb | Self::Vitess => DbBackend::MySQL,
            Self::MariaDb => DbBackend::MariaDB,
            Self::Sqlite => DbBackend::SQLite,
        }
    }

    /// The server's `db.system.name` in OpenTelemetry's semantic
    /// conventions, or its own lowercase name where they list none.
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(dead_code)
    )]
    pub(crate) fn system_name(self) -> &'static str {
        match self {
            Self::Postgres => "postgresql",
            Self::CockroachDb => "cockroachdb",
            Self::YugabyteDb => "yugabytedb",
            Self::MySql => "mysql",
            Self::MariaDb => "mariadb",
            Self::TiDb => "tidb",
            Self::Vitess => "vitess",
            Self::Sqlite => "sqlite",
        }
    }
}

impl std::fmt::Display for DbServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Postgres => "PostgreSQL",
            Self::CockroachDb => "CockroachDB",
            Self::YugabyteDb => "YugabyteDB",
            Self::MySql => "MySQL",
            Self::MariaDb => "MariaDB",
            Self::TiDb => "TiDB",
            Self::Vitess => "Vitess",
            Self::Sqlite => "SQLite",
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/server_tests.rs"]
mod tests;
