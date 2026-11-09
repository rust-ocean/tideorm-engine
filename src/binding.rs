// Fallible driver argument encoding for TideORM's value features. A value the
// backend cannot hold is an encoding error, never a panic, and the arguments
// built so far are dropped with it.
use driver::{Arguments, Encode, Type};
use sql_builder::{OptionEnum, Value};

/// Binds every value with `bind`, which adds one to the arguments.
fn bind_all<A: Arguments>(
    values: &[Value],
    bind: fn(&mut A, &Value) -> Result<(), driver::Error>,
) -> Result<A, driver::Error> {
    let mut args = A::default();
    args.reserve(values.len(), 0);
    for value in values {
        bind(&mut args, value)?;
    }
    Ok(args)
}

fn add<'t, A, T>(args: &mut A, value: T) -> Result<(), driver::Error>
where
    A: Arguments,
    T: Encode<'t, A::Database> + Type<A::Database>,
{
    args.add(value).map_err(driver::Error::Encode)
}

/// `match value`: the backend's own `$arm`s bind the variants backends
/// store differently, and every other variant binds as itself.
macro_rules! bind_with {
    ($args:ident, $value:ident { $($arm:tt)* }) => {
        match $value {
            Value::Bool(v) => add($args, *v),
            Value::SmallInt(v) => add($args, *v),
            Value::Int(v) => add($args, *v),
            Value::BigInt(v) => add($args, *v),
            Value::Float(v) => add($args, *v),
            Value::Double(v) => add($args, *v),
            Value::String(v) => add($args, v.as_deref()),
            Value::Enum(v) => add($args, enum_text(v)),
            Value::Char(v) => add($args, v.map(String::from)),
            Value::Bytes(v) => add($args, v.as_deref()),
            Value::ChronoDate(v) => add($args, *v),
            Value::ChronoTime(v) => add($args, *v),
            Value::ChronoDateTime(v) => add($args, *v),
            Value::ChronoDateTimeUtc(v) => add($args, *v),
            Value::ChronoDateTimeLocal(v) => add($args, *v),
            Value::ChronoDateTimeWithTimeZone(v) => add($args, *v),
            Value::SmallUnsigned(v) => add($args, *v),
            Value::Unsigned(v) => add($args, *v),
            Value::BigUnsigned(v) => add($args, *v),
            Value::Uuid(v) => add($args, *v),
            Value::Json(v) => add($args, v.as_deref()),
            $($arm)*
        }
    };
}

fn encode_error(error: impl Into<driver::error::BoxDynError>) -> driver::Error {
    driver::Error::Encode(error.into())
}

/// The variant's name, which every backend binds as text.
fn enum_text(value: &OptionEnum) -> Option<&str> {
    match value {
        OptionEnum::Some(value) => Some(&*value.value),
        OptionEnum::None(_) => None,
    }
}

#[cfg(feature = "postgres")]
pub(crate) fn postgres(values: &[Value]) -> Result<driver::postgres::PgArguments, driver::Error> {
    // The driver's `i8` is the one-byte `"char"` and its `u8` is `BYTEA`'s
    // element, so both are bound as the `SMALLINT` that holds them.
    bind_all(values, |args, value| {
        bind_with!(args, value {
            Value::TinyInt(v) => add(args, v.map(i16::from)),
            Value::TinyUnsigned(v) => add(args, v.map(i16::from)),
            Value::Decimal(v) => add(args, *v),
            Value::Array(ty, v) => postgres_array(args, ty, v.as_deref().map(Vec::as_slice)),
        })
    })
}

/// Binds an array as the PostgreSQL array of its declared element type.
/// Every element must be a value of that type: a mismatched one is an
/// encoding error, never bound as if it were of that type.
#[cfg(feature = "postgres")]
fn postgres_array(
    args: &mut driver::postgres::PgArguments,
    ty: &sql_builder::ArrayType,
    values: Option<&[Value]>,
) -> Result<(), driver::Error> {
    use sql_builder::ArrayType;
    use sql_builder::prelude::{
        DateTime, Decimal, FixedOffset, Json, Local, NaiveDate, NaiveDateTime, NaiveTime, Utc, Uuid,
    };

    fn elements<T: sql_builder::ValueType>(
        values: Option<&[Value]>,
    ) -> Result<Option<Vec<T>>, driver::Error> {
        values
            .map(|values| {
                values
                    .iter()
                    .map(|value| {
                        sql_builder::value::postgres_array::try_array_element::<T>(value.clone())
                            .map_err(encode_error)
                    })
                    .collect()
            })
            .transpose()
    }

    fn widened<T: sql_builder::ValueType, U>(
        values: Option<&[Value]>,
        widen: fn(T) -> U,
    ) -> Result<Option<Vec<U>>, driver::Error> {
        Ok(elements(values)?.map(|values| values.into_iter().map(widen).collect()))
    }

    match ty {
        ArrayType::Bool => add(args, elements::<bool>(values)?),
        ArrayType::TinyInt => add(args, widened::<i8, _>(values, i16::from)?),
        ArrayType::SmallInt => add(args, elements::<i16>(values)?),
        ArrayType::Int => add(args, elements::<i32>(values)?),
        ArrayType::BigInt => add(args, elements::<i64>(values)?),
        ArrayType::TinyUnsigned => add(args, widened::<u8, _>(values, i16::from)?),
        ArrayType::SmallUnsigned => add(args, widened::<u16, _>(values, i32::from)?),
        ArrayType::Unsigned => add(args, widened::<u32, _>(values, i64::from)?),
        ArrayType::BigUnsigned => {
            let values: Option<Vec<i64>> = elements::<u64>(values)?
                .map(|values| values.into_iter().map(i64::try_from).collect())
                .transpose()
                .map_err(encode_error)?;
            add(args, values)
        }
        ArrayType::Float => add(args, elements::<f32>(values)?),
        ArrayType::Double => add(args, elements::<f64>(values)?),
        ArrayType::String => add(args, elements::<String>(values)?),
        ArrayType::Enum(_) => {
            let values = values
                .map(|values| {
                    values
                        .iter()
                        .map(|value| {
                            sql_builder::value::postgres_array::enum_array_element(value)
                                .map(|value| &*value.value)
                                .map_err(encode_error)
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?;
            add(args, values)
        }
        ArrayType::Char => add(args, widened::<char, _>(values, String::from)?),
        ArrayType::Bytes => add(args, elements::<Vec<u8>>(values)?),
        ArrayType::ChronoDate => add(args, elements::<NaiveDate>(values)?),
        ArrayType::ChronoTime => add(args, elements::<NaiveTime>(values)?),
        ArrayType::ChronoDateTime => add(args, elements::<NaiveDateTime>(values)?),
        ArrayType::ChronoDateTimeUtc => add(args, elements::<DateTime<Utc>>(values)?),
        ArrayType::ChronoDateTimeLocal => add(args, elements::<DateTime<Local>>(values)?),
        ArrayType::ChronoDateTimeWithTimeZone => {
            add(args, elements::<DateTime<FixedOffset>>(values)?)
        }
        ArrayType::Uuid => add(args, elements::<Uuid>(values)?),
        ArrayType::Decimal => add(args, elements::<Decimal>(values)?),
        ArrayType::Json => add(args, elements::<Json>(values)?),
    }
}

#[cfg(feature = "mysql")]
pub(crate) fn mysql(values: &[Value]) -> Result<driver::mysql::MySqlArguments, driver::Error> {
    bind_all(values, |args, value| {
        bind_with!(args, value {
            Value::TinyInt(v) => add(args, *v),
            Value::TinyUnsigned(v) => add(args, *v),
            Value::Decimal(v) => add(args, *v),
            Value::Array(..) => Err(encode_error("Mysql doesn't support array arguments")),
        })
    })
}

#[cfg(feature = "sqlite")]
pub(crate) fn sqlite(values: &[Value]) -> Result<driver::sqlite::SqliteArguments, driver::Error> {
    bind_all(values, |args, value| {
        bind_with!(args, value {
            Value::TinyInt(v) => add(args, *v),
            Value::TinyUnsigned(v) => add(args, *v),
            // SQLite has no decimal type; the text keeps every digit.
            Value::Decimal(v) => add(args, v.map(|v| v.to_string())),
            Value::Array(..) => Err(encode_error("Sqlite doesn't support array arguments")),
        })
    })
}
