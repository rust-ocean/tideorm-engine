//! The backend a connection reaches, and what it implies for the SQL sent
//! there.

use crate::{Statement, StatementBuilder, error::*};

/// Evaluates `$body` with `$builder` bound to the query builder that renders
/// `$backend`'s statements.
macro_rules! on_builder {
    ($backend:expr, $builder:ident => $body:expr) => {
        match $backend {
            $crate::DbBackend::MySQL | $crate::DbBackend::MariaDB => {
                let $builder = sql_builder::MysqlQueryBuilder;
                $body
            }
            $crate::DbBackend::Postgres => {
                let $builder = sql_builder::PostgresQueryBuilder;
                $body
            }
            $crate::DbBackend::SQLite => {
                let $builder = sql_builder::SqliteQueryBuilder;
                $body
            }
        }
    };
}
pub(crate) use on_builder;

/// A database backend: the server a connection reaches. It decides how a
/// statement renders (placeholders, quoting, `RETURNING`) and what it may use.
///
/// MySQL and MariaDB speak one protocol, and a connection tells them apart by
/// what the server said when it opened: a MySQL-protocol pool answers
/// [`MariaDB`](Self::MariaDB) once it has opened a connection, which
/// [`Database::connect`](crate::Database::connect) does unless
/// `connect_lazy` is set, and [`MySQL`](Self::MySQL) before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DbBackend {
    /// PostgreSQL. The default.
    #[default]
    Postgres,
    /// MySQL.
    MySQL,
    /// MariaDB. Statements render as for MySQL, except where its
    /// [`dialect`](Self::dialect) differs.
    MariaDB,
    /// SQLite.
    SQLite,
}

impl DbBackend {
    /// The driver's maximum number of bind parameters in one prepared statement.
    ///
    /// Returns an error if this backend's driver feature is disabled. Configured
    /// server or connection limits may be lower than this protocol/build limit.
    pub fn max_bind_parameters(&self) -> Result<usize, DbErr> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres => Ok(<driver::Postgres as driver::Database>::MAX_BIND_PARAMETERS),
            #[cfg(feature = "mysql")]
            Self::MySQL | Self::MariaDB => {
                Ok(<driver::MySql as driver::Database>::MAX_BIND_PARAMETERS)
            }
            #[cfg(feature = "sqlite")]
            Self::SQLite => Ok(<driver::Sqlite as driver::Database>::MAX_BIND_PARAMETERS),
            #[allow(unreachable_patterns)]
            _ => Err(DbErr::Conn(RuntimeErr::Internal(format!(
                "{} driver feature is disabled",
                self.as_str()
            )))),
        }
    }

    /// Infer the backend from a connection URL's scheme.
    ///
    /// Recognises `postgres://`, `postgresql://`, `mariadb://`, `mysql://`
    /// and `sqlite:`, in any case, and returns `None` for anything else.
    /// [`Database::connect`](crate::Database::connect) picks its driver by
    /// it, the MySQL one for `mariadb://`. A MariaDB server reached through a
    /// `mysql://` URL is [`MySQL`](Self::MySQL) here; its connection reports
    /// [`MariaDB`](Self::MariaDB).
    pub fn from_url(url: &str) -> Option<Self> {
        let url = url.to_ascii_lowercase();
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            Some(Self::Postgres)
        } else if url.starts_with("mariadb://") {
            Some(Self::MariaDB)
        } else if url.starts_with("mysql://") {
            Some(Self::MySQL)
        } else if url.starts_with("sqlite:") {
            Some(Self::SQLite)
        } else {
            None
        }
    }

    /// Build an SQL [Statement]
    pub fn build<S>(&self, statement: &S) -> Result<Statement, DbErr>
    where
        S: StatementBuilder,
    {
        statement.build(self)
    }

    /// Render `condition` as the predicate of a `WHERE` clause, without the
    /// keyword, with the values it binds in place, so it can be spliced into
    /// a statement. A part this backend cannot render is an error.
    pub fn build_condition(
        &self,
        condition: &sql_builder::Condition,
    ) -> Result<sql_builder::BoundSql, DbErr> {
        use sql_builder::QueryBuilder;
        let mut sql = sql_builder::BoundSql::default();
        on_builder!(self, builder => builder.prepare_expr(&condition.clone().into(), &mut sql))
            .map_err(query_err)?;
        Ok(sql)
    }

    /// Render `statement` with the values it binds in place, so it can be
    /// spliced into another statement as a subquery. A part this backend
    /// cannot render is an error.
    pub fn build_bound(
        &self,
        statement: &impl sql_builder::QueryStatementBuilder,
    ) -> Result<sql_builder::BoundSql, DbErr> {
        let mut sql = sql_builder::BoundSql::default();
        on_builder!(self, builder => statement.build_collect_any_into(&builder, &mut sql))
            .map_err(query_err)?;
        Ok(sql)
    }

    /// The dialect this backend's SQL text is written in.
    pub fn dialect(self) -> &'static dyn sql_builder::SqlDialect {
        match self {
            Self::Postgres => &sql_builder::PostgresQueryBuilder,
            Self::MySQL => &sql_builder::MysqlQueryBuilder,
            Self::MariaDB => &sql_builder::MariaDbDialect,
            Self::SQLite => &sql_builder::SqliteQueryBuilder,
        }
    }

    /// Whether the backend has a native array column type.
    ///
    /// PostgreSQL only. Elsewhere an "array" column is a JSON array.
    pub fn supports_arrays(&self) -> bool {
        self.dialect().native_arrays()
    }

    /// Whether an `UPDATE` or `DELETE` can return the rows it wrote.
    ///
    /// MySQL has no `RETURNING`. MariaDB has `INSERT .. RETURNING` (from
    /// 10.5), which the engine's inserts use, but `UPDATE .. RETURNING` only
    /// from 13.0, a server version the engine does not check.
    pub fn supports_returning(&self) -> bool {
        matches!(self, Self::Postgres | Self::SQLite)
    }

    /// A reasonable number of rows to write per batch on this backend.
    ///
    /// A guideline for chunking bulk work, chosen to stay clear of each
    /// backend's statement and parameter limits, not an enforced cap.
    pub fn optimal_batch_size(&self) -> usize {
        match self {
            Self::Postgres => 1000,
            Self::MySQL | Self::MariaDB => 500,
            Self::SQLite => 100,
        }
    }

    /// The character this backend quotes identifiers with.
    ///
    /// Informational. Do not build SQL by wrapping a name in it: the dialect's
    /// quoting helpers also escape the quote character itself.
    pub fn quote_char(&self) -> char {
        self.dialect().identifier_quote().left()
    }

    /// The backend's name: `PostgreSQL`, `MySQL`, `MariaDB` or `SQLite`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::MySQL => "MySQL",
            Self::MariaDB => "MariaDB",
            Self::SQLite => "SQLite",
        }
    }
}

impl From<DbBackend> for sql_builder::lexer::Dialect {
    fn from(backend: DbBackend) -> Self {
        match backend {
            DbBackend::MySQL | DbBackend::MariaDB => Self::MySql,
            DbBackend::Postgres => Self::Postgres,
            DbBackend::SQLite => Self::Sqlite,
        }
    }
}

impl std::fmt::Display for DbBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
