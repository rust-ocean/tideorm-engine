macro_rules! impl_timestamp {
    ($ty:ident, $inner:ident, $from:ident, $to:ident) => {
        impl std::convert::From<$inner> for $ty {
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl std::convert::From<$ty> for crate::Value {
            fn from(source: $ty) -> Self {
                $to(source).into()
            }
        }

        impl crate::TryGetable for $ty {
            fn try_get_by<I: crate::ColIdx>(
                res: &crate::QueryResult,
                idx: I,
            ) -> std::result::Result<Self, TryGetError> {
                let ts = <i64 as crate::TryGetable>::try_get_by(res, idx)?;
                $from(ts).ok_or(TryGetError::DbErr(DbErr::Type(
                    "Failed to convert i64 to timestamp".to_owned(),
                )))
            }
        }

        impl crate::sql_builder::ValueType for $ty {
            fn try_from(
                v: crate::Value,
            ) -> std::result::Result<Self, crate::sql_builder::ValueTypeErr> {
                let ts = <i64 as crate::sql_builder::ValueType>::try_from(v)?;
                $from(ts).ok_or(crate::sql_builder::ValueTypeErr)
            }

            fn type_name() -> std::string::String {
                stringify!($ty).to_owned()
            }

            fn array_type() -> crate::sql_builder::ArrayType {
                <i64 as crate::sql_builder::ValueType>::array_type()
            }

            fn column_type() -> crate::sql_builder::ColumnType {
                <i64 as crate::sql_builder::ValueType>::column_type()
            }
        }

        impl crate::sql_builder::Nullable for $ty {
            fn null() -> crate::Value {
                <i64 as crate::sql_builder::Nullable>::null()
            }
        }

        impl crate::IntoActiveValue<$ty> for $ty {
            fn into_active_value(self) -> crate::ActiveValue<$ty> {
                crate::ActiveValue::Set(self)
            }
        }

        impl Deref for $ty {
            type Target = $inner;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $ty {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }

        impl PartialEq<$inner> for $ty {
            fn eq(&self, other: &$inner) -> bool {
                self.0.eq(other)
            }
        }
    };
}

pub(super) use impl_timestamp;
