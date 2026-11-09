use crate::{ColIdx, ColumnDef, DbErr, Iterable, QueryResult, TryFromU64, TryGetError, TryGetable};
use sql_builder::{DynIden, Expr, ExprTrait, Nullable, SimpleExpr, Value, ValueType};

/// A Rust representation of enum defined in database.
///
/// Implement it by hand or with the `DeriveActiveEnum` derive macro.
pub trait ActiveEnum: Sized + Iterable {
    /// Define the Rust type that each enum variant corresponds.
    type Value: ActiveEnumValue;

    /// This has no purpose. It will be removed in the next major version.
    type ValueVec;

    /// Get the name of enum
    fn name() -> DynIden;

    /// Convert enum variant into the corresponding value.
    fn to_value(&self) -> Self::Value;

    /// Try to convert the corresponding value into enum variant.
    fn try_from_value(v: &Self::Value) -> Result<Self, DbErr>;

    /// Get the database column definition of this active enum.
    fn db_type() -> ColumnDef;

    /// Convert an owned enum variant into the corresponding value.
    fn into_value(self) -> Self::Value {
        Self::to_value(&self)
    }

    /// Construct a enum expression with casting
    fn as_enum(&self) -> SimpleExpr {
        let value: Value = Self::to_value(self).into();
        match value {
            Value::Enum(_) => Expr::val(value),
            _ => Expr::val(value).as_enum(Self::name()),
        }
    }

    /// Get the name of all enum variants
    fn values() -> Vec<Self::Value> {
        Self::ALL.iter().map(Self::to_value).collect()
    }
}

/// The Rust Value backing ActiveEnums
pub trait ActiveEnumValue: Into<Value> + ValueType + Nullable + TryGetable {
    /// For getting an array of enum. Postgres only
    fn try_get_vec_by<I: ColIdx>(res: &QueryResult, index: I) -> Result<Vec<Self>, TryGetError>;
}

macro_rules! impl_active_enum_value {
    ($type:ident) => {
        impl ActiveEnumValue for $type {
            fn try_get_vec_by<I: ColIdx>(
                _res: &QueryResult,
                _index: I,
            ) -> Result<Vec<Self>, TryGetError> {
                Err(TryGetError::DbErr(DbErr::BackendNotSupported {
                    db: "Postgres",
                    ctx: "ActiveEnumValue::try_get_vec_by",
                }))
            }
        }
    };
}

macro_rules! impl_active_enum_value_with_pg_array {
    ($type:ident) => {
        impl ActiveEnumValue for $type {
            fn try_get_vec_by<I: ColIdx>(
                _res: &QueryResult,
                _index: I,
            ) -> Result<Vec<Self>, TryGetError> {
                #[cfg(feature = "postgres")]
                {
                    <Vec<Self>>::try_get_by(_res, _index)
                }
                #[cfg(not(feature = "postgres"))]
                Err(TryGetError::DbErr(DbErr::BackendNotSupported {
                    db: "Postgres",
                    ctx: "ActiveEnumValue::try_get_vec_by (the `postgres` feature is not enabled)",
                }))
            }
        }
    };
}

impl_active_enum_value!(u8);
impl_active_enum_value!(u16);
impl_active_enum_value!(u32);
impl_active_enum_value!(u64);
impl_active_enum_value_with_pg_array!(String);
impl_active_enum_value_with_pg_array!(i8);
impl_active_enum_value_with_pg_array!(i16);
impl_active_enum_value_with_pg_array!(i32);
impl_active_enum_value_with_pg_array!(i64);

impl TryGetable for sql_builder::Enum {
    fn try_get_by<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
        let value: String = <String as TryGetable>::try_get_by(res, idx)?;
        Ok(Self {
            // `DeriveActiveEnum` overwrites `type_name` when constructing values for queries, but we
            // can't reliably recover the enum type name from `QueryResult`. Keeping it empty may
            // still cause issues if this value is later reused to build SQL (e.g. missing casts).
            type_name: "".into(),
            value: value.into(),
        })
    }
}

impl ActiveEnumValue for sql_builder::Enum {
    fn try_get_vec_by<I: ColIdx>(_res: &QueryResult, _index: I) -> Result<Vec<Self>, TryGetError> {
        #[cfg(feature = "postgres")]
        {
            let values: Vec<String> = <Vec<String> as TryGetable>::try_get_by(_res, _index)?;
            Ok(values
                .into_iter()
                .map(|value| Self {
                    // See comment in `TryGetable for sql_builder::Enum` about empty `type_name`.
                    type_name: "".into(),
                    value: value.into(),
                })
                .collect())
        }
        #[cfg(not(feature = "postgres"))]
        {
            Err(TryGetError::DbErr(DbErr::BackendNotSupported {
                db: "Postgres",
                ctx: "ActiveEnumValue::try_get_vec_by (the `postgres` feature is not enabled)",
            }))
        }
    }
}

impl<T> TryFromU64 for T
where
    T: ActiveEnum,
{
    fn try_from_u64(_: u64) -> Result<Self, DbErr> {
        Err(DbErr::ConvertFromU64(
            "Fail to construct ActiveEnum from a u64, if your primary key consist of a ActiveEnum field, its auto increment should be set to false.",
        ))
    }
}
