use super::backend::on_builder;
use crate::{DbBackend, DbErr, error::query_err};
use sql_builder::{QueryBuilder, QueryStatementBuilder, SqlWriterValues};
pub use sql_builder::{Value, Values};
use std::fmt;

/// A SQL string together with its bound parameters, ready to send to a
/// connection. Build one yourself with
/// [`from_sql_and_values`](Self::from_sql_and_values), or get one out of any query builder
/// via [`QueryTrait::build`](crate::QueryTrait::build).
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    /// The SQL text, with backend-specific placeholders for the values.
    pub sql: String,
    /// Bound parameter values, in the order they appear in `sql`. `None`
    /// means the statement has no parameters.
    pub values: Option<Values>,
    /// Backend this statement was built for; both the SQL dialect and the
    /// placeholder style must match.
    pub db_backend: DbBackend,
    /// Whether the connection's metric callback should log this statement; it
    /// hears of every statement either way. See [`unlogged`](Self::unlogged).
    pub log: bool,
}

/// Anything that can be rendered to a backend-specific [`Statement`].
///
/// Implemented by the `sql_builder` statement types (`SelectStatement`,
/// `InsertStatement`, etc.) so they can be passed to
/// [`ConnectionTrait::execute`](crate::ConnectionTrait::execute) /
/// [`query_all`](crate::ConnectionTrait::query_all) directly.
pub trait StatementBuilder: Sync {
    /// Render `self` into a [`Statement`] for `db_backend`.
    fn build(&self, db_backend: &DbBackend) -> Result<Statement, DbErr>;
}

impl Statement {
    /// Encode all parameters for PostgreSQL before executing any SQL.
    ///
    /// Returns the driver's arguments, or an encoding error without a partial
    /// argument list. This does not validate SQL, select a connection, or apply
    /// TideORM's model-specific storage checks. The statement remains reusable.
    #[cfg(feature = "postgres")]
    pub fn try_postgres_arguments(&self) -> Result<driver::postgres::PgArguments, driver::Error> {
        crate::binding::postgres(self.values.as_ref().map_or(&[], |values| &values.0))
    }

    /// Encode all parameters for MySQL/MariaDB before executing any SQL.
    ///
    /// Returns the driver's arguments, or an encoding error without a partial
    /// argument list. A `DateTime<Utc>` or `DateTime<Local>` is bound as the UTC
    /// `DATETIME` it holds. Model validation remains the caller's responsibility;
    /// SQL and dialect are not validated. The statement remains reusable.
    #[cfg(feature = "mysql")]
    pub fn try_mysql_arguments(&self) -> Result<driver::mysql::MySqlArguments, driver::Error> {
        crate::binding::mysql(self.values.as_ref().map_or(&[], |values| &values.0))
    }

    /// Encode all parameters for SQLite before executing any SQL.
    ///
    /// Returns the driver's arguments, or an encoding error without a partial
    /// argument list. Model storage checks (including finite floating-point and
    /// readable decimal limits) remain in TideORM. SQL and dialect are not
    /// validated. The statement remains reusable.
    #[cfg(feature = "sqlite")]
    pub fn try_sqlite_arguments(&self) -> Result<driver::sqlite::SqliteArguments, driver::Error> {
        crate::binding::sqlite(self.values.as_ref().map_or(&[], |values| &values.0))
    }

    /// Create a [Statement] from a [crate::DbBackend] and a raw SQL statement
    pub fn from_string<T>(db_backend: DbBackend, stmt: T) -> Statement
    where
        T: Into<String>,
    {
        Statement {
            sql: stmt.into(),
            values: None,
            db_backend,
            log: true,
        }
    }

    /// Create a SQL statement from a [crate::DbBackend], a
    /// raw SQL statement and param values
    pub fn from_sql_and_values<I, T>(db_backend: DbBackend, sql: T, values: I) -> Self
    where
        I: IntoIterator<Item = Value>,
        T: Into<String>,
    {
        Self::from_string_values_tuple(db_backend, (sql, Values(values.into_iter().collect())))
    }

    pub(crate) fn from_string_values_tuple<T>(db_backend: DbBackend, stmt: (T, Values)) -> Statement
    where
        T: Into<String>,
    {
        Statement {
            sql: stmt.0.into(),
            values: Some(stmt.1),
            db_backend,
            log: true,
        }
    }

    /// The statement, marked for the connection's metric callback not to log:
    /// for a caller that logs it itself, with more context, or for a probe of
    /// its own. The callback still times it.
    pub fn unlogged(mut self) -> Self {
        self.log = false;
        self
    }
}

/// The SQL with its values inlined. Values that cannot be inlined (a marker
/// with no value, a value with no marker) leave the SQL as it is, followed by
/// a comment saying why.
impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.values {
            Some(values) => match self
                .db_backend
                .dialect()
                .inline_values(&self.sql, &values.0)
            {
                Ok(string) => f.write_str(&string),
                Err(error) => write!(f, "{} /* values not inlined: {error} */", self.sql),
            },
            None => {
                write!(f, "{}", self.sql)
            }
        }
    }
}

fn build_query<S: QueryStatementBuilder>(
    statement: &S,
    db_backend: DbBackend,
) -> Result<Statement, DbErr> {
    let stmt = on_builder!(db_backend, builder => {
        let (placeholder, numbered) = builder.placeholder();
        let mut sql = SqlWriterValues::new(placeholder, numbered);
        statement
            .build_collect_any_into(&builder, &mut sql)
            .map_err(query_err)?;
        sql.into_parts()
    });
    Ok(Statement::from_string_values_tuple(db_backend, stmt))
}

macro_rules! build_query_stmt {
    ($stmt: ty) => {
        impl StatementBuilder for $stmt {
            fn build(&self, db_backend: &DbBackend) -> Result<Statement, DbErr> {
                build_query(self, *db_backend)
            }
        }
    };
}

build_query_stmt!(sql_builder::InsertStatement);
build_query_stmt!(sql_builder::SelectStatement);
build_query_stmt!(sql_builder::UpdateStatement);
build_query_stmt!(sql_builder::DeleteStatement);
build_query_stmt!(sql_builder::WithQuery);
