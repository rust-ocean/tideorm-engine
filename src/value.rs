//! Helpers for working with [`Value`]s: the Unix-timestamp column types
//! [`ChronoUnixTimestamp`] and [`ChronoUnixTimestampMillis`], and the placeholder values
//! `ActiveModelTrait::default_values` fills an active model with.
//!
//! `set_from_json` merges a partial JSON object over those placeholders so that every
//! column deserializes.
//!
//! The derive writes `(&model.field).default_value()` for each field and lets method
//! resolution choose the trait: the two traits implemented on `ActiveValue<_>` itself
//! match first, a plain column with a placeholder ([`DefaultActiveValue`]) or an
//! `Option` column ([`DefaultActiveValueNone`]); a column matching neither falls through
//! to [`DefaultActiveValueNotSet`], implemented one reference deeper. The bounds keep
//! the first two apart, since an `Option<_>` is never [`Nullable`].

use crate::sql_builder::{Nullable, ValueType};
use crate::{ActiveValue, Value};

mod timestamp;
use timestamp::*;

mod with_chrono;
pub use with_chrono::*;

/// The placeholder of a column whose type has one.
pub trait DefaultActiveValue {
    /// `Set` to the type's zero value (`0`, `false`, an empty string, ..), or `NotSet`
    /// when that value does not convert back into the type.
    fn default_value(&self) -> Self;
}

/// The placeholder of an `Option` column.
pub trait DefaultActiveValueNone {
    /// `Set(None)`.
    fn default_value(&self) -> Self;
}

/// The placeholder of any other column: none.
pub trait DefaultActiveValueNotSet {
    /// The active value the placeholder is.
    type Value;

    /// `NotSet`.
    fn default_value(&self) -> Self::Value;
}

impl<V> DefaultActiveValue for ActiveValue<V>
where
    V: Into<Value> + ValueType + Nullable,
{
    fn default_value(&self) -> Self {
        match V::try_from(V::null().dummy_value()) {
            Ok(v) => ActiveValue::Set(v),
            Err(_) => ActiveValue::NotSet,
        }
    }
}

impl<V> DefaultActiveValueNone for ActiveValue<Option<V>>
where
    V: Into<Value> + Nullable,
{
    fn default_value(&self) -> Self {
        ActiveValue::Set(None)
    }
}

impl<V> DefaultActiveValueNotSet for &ActiveValue<V>
where
    V: Into<Value>,
{
    type Value = ActiveValue<V>;

    fn default_value(&self) -> Self::Value {
        ActiveValue::NotSet
    }
}
