use crate::Value;
use sql_builder::Nullable;
use std::fmt::Debug;

pub use ActiveValue::{NotSet, Set, Unchanged};

/// The state of a field in an [ActiveModel][crate::ActiveModelTrait].
///
/// There are three possible states represented by three enum variants:
///
/// - [Set] - a value that's explicitly set by the application and sent to the database.
/// - [Unchanged] - an existing, unchanged value from the database.
/// - [NotSet] - an undefined value (nothing is sent to the database).
///
/// The difference between these states is useful
/// when constructing `INSERT` and `UPDATE` SQL statements.
/// It's also useful for knowing which fields have changed in a record.
#[derive(Clone, Debug)]
pub enum ActiveValue<V>
where
    V: Into<Value>,
{
    /// A [Value] that's explicitly set by the application and sent to the database.
    ///
    /// Use this to insert or set a specific value.
    ///
    /// When editing an existing value, you can use [set_ne][ActiveValue::set_ne]
    /// to preserve the [Unchanged] state when the new value is the same as the old one.
    /// Then you can meaningfully use methods like [crate::ActiveModelTrait::is_changed].
    Set(V),
    /// An existing, unchanged [Value] from the database.
    ///
    /// You get these when you query an existing [Model][crate::ModelTrait]
    /// from the database and convert it into an [ActiveModel][crate::ActiveModelTrait].
    ///
    /// When you edit it, you can use [set_ne][ActiveValue::set_ne]
    /// to preserve this "unchanged" state if the new value is the same as the old one.
    /// Then you can meaningfully use methods like [crate::ActiveModelTrait::is_changed].
    Unchanged(V),
    /// An undefined [Value]. Nothing is sent to the database.
    ///
    /// When you create a new [ActiveModel][crate::ActiveModelTrait],
    /// its fields are [NotSet][ActiveValue::NotSet] by default.
    ///
    /// This can be useful when:
    ///
    /// - You insert a new record and want the database to generate a default value (e.g., an id).
    /// - In an `UPDATE` statement, you don't want to update some field.
    NotSet,
}

/// Any type that can be converted into an [ActiveValue]
pub trait IntoActiveValue<V>
where
    V: Into<Value>,
{
    /// Method to perform the conversion
    fn into_active_value(self) -> ActiveValue<V>;
}

impl<V> IntoActiveValue<V> for Option<V>
where
    V: IntoActiveValue<V> + Into<Value> + Nullable,
{
    fn into_active_value(self) -> ActiveValue<V> {
        match self {
            Some(value) => Set(value),
            None => NotSet,
        }
    }
}

impl<V> IntoActiveValue<Option<V>> for Option<Option<V>>
where
    V: IntoActiveValue<V> + Into<Value> + Nullable,
{
    fn into_active_value(self) -> ActiveValue<Option<V>> {
        match self {
            Some(value) => Set(value),
            None => NotSet,
        }
    }
}

macro_rules! impl_into_active_value {
    ($ty: ty) => {
        impl IntoActiveValue<$ty> for $ty {
            fn into_active_value(self) -> ActiveValue<$ty> {
                Set(self)
            }
        }
    };
}

impl_into_active_value!(bool);
impl_into_active_value!(i8);
impl_into_active_value!(i16);
impl_into_active_value!(i32);
impl_into_active_value!(i64);
impl_into_active_value!(u8);
impl_into_active_value!(u16);
impl_into_active_value!(u32);
impl_into_active_value!(u64);
impl_into_active_value!(f32);
impl_into_active_value!(f64);
impl_into_active_value!(&'static str);
impl_into_active_value!(String);
impl_into_active_value!(Vec<u8>);

impl_into_active_value!(crate::prelude::Json);

impl_into_active_value!(crate::prelude::Date);

impl_into_active_value!(crate::prelude::Time);

impl_into_active_value!(crate::prelude::DateTime);

impl_into_active_value!(crate::prelude::DateTimeWithTimeZone);

impl_into_active_value!(crate::prelude::DateTimeUtc);

impl_into_active_value!(crate::prelude::DateTimeLocal);

impl_into_active_value!(crate::prelude::Decimal);

impl_into_active_value!(crate::prelude::Uuid);

impl<V> Default for ActiveValue<V>
where
    V: Into<Value>,
{
    /// Create an [ActiveValue::NotSet]
    fn default() -> Self {
        Self::NotSet
    }
}

impl<V> ActiveValue<V>
where
    V: Into<Value>,
{
    /// Create an [ActiveValue::Set]
    pub fn set(value: V) -> Self {
        Self::Set(value)
    }

    /// Check if the [ActiveValue] is [ActiveValue::Set]
    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    /// Check if the [ActiveValue] is [ActiveValue::Set] and that the inner value
    /// matches a given predicate.
    pub fn is_set_and(&self, f: impl FnOnce(&V) -> bool) -> bool {
        matches!(self, Self::Set(v) if f(v))
    }

    /// Create an [ActiveValue::Unchanged]
    pub fn unchanged(value: V) -> Self {
        Self::Unchanged(value)
    }

    /// Check if the [ActiveValue] is [ActiveValue::Unchanged]
    pub fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged(_))
    }

    /// Check if the [ActiveValue] is [ActiveValue::Unchanged] and that the inner
    /// value matches a given predicate.
    pub fn is_unchanged_and(&self, f: impl FnOnce(&V) -> bool) -> bool {
        matches!(self, Self::Unchanged(v) if f(v))
    }

    /// Create an [ActiveValue::NotSet]
    pub fn not_set() -> Self {
        Self::default()
    }

    /// Check if the [ActiveValue] is [ActiveValue::NotSet]
    pub fn is_not_set(&self) -> bool {
        matches!(self, Self::NotSet)
    }

    /// Take ownership of the inner value, also setting self to `NotSet`
    pub fn take(&mut self) -> Option<V> {
        match std::mem::take(self) {
            ActiveValue::Set(value) | ActiveValue::Unchanged(value) => Some(value),
            ActiveValue::NotSet => None,
        }
    }

    /// Get an owned value of the [ActiveValue]
    ///
    /// # Panics
    ///
    /// Panics if it is [ActiveValue::NotSet]
    pub fn unwrap(self) -> V {
        match self {
            ActiveValue::Set(value) | ActiveValue::Unchanged(value) => value,
            ActiveValue::NotSet => panic!("Cannot unwrap ActiveValue::NotSet"),
        }
    }

    /// Take ownership of the inner value, consuming self
    pub fn into_value(self) -> Option<Value> {
        match self {
            ActiveValue::Set(value) | ActiveValue::Unchanged(value) => Some(value.into()),
            ActiveValue::NotSet => None,
        }
    }

    /// Wrap the [Value] into a `ActiveValue<Value>`
    pub fn into_wrapped_value(self) -> ActiveValue<Value> {
        match self {
            Self::Set(value) => ActiveValue::set(value.into()),
            Self::Unchanged(value) => ActiveValue::unchanged(value.into()),
            Self::NotSet => ActiveValue::not_set(),
        }
    }

    /// Reset the value from [ActiveValue::Unchanged] to [ActiveValue::Set],
    /// leaving [ActiveValue::NotSet] untouched.
    pub fn reset(&mut self) {
        *self = match self.take() {
            Some(value) => ActiveValue::Set(value),
            None => ActiveValue::NotSet,
        };
    }

    /// `Set(value)`, except when [`self.is_unchanged()`][ActiveValue#method.is_unchanged]
    /// and `value` equals the current [Unchanged][ActiveValue::Unchanged] value.
    ///
    /// This is useful when you have an [Unchanged][ActiveValue::Unchanged] value from the database,
    /// then update it using this method,
    /// and then use [`.is_unchanged()`][ActiveValue#method.is_unchanged] to see whether it has *actually* changed.
    ///
    /// The same nice effect applies to the entire `ActiveModel`.
    /// You can now meaningfully use [crate::ActiveModelTrait::is_changed][crate::ActiveModelTrait#method.is_changed]
    /// to see whether are any changes that need to be saved to the database.
    pub fn set_ne(&mut self, value: V)
    where
        V: PartialEq,
    {
        match self {
            ActiveValue::Unchanged(current) if &value == current => {}
            _ => *self = ActiveValue::Set(value),
        }
    }

    /// Alias for [`ActiveValue::set_ne`]. Kept for compatibility.
    pub fn set_if_not_equals(&mut self, value: V)
    where
        V: PartialEq,
    {
        self.set_ne(value);
    }

    /// `Set(value)`, except when the current value is
    /// [Unchanged][ActiveValue::Unchanged] and equals `value`, or is
    /// [Set][ActiveValue::Set] and `f(&value)` is false.
    ///
    /// This is useful in the same situations as [`ActiveValue::set_ne`] as
    /// well as when you want to leave an existing [Set][ActiveValue::Set] value alone
    /// depending on a condition, such as ensuring a `None` value never replaced an
    /// existing `Some` value. This can come up when trying to merge two [ActiveValue]s.
    pub fn set_ne_and(&mut self, value: V, f: impl FnOnce(&V) -> bool)
    where
        V: PartialEq,
    {
        match self {
            ActiveValue::Unchanged(current) if &value == current => {}
            ActiveValue::Set(_) if !f(&value) => {}
            _ => *self = ActiveValue::Set(value),
        }
    }

    /// Alias for [`ActiveValue::set_ne_and`]. Kept for compatibility.
    pub fn set_if_not_equals_and(&mut self, value: V, f: impl FnOnce(&V) -> bool)
    where
        V: PartialEq,
    {
        self.set_ne_and(value, f);
    }

    /// `Set(value)` if [`self.is_not_set()`][ActiveValue#method.is_not_set], no-op otherwise.
    /// Similar to "null coalescing" or [Option#method.get_or_insert], but without
    /// returning the inner value if it is set/unchanged.
    pub fn set_if_unset(&mut self, value: V) {
        if let ActiveValue::NotSet = self {
            *self = ActiveValue::Set(value);
        }
    }

    /// `Set(f())` if [`self.is_not_set()`][ActiveValue#method.is_not_set], no-op otherwise.
    /// Similar to "null coalescing" or [Option#method.get_or_insert_with], but without
    /// returning the inner value if it is set/unchanged.
    ///
    /// This can be useful if the value you want to replace it with is expensive to compute,
    /// or has side-effects which need to be ran (like logging).
    pub fn set_if_unset_with(&mut self, f: impl FnOnce() -> V) {
        if let ActiveValue::NotSet = self {
            *self = ActiveValue::Set(f());
        }
    }

    /// `Set(V::default())` if [`self.is_not_set()`][ActiveValue#method.is_not_set], no-op otherwise.
    /// Similar to "null coalescing" or [Option#method.get_or_insert_default], but without
    /// returning the inner value if it is set/unchanged.
    ///
    /// Convenient shorthand for `set_if_unset(Default::default())`.
    pub fn set_if_unset_default(&mut self)
    where
        V: Default,
    {
        if let ActiveValue::NotSet = self {
            *self = ActiveValue::Set(V::default());
        }
    }

    /// Get the inner value, unless `self` is [NotSet][ActiveValue::NotSet].
    ///
    /// There's also a panicking version: [ActiveValue::as_ref].
    pub fn try_as_ref(&self) -> Option<&V> {
        match self {
            ActiveValue::Set(value) | ActiveValue::Unchanged(value) => Some(value),
            ActiveValue::NotSet => None,
        }
    }
}

impl<V> ActiveValue<Option<V>>
where
    V: Into<Value> + Nullable,
{
    /// Flatten an `&ActiveValue<Option<V>>` into an `Option<&V>`, folding
    /// [NotSet][ActiveValue::NotSet] together with the inner [None][Option::None],
    /// and [Set][ActiveValue::Set] / [Unchanged][ActiveValue::Unchanged] together
    /// with the inner [Some][Option::Some].
    ///
    /// Shorthand for `self.try_as_ref().and_then(Option::as_ref)`. For the owned
    /// version, see [ActiveValue::into_option].
    pub fn as_option(&self) -> Option<&V> {
        self.try_as_ref().and_then(Option::as_ref)
    }

    /// Flatten an `ActiveValue<Option<V>>` into an `Option<V>`, folding
    /// [NotSet][ActiveValue::NotSet] together with the inner [None][Option::None],
    /// and [Set][ActiveValue::Set] / [Unchanged][ActiveValue::Unchanged] together
    /// with the inner [Some][Option::Some].
    ///
    /// For a borrowing version, see [ActiveValue::as_option].
    pub fn into_option(self) -> Option<V> {
        match self {
            ActiveValue::Set(value) | ActiveValue::Unchanged(value) => value,
            ActiveValue::NotSet => None,
        }
    }
}

impl<V> std::convert::AsRef<V> for ActiveValue<V>
where
    V: Into<Value>,
{
    /// # Panics
    ///
    /// Panics if it is [ActiveValue::NotSet].
    ///
    /// See [ActiveValue::try_as_ref] for a fallible non-panicking version.
    fn as_ref(&self) -> &V {
        match self {
            ActiveValue::Set(value) | ActiveValue::Unchanged(value) => value,
            ActiveValue::NotSet => panic!("Cannot borrow ActiveValue::NotSet"),
        }
    }
}

impl<V> PartialEq for ActiveValue<V>
where
    V: Into<Value> + std::cmp::PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ActiveValue::Set(l), ActiveValue::Set(r))
            | (ActiveValue::Unchanged(l), ActiveValue::Unchanged(r)) => l == r,
            (ActiveValue::NotSet, ActiveValue::NotSet) => true,
            _ => false,
        }
    }
}

impl<V> From<ActiveValue<V>> for ActiveValue<Option<V>>
where
    V: Into<Value> + Nullable,
{
    fn from(value: ActiveValue<V>) -> Self {
        match value {
            ActiveValue::Set(value) => ActiveValue::set(Some(value)),
            ActiveValue::Unchanged(value) => ActiveValue::unchanged(Some(value)),
            ActiveValue::NotSet => ActiveValue::not_set(),
        }
    }
}
