//! `TryGetable` for each type a column is read as.

use super::*;

/// Column `index` of `row` as a `T`; SQL `NULL` is [`TryGetError::Null`],
/// naming the column as `idx`.
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub(super) fn column<'r, R, T, Index>(
    row: &'r R,
    index: Index,
    idx: impl ColIdx,
) -> Result<T, TryGetError>
where
    R: Row,
    Index: driver::ColumnIndex<R>,
    T: driver::Type<R::Database> + driver::Decode<'r, R::Database>,
{
    row.try_get::<Option<T>, _>(index)
        .map_err(|e| err_read_idx_col(idx, e))
        .and_then(|value| value.ok_or_else(|| err_null_idx_col(idx)))
}

/// `T` as converted from the `from` a driver returned.
#[cfg(any(feature = "postgres", feature = "sqlite"))]
fn converted<T, F>(from: &'static str, into: &'static str, value: F) -> Result<T, TryGetError>
where
    F: TryInto<T>,
    F::Error: std::error::Error + Send + Sync + 'static,
{
    value.try_into().map_err(|e| {
        DbErr::TryIntoErr {
            from,
            into,
            source: Arc::new(e),
        }
        .into()
    })
}

/// Types every driver decodes as they are.
macro_rules! try_getable_all {
    ( $($type: ty),+ $(,)? ) => {$(
        impl TryGetable for $type {
            #[allow(unused_variables)]
            fn try_get_by<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
                on_row!(&res.row, idx, |row, index| column(row, index, idx))
            }
        }
    )+};
}

try_getable_all!(
    bool,
    i16,
    i32,
    i64,
    f32,
    f64,
    String,
    Vec<u8>,
    serde_json::Value,
    chrono::NaiveDate,
    chrono::NaiveTime,
    chrono::NaiveDateTime,
    chrono::DateTime<chrono::Utc>,
    chrono::DateTime<chrono::Local>,
    chrono::DateTime<chrono::FixedOffset>,
    u16,
    u32,
    u64,
);

/// One-byte integers, which PostgreSQL's driver reads as the `"char"` type
/// (`i8`) or `BYTEA`'s element (`u8`): there each is read from the integer
/// column that holds it.
macro_rules! try_getable_byte {
    ( $($type: ty => $wider: ty),+ $(,)? ) => {$(
        impl TryGetable for $type {
            #[allow(unused_variables)]
            fn try_get_by<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
                match &res.row {
                    #[cfg(feature = "mysql")]
                    QueryResultRow::MySql(row) => column(row, idx.as_mysql_index(), idx),
                    #[cfg(feature = "postgres")]
                    QueryResultRow::Postgres(row) => {
                        let value: $wider = column(row, idx.as_postgres_index(), idx)?;
                        converted(stringify!($wider), stringify!($type), value)
                    }
                    #[cfg(feature = "sqlite")]
                    QueryResultRow::Sqlite(row) => column(row, idx.as_sqlite_index(), idx),
                    #[allow(unreachable_patterns)]
                    _ => unreachable!(),
                }
            }
        }
    )+};
}

try_getable_byte!(i8 => i16, u8 => u16);

use rust_decimal::Decimal;

/// SQLite has no decimal type: a decimal is read as the value is stored,
/// text and integers exactly and a real through `f64`.
impl TryGetable for Decimal {
    #[allow(unused_variables)]
    fn try_get_by<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
        match &res.row {
            #[cfg(feature = "mysql")]
            QueryResultRow::MySql(row) => column(row, idx.as_mysql_index(), idx),
            #[cfg(feature = "postgres")]
            QueryResultRow::Postgres(row) => column(row, idx.as_postgres_index(), idx),
            #[cfg(feature = "sqlite")]
            QueryResultRow::Sqlite(row) => sqlite_decimal(row, idx),
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }
}

#[cfg(feature = "sqlite")]
fn sqlite_decimal<I: ColIdx>(
    row: &driver::sqlite::SqliteRow,
    idx: I,
) -> Result<Decimal, TryGetError> {
    use driver::{Row, TypeInfo, ValueRef};

    let raw = row
        .try_get_raw(idx.as_sqlite_index())
        .map_err(|e| err_read_idx_col(idx, e))?;
    match raw.type_info().name() {
        "TEXT" => {
            let text: String = column(row, idx.as_sqlite_index(), idx)?;
            text.trim().parse().map_err(|e| {
                DbErr::TryIntoErr {
                    from: "String",
                    into: "Decimal",
                    source: Arc::new(e),
                }
                .into()
            })
        }
        "INTEGER" => column(row, idx.as_sqlite_index(), idx).map(|value: i64| value.into()),
        _ => {
            let value: f64 = column(row, idx.as_sqlite_index(), idx)?;
            converted("f64", "Decimal", value)
        }
    }
}

/// A UUID column: `BINARY(16)` bytes or, as MariaDB's `UUID` type returns
/// one, text.
#[allow(unused_variables)]
fn uuid_column<I: ColIdx>(res: &QueryResult, idx: I) -> Result<uuid::Uuid, TryGetError> {
    on_row!(&res.row, idx, |row, index| column(row, index, idx))
}

macro_rules! try_getable_uuid {
    ( $type: ty, $conversion_fn: expr ) => {
        impl TryGetable for $type {
            fn try_get_by<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
                uuid_column(res, idx).map($conversion_fn)
            }
        }
    };
}

try_getable_uuid!(uuid::Uuid, Into::into);
try_getable_uuid!(uuid::fmt::Braced, uuid::Uuid::braced);
try_getable_uuid!(uuid::fmt::Hyphenated, uuid::Uuid::hyphenated);
try_getable_uuid!(uuid::fmt::Simple, uuid::Uuid::simple);
try_getable_uuid!(uuid::fmt::Urn, uuid::Uuid::urn);

/// Arrays, which only PostgreSQL has.
#[cfg(feature = "postgres")]
mod postgres_array {
    use super::*;

    /// The error for a type the row's driver cannot decode.
    #[cfg(any(feature = "mysql", feature = "sqlite"))]
    fn unsupported(type_name: &str, driver: &str) -> TryGetError {
        type_err(format!("{type_name} unsupported by {driver}")).into()
    }

    /// Column `idx` as a PostgreSQL array of `T`, which the other drivers
    /// refuse naming `element`.
    #[allow(unused_variables)]
    fn array_column<T, I>(res: &QueryResult, idx: I, element: &str) -> Result<Vec<T>, TryGetError>
    where
        I: ColIdx,
        Vec<T>: driver::Type<driver::Postgres> + for<'r> driver::Decode<'r, driver::Postgres>,
    {
        match &res.row {
            QueryResultRow::Postgres(row) => column(row, idx.as_postgres_index(), idx),
            #[cfg(feature = "mysql")]
            QueryResultRow::MySql(_) => Err(unsupported(element, "the MySQL driver")),
            #[cfg(feature = "sqlite")]
            QueryResultRow::Sqlite(_) => Err(unsupported(element, "the SQLite driver")),
        }
    }

    macro_rules! try_getable_postgres_array {
        ( $($type: ty),+ $(,)? ) => {$(
            impl TryGetable for Vec<$type> {
                fn try_get_by<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
                    array_column(res, idx, stringify!($type))
                }
            }
        )+};
    }

    try_getable_postgres_array!(
        bool,
        i16,
        i32,
        i64,
        u16,
        u32,
        u64,
        f32,
        f64,
        String,
        Vec<u8>,
        serde_json::Value,
        chrono::NaiveDate,
        chrono::NaiveTime,
        chrono::NaiveDateTime,
        chrono::DateTime<chrono::FixedOffset>,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Local>,
        rust_decimal::Decimal,
    );

    macro_rules! try_getable_postgres_array_uuid {
        ( $type: ty, $conversion_fn: expr ) => {
            impl TryGetable for Vec<$type> {
                fn try_get_by<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
                    array_column::<uuid::Uuid, _>(res, idx, stringify!($type))
                        .map(|uuids| uuids.into_iter().map($conversion_fn).collect())
                }
            }
        };
    }

    try_getable_postgres_array_uuid!(uuid::Uuid, Into::into);
    try_getable_postgres_array_uuid!(uuid::fmt::Braced, uuid::Uuid::braced);
    try_getable_postgres_array_uuid!(uuid::fmt::Hyphenated, uuid::Uuid::hyphenated);
    try_getable_postgres_array_uuid!(uuid::fmt::Simple, uuid::Uuid::simple);
    try_getable_postgres_array_uuid!(uuid::fmt::Urn, uuid::Uuid::urn);

    /// Read as a scalar `i8` is: from an integer array, not the driver's
    /// `"char"[]`.
    impl TryGetable for Vec<i8> {
        fn try_get_by<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
            array_column::<i16, _>(res, idx, "i8")?
                .into_iter()
                .map(|value| converted("i16", "i8", value))
                .collect()
        }
    }
}
