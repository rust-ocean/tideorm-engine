//! Reading several columns at once, JSON and array columns, and keys
//! from a last-insert id.

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
use super::impls::column;
use super::*;

/// Decode multiple column values out of a [`QueryResult`] into a tuple.
///
/// The tuple impls let you write things like
/// `Select::into_tuple::<(i32, String)>()` for queries with a small number of
/// scalar outputs.
pub trait TryGetableMany: Sized {
    /// Decode by column name; `cols` names each tuple position.
    fn try_get_many(res: &QueryResult, cols: &[String]) -> Result<Self, TryGetError>;

    /// Decode positionally, in the order columns appear in the SELECT list.
    fn try_get_many_by_index(res: &QueryResult) -> Result<Self, TryGetError>;
}

impl<T> TryGetableMany for T
where
    T: TryGetable,
{
    fn try_get_many(res: &QueryResult, cols: &[String]) -> Result<Self, TryGetError> {
        try_get_many_with_slice_len_of(1, cols)?;
        T::try_get_by(res, cols[0].as_str())
    }

    fn try_get_many_by_index(res: &QueryResult) -> Result<Self, TryGetError> {
        T::try_get_by_index(res, 0)
    }
}

impl<T> TryGetableMany for (T,)
where
    T: TryGetableMany,
{
    fn try_get_many(res: &QueryResult, cols: &[String]) -> Result<Self, TryGetError> {
        T::try_get_many(res, cols).map(|r| (r,))
    }

    fn try_get_many_by_index(res: &QueryResult) -> Result<Self, TryGetError> {
        T::try_get_many_by_index(res).map(|r| (r,))
    }
}

macro_rules! impl_try_get_many {
    ( $LEN:expr, $($T:ident : $N:expr),+ $(,)? ) => {
        impl< $($T),+ > TryGetableMany for ( $($T),+ )
        where
            $($T: TryGetable),+
        {
            fn try_get_many(res: &QueryResult, cols: &[String]) -> Result<Self, TryGetError> {
                try_get_many_with_slice_len_of($LEN, cols)?;
                Ok((
                    $($T::try_get_by(res, cols[$N].as_str())?),+
                ))
            }

            fn try_get_many_by_index(res: &QueryResult) -> Result<Self, TryGetError> {
                Ok((
                    $($T::try_get_by_index(res, $N)?),+
                ))
            }
        }
    };
}

#[rustfmt::skip]
mod impl_try_get_many {
    use super::*;

    impl_try_get_many!( 2, T0:0, T1:1);
    impl_try_get_many!( 3, T0:0, T1:1, T2:2);
    impl_try_get_many!( 4, T0:0, T1:1, T2:2, T3:3);
    impl_try_get_many!( 5, T0:0, T1:1, T2:2, T3:3, T4:4);
    impl_try_get_many!( 6, T0:0, T1:1, T2:2, T3:3, T4:4, T5:5);
    impl_try_get_many!( 7, T0:0, T1:1, T2:2, T3:3, T4:4, T5:5, T6:6);
    impl_try_get_many!( 8, T0:0, T1:1, T2:2, T3:3, T4:4, T5:5, T6:6, T7:7);
    impl_try_get_many!( 9, T0:0, T1:1, T2:2, T3:3, T4:4, T5:5, T6:6, T7:7, T8:8);
    impl_try_get_many!(10, T0:0, T1:1, T2:2, T3:3, T4:4, T5:5, T6:6, T7:7, T8:8, T9:9);
    impl_try_get_many!(11, T0:0, T1:1, T2:2, T3:3, T4:4, T5:5, T6:6, T7:7, T8:8, T9:9, T10:10);
    impl_try_get_many!(12, T0:0, T1:1, T2:2, T3:3, T4:4, T5:5, T6:6, T7:7, T8:8, T9:9, T10:10, T11:11);
}

fn try_get_many_with_slice_len_of(len: usize, cols: &[String]) -> Result<(), TryGetError> {
    if cols.len() < len {
        Err(type_err(format!(
            "Expect {} column names supplied but got slice of length {}",
            len,
            cols.len()
        ))
        .into())
    } else {
        Ok(())
    }
}

/// An interface to get an array of values from the query result.
/// A type can only implement `ActiveEnum` or `TryGetableFromJson`, but not both.
/// A blanket impl is provided for `TryGetableFromJson`, while the impl for `ActiveEnum`
/// is provided by the `DeriveActiveEnum` macro. So as an end user you won't normally
/// touch this trait.
pub trait TryGetableArray: Sized {
    /// Just a delegate
    fn try_get_by<I: ColIdx>(res: &QueryResult, index: I) -> Result<Vec<Self>, TryGetError>;
}

impl<T> TryGetable for Vec<T>
where
    T: TryGetableArray,
{
    fn try_get_by<I: ColIdx>(res: &QueryResult, index: I) -> Result<Self, TryGetError> {
        T::try_get_by(res, index)
    }
}

// TryGetableFromJson //

/// An interface to get a JSON from the query result
pub trait TryGetableFromJson: Sized
where
    for<'de> Self: serde::Deserialize<'de>,
{
    /// Get a JSON from the query result with prefixed column name
    #[allow(unused_variables, unreachable_code)]
    fn try_get_from_json<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
        on_row!(&res.row, idx, |row, index| {
            column::<_, driver::types::Json<Self>, _>(row, index, idx).map(|json| json.0)
        })
    }

    /// Decode a JSON array into a `Vec<Self>`.
    fn from_json_vec(value: serde_json::Value) -> Result<Vec<Self>, TryGetError> {
        match value {
            serde_json::Value::Array(values) => {
                let mut res = Vec::new();
                for item in values {
                    res.push(serde_json::from_value(item).map_err(crate::error::json_err)?);
                }
                Ok(res)
            }
            _ => Err(TryGetError::DbErr(DbErr::Json(
                "Value is not an Array".to_owned(),
            ))),
        }
    }
}

impl<T> TryGetable for T
where
    T: TryGetableFromJson,
{
    fn try_get_by<I: ColIdx>(res: &QueryResult, index: I) -> Result<Self, TryGetError> {
        T::try_get_from_json(res, index)
    }
}

impl<T> TryGetableArray for T
where
    T: TryGetableFromJson,
{
    fn try_get_by<I: ColIdx>(res: &QueryResult, index: I) -> Result<Vec<T>, TryGetError> {
        T::from_json_vec(serde_json::Value::try_get_by(res, index)?)
    }
}

// TryFromU64 //
/// Build a value of `Self` from a `u64` last-insert-id returned by the
/// database after an auto-increment `INSERT`.
///
/// Auto-generated for primitive integer primary keys; failures (e.g. for
/// non-integer or composite keys) yield [`DbErr::ConvertFromU64`](crate::DbErr::ConvertFromU64).
pub trait TryFromU64: Sized {
    /// Build `Self` from the raw `u64` returned by the driver.
    fn try_from_u64(n: u64) -> Result<Self, DbErr>;
}

use serde::de::DeserializeOwned;

impl<K, V> TryGetableFromJson for HashMap<K, V>
where
    K: DeserializeOwned + Eq + Hash,
    V: DeserializeOwned,
{
}

impl<K, V> TryGetableFromJson for BTreeMap<K, V>
where
    K: DeserializeOwned + Ord,
    V: DeserializeOwned,
{
}

macro_rules! try_from_u64_err {
    ( $type: ty ) => {
        impl TryFromU64 for $type {
            fn try_from_u64(_: u64) -> Result<Self, DbErr> {
                Err(DbErr::ConvertFromU64(stringify!($type)))
            }
        }
    };

    ( $($gen_type: ident),* ) => {
        impl<$( $gen_type, )*> TryFromU64 for ($( $gen_type, )*)
        where
            $( $gen_type: TryFromU64, )*
        {
            fn try_from_u64(_: u64) -> Result<Self, DbErr> {
                Err(DbErr::ConvertFromU64(stringify!($($gen_type,)*)))
            }
        }
    };
}

#[rustfmt::skip]
mod try_from_u64_err {
    use super::*;

    try_from_u64_err!(T0, T1);
    try_from_u64_err!(T0, T1, T2);
    try_from_u64_err!(T0, T1, T2, T3);
    try_from_u64_err!(T0, T1, T2, T3, T4);
    try_from_u64_err!(T0, T1, T2, T3, T4, T5);
    try_from_u64_err!(T0, T1, T2, T3, T4, T5, T6);
    try_from_u64_err!(T0, T1, T2, T3, T4, T5, T6, T7);
    try_from_u64_err!(T0, T1, T2, T3, T4, T5, T6, T7, T8);
    try_from_u64_err!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9);
    try_from_u64_err!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
    try_from_u64_err!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
}

macro_rules! try_from_u64_numeric {
    ( $type: ty ) => {
        impl TryFromU64 for $type {
            fn try_from_u64(n: u64) -> Result<Self, DbErr> {
                use std::convert::TryInto;
                n.try_into().map_err(|e| DbErr::TryIntoErr {
                    from: stringify!(u64),
                    into: stringify!($type),
                    source: Arc::new(e),
                })
            }
        }
    };
}

try_from_u64_numeric!(i8);
try_from_u64_numeric!(i16);
try_from_u64_numeric!(i32);
try_from_u64_numeric!(i64);
try_from_u64_numeric!(u8);
try_from_u64_numeric!(u16);
try_from_u64_numeric!(u32);
try_from_u64_numeric!(u64);

macro_rules! try_from_u64_string {
    ( $type: ty ) => {
        impl TryFromU64 for $type {
            fn try_from_u64(n: u64) -> Result<Self, DbErr> {
                Ok(n.to_string())
            }
        }
    };
}

try_from_u64_string!(String);

try_from_u64_err!(bool);
try_from_u64_err!(f32);
try_from_u64_err!(f64);
try_from_u64_err!(Vec<u8>);

try_from_u64_err!(serde_json::Value);

try_from_u64_err!(chrono::NaiveDate);

try_from_u64_err!(chrono::NaiveTime);

try_from_u64_err!(chrono::NaiveDateTime);

try_from_u64_err!(chrono::DateTime<chrono::FixedOffset>);

try_from_u64_err!(chrono::DateTime<chrono::Utc>);

try_from_u64_err!(chrono::DateTime<chrono::Local>);

try_from_u64_err!(rust_decimal::Decimal);

try_from_u64_err!(uuid::Uuid);
