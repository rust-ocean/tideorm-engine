pub use crate::error::TryGetError;
use crate::error::{DbErr, type_err};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    hash::Hash,
    sync::Arc,
};

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
use crate::connector::*;
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
use driver::Row;
#[cfg(feature = "postgres")]
use driver::{TypeInfo, ValueRef};

/// One row of a query result.
///
/// Returned by [`ConnectionTrait::query_one`](crate::ConnectionTrait::query_one)
/// / [`query_all`](crate::ConnectionTrait::query_all). Read columns from it
/// with [`QueryResult::try_get`] / [`try_get_by`](Self::try_get_by) or
/// materialise into a struct via [`FromQueryResult`](crate::FromQueryResult).
#[derive(Debug)]
pub struct QueryResult {
    pub(crate) row: QueryResultRow,
}

#[allow(clippy::enum_variant_names)]
pub(crate) enum QueryResultRow {
    #[cfg(feature = "mysql")]
    MySql(driver::mysql::MySqlRow),
    #[cfg(feature = "postgres")]
    Postgres(driver::postgres::PgRow),
    #[cfg(feature = "sqlite")]
    Sqlite(driver::sqlite::SqliteRow),
}

/// Evaluates `$body` with `$row` bound to the driver's own row inside
/// `$result` (a `&QueryResultRow`) and `$index` to `$idx` as that driver's
/// column index.
macro_rules! on_row {
    ($result:expr, $idx:expr, |$row:ident, $index:ident| $body:expr) => {
        match $result {
            #[cfg(feature = "mysql")]
            QueryResultRow::MySql($row) => {
                let $index = $idx.as_mysql_index();
                $body
            }
            #[cfg(feature = "postgres")]
            QueryResultRow::Postgres($row) => {
                let $index = $idx.as_postgres_index();
                $body
            }
            #[cfg(feature = "sqlite")]
            QueryResultRow::Sqlite($row) => {
                let $index = $idx.as_sqlite_index();
                $body
            }
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    };
}

/// Decode a single column value out of a [`QueryResult`].
///
/// Implemented for every primitive the engine understands (integers, strings,
/// booleans, JSON, UUIDs, chrono / time types when their features are
/// enabled, etc.). User code rarely names this trait directly; it's the
/// foundation that [`FromQueryResult`](crate::FromQueryResult) builds on.
pub trait TryGetable: Sized {
    /// Decode the value at the column named or positioned by `index`.
    fn try_get_by<I: ColIdx>(res: &QueryResult, index: I) -> Result<Self, TryGetError>;

    /// Decode the value at the given positional index in the SELECT list.
    fn try_get_by_index(res: &QueryResult, index: usize) -> Result<Self, TryGetError> {
        Self::try_get_by(res, index)
    }
}

impl From<TryGetError> for DbErr {
    fn from(e: TryGetError) -> DbErr {
        match e {
            TryGetError::DbErr(e) => e,
            TryGetError::Null(s) => {
                type_err(format!("A null value was encountered while decoding {s}"))
            }
        }
    }
}

impl From<DbErr> for TryGetError {
    fn from(e: DbErr) -> TryGetError {
        Self::DbErr(e)
    }
}

// QueryResult //

impl QueryResult {
    #[doc(hidden)]
    #[cfg(feature = "postgres")]
    pub fn try_get_from_postgres<T, I>(&self, idx: I) -> Option<Result<T, TryGetError>>
    where
        T: driver::Type<driver::Postgres> + for<'r> driver::Decode<'r, driver::Postgres>,
        I: ColIdx,
    {
        match &self.row {
            QueryResultRow::Postgres(row) => {
                let value = match row.try_get_raw(idx.as_postgres_index()) {
                    Ok(value) => value,
                    Err(err) => return Some(Err(driver_error_to_query_err(err).into())),
                };

                if !value.is_null() {
                    let ty = value.type_info();
                    if !ty.is_null() && !T::compatible(&ty) {
                        return None;
                    }
                }

                Some(impls::column(row, idx.as_postgres_index(), idx))
            }
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// Get a value from the query result with an ColIdx
    pub fn try_get_by<T, I>(&self, index: I) -> Result<T, DbErr>
    where
        T: TryGetable,
        I: ColIdx,
    {
        Ok(T::try_get_by(self, index)?)
    }

    /// Get a value from the query result with an ColIdx
    pub fn try_get_by_nullable<T, I>(&self, index: I) -> Result<T, TryGetError>
    where
        T: TryGetable,
        I: ColIdx,
    {
        T::try_get_by(self, index)
    }

    /// Get a value from the query result by column name
    pub fn try_get<T>(&self, col: &str) -> Result<T, DbErr>
    where
        T: TryGetable,
    {
        Ok(T::try_get_by(self, col)?)
    }

    /// Get a value from the query result based on the order in the select expressions
    pub fn try_get_by_index<T>(&self, idx: usize) -> Result<T, DbErr>
    where
        T: TryGetable,
    {
        Ok(T::try_get_by_index(self, idx)?)
    }

    /// Get a value from the query result based on the order in the select expressions
    pub fn try_get_by_index_nullable<T>(&self, idx: usize) -> Result<T, TryGetError>
    where
        T: TryGetable,
    {
        T::try_get_by_index(self, idx)
    }

    /// Get a tuple value from the query result by column names
    pub fn try_get_many<T>(&self, cols: &[String]) -> Result<T, DbErr>
    where
        T: TryGetableMany,
    {
        Ok(T::try_get_many(self, cols)?)
    }

    /// Get a tuple value from the query result based on the order in the select expressions
    pub fn try_get_many_by_index<T>(&self) -> Result<T, DbErr>
    where
        T: TryGetableMany,
    {
        Ok(T::try_get_many_by_index(self)?)
    }

    /// Retrieves the names of the columns in the result set
    pub fn column_names(&self) -> Vec<String> {
        #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
        use driver::Column;

        on_row!(&self.row, 0usize, |row, _index| {
            row.columns().iter().map(|c| c.name().to_string()).collect()
        })
    }

    /// Access the underlying `MySqlRow` if we use the MySQL backend.
    #[cfg(feature = "mysql")]
    pub fn try_as_mysql_row(&self) -> Option<&driver::mysql::MySqlRow> {
        match &self.row {
            QueryResultRow::MySql(mysql_row) => Some(mysql_row),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// Access the underlying `PgRow` if we use the Postgres backend.
    #[cfg(feature = "postgres")]
    pub fn try_as_pg_row(&self) -> Option<&driver::postgres::PgRow> {
        match &self.row {
            QueryResultRow::Postgres(pg_row) => Some(pg_row),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// Access the underlying `SqliteRow` if we use the SQLite backend.
    #[cfg(feature = "sqlite")]
    pub fn try_as_sqlite_row(&self) -> Option<&driver::sqlite::SqliteRow> {
        match &self.row {
            QueryResultRow::Sqlite(sqlite_row) => Some(sqlite_row),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

#[allow(unused_variables)]
impl Debug for QueryResultRow {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            #[cfg(feature = "mysql")]
            Self::MySql(row) => write!(f, "{row:?}"),
            #[cfg(feature = "postgres")]
            Self::Postgres(_) => write!(f, "QueryResultRow::Postgres cannot be inspected"),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(_) => write!(f, "QueryResultRow::Sqlite cannot be inspected"),
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }
}

// TryGetable //

impl<T: TryGetable> TryGetable for Option<T> {
    fn try_get_by<I: ColIdx>(res: &QueryResult, index: I) -> Result<Self, TryGetError> {
        match T::try_get_by(res, index) {
            Ok(v) => Ok(Some(v)),
            Err(TryGetError::Null(_)) => Ok(None),
            Err(TryGetError::DbErr(err)) if is_column_not_found(&err) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Whether `err` is the driver's report that the row has no such column.
fn is_column_not_found(err: &DbErr) -> bool {
    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    if let DbErr::Query(crate::RuntimeErr::DriverError(err)) = err {
        return matches!(**err, driver::Error::ColumnNotFound(_));
    }
    let _ = err;
    false
}

/// Reads one field of a derived model: `None` for SQL NULL, and the
/// "Missing value" type error for a column the row does not have, so a
/// partial row is never read as an absent joined model.
#[doc(hidden)]
pub fn __read_model_field<T: TryGetable>(
    res: &QueryResult,
    col: &str,
) -> Result<Option<T>, TryGetError> {
    match T::try_get_by(res, col) {
        Ok(value) => Ok(Some(value)),
        Err(TryGetError::Null(_)) => Ok(None),
        Err(TryGetError::DbErr(err)) if is_column_not_found(&err) => Err(TryGetError::DbErr(
            type_err(format!("Missing value for column '{col}'")),
        )),
        Err(err) => Err(err),
    }
}

/// Column Index, used by [`TryGetable`]. Implemented for `&str` and `usize`
pub trait ColIdx: Debug + Copy {
    #[cfg(feature = "mysql")]
    /// Type surrogate
    type MySqlIndex: driver::ColumnIndex<driver::mysql::MySqlRow>;
    #[cfg(feature = "postgres")]
    /// Type surrogate
    type PostgresIndex: driver::ColumnIndex<driver::postgres::PgRow>;
    #[cfg(feature = "sqlite")]
    /// Type surrogate
    type SqliteIndex: driver::ColumnIndex<driver::sqlite::SqliteRow>;

    #[cfg(feature = "mysql")]
    /// Basically a no-op; only to satisfy trait bounds
    fn as_mysql_index(&self) -> Self::MySqlIndex;
    #[cfg(feature = "postgres")]
    /// Basically a no-op; only to satisfy trait bounds
    fn as_postgres_index(&self) -> Self::PostgresIndex;
    #[cfg(feature = "sqlite")]
    /// Basically a no-op; only to satisfy trait bounds
    fn as_sqlite_index(&self) -> Self::SqliteIndex;

    /// Self must be `&str`, return `None` otherwise
    fn as_str(&self) -> Option<&str>;
    /// Self must be `usize`, return `None` otherwise
    fn as_usize(&self) -> Option<&usize>;
}

impl ColIdx for &str {
    #[cfg(feature = "mysql")]
    type MySqlIndex = Self;
    #[cfg(feature = "postgres")]
    type PostgresIndex = Self;
    #[cfg(feature = "sqlite")]
    type SqliteIndex = Self;

    #[cfg(feature = "mysql")]
    #[inline]
    fn as_mysql_index(&self) -> Self::MySqlIndex {
        self
    }
    #[cfg(feature = "postgres")]
    #[inline]
    fn as_postgres_index(&self) -> Self::PostgresIndex {
        self
    }
    #[cfg(feature = "sqlite")]
    #[inline]
    fn as_sqlite_index(&self) -> Self::SqliteIndex {
        self
    }

    #[inline]
    fn as_str(&self) -> Option<&str> {
        Some(self)
    }
    #[inline]
    fn as_usize(&self) -> Option<&usize> {
        None
    }
}

impl ColIdx for usize {
    #[cfg(feature = "mysql")]
    type MySqlIndex = Self;
    #[cfg(feature = "postgres")]
    type PostgresIndex = Self;
    #[cfg(feature = "sqlite")]
    type SqliteIndex = Self;

    #[cfg(feature = "mysql")]
    #[inline]
    fn as_mysql_index(&self) -> Self::MySqlIndex {
        *self
    }
    #[cfg(feature = "postgres")]
    #[inline]
    fn as_postgres_index(&self) -> Self::PostgresIndex {
        *self
    }
    #[cfg(feature = "sqlite")]
    #[inline]
    fn as_sqlite_index(&self) -> Self::SqliteIndex {
        *self
    }

    #[inline]
    fn as_str(&self) -> Option<&str> {
        None
    }
    #[inline]
    fn as_usize(&self) -> Option<&usize> {
        Some(self)
    }
}

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
fn err_null_idx_col<I: ColIdx>(idx: I) -> TryGetError {
    TryGetError::Null(format!("{idx:?}"))
}

/// The error for column `idx` the driver could not read: a value the Rust
/// type cannot hold is a [`DbErr::Decode`] naming the column.
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
fn err_read_idx_col<I: ColIdx>(idx: I, err: driver::Error) -> TryGetError {
    match err {
        driver::Error::ColumnDecode { source, .. } => DbErr::Decode {
            column: idx
                .as_str()
                .map_or_else(|| format!("{idx:?}"), str::to_owned),
            source: Arc::from(source),
        },
        err => driver_error_to_query_err(err),
    }
    .into()
}

mod impls;
mod traits;

pub use traits::*;
