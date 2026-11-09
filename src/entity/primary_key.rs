use super::{ColumnTrait, IdenStatic, Iterable};
use crate::{TryFromU64, TryGetableMany};
use sql_builder::{FromValueTuple, IntoValueTuple};
use std::fmt::Debug;

/// Describes the primary key of an entity: which column variants make it
/// up, the Rust value type (a tuple for composite keys), and whether the
/// database auto-generates it.
///
/// TideORM's model derive implements it from the model's
/// `#[tideorm(primary_key)]` fields.
pub trait PrimaryKeyTrait: IdenStatic + Iterable {
    /// Rust type of the primary-key value. A single column is its own type
    /// (e.g. `i32`); composite keys use a tuple (e.g. `(i32, String)`).
    type ValueType: Sized
        + Send
        + Debug
        + PartialEq
        + IntoValueTuple
        + FromValueTuple
        + TryGetableMany
        + TryFromU64
        + PrimaryKeyArity;

    /// `true` if the database generates the primary key (e.g. `SERIAL` /
    /// `IDENTITY` / `AUTOINCREMENT`). Auto-increment keys can be left as
    /// `NotSet` on insert.
    fn auto_increment() -> bool;
}

/// Conversion between an entity's `PrimaryKey` enum and its `Column` enum.
/// Generated alongside [`PrimaryKeyTrait`] for every entity.
pub trait PrimaryKeyToColumn {
    /// The entity's column enum (i.e. `<Self::Entity as EntityTrait>::Column`).
    type Column: ColumnTrait;

    /// Convert a primary-key variant into the matching column.
    fn into_column(self) -> Self::Column;

    /// Reverse of [`into_column`](Self::into_column). Returns `None` if
    /// `col` is not part of the primary key.
    fn from_column(col: Self::Column) -> Option<Self>
    where
        Self: Sized;
}

/// Number of columns a primary key spans — 1 for a single-column key,
/// `n` for a composite key represented as an `n`-tuple.
pub trait PrimaryKeyArity {
    /// Number of columns in the primary key.
    const ARITY: usize;
}

impl<V> PrimaryKeyArity for V
where
    V: crate::TryGetable,
{
    const ARITY: usize = 1;
}

macro_rules! impl_pk_arity {
    ($len:expr, $($tuple_arg:ident),*) => {
        impl<$($tuple_arg: crate::TryGetableMany,)*> PrimaryKeyArity for ($($tuple_arg,)*) {
            const ARITY: usize = $len;
        }
    }
}

impl_pk_arity!(1, T1);
impl_pk_arity!(2, T1, T2);
impl_pk_arity!(3, T1, T2, T3);
impl_pk_arity!(4, T1, T2, T3, T4);
impl_pk_arity!(5, T1, T2, T3, T4, T5);
impl_pk_arity!(6, T1, T2, T3, T4, T5, T6);
impl_pk_arity!(7, T1, T2, T3, T4, T5, T6, T7);
impl_pk_arity!(8, T1, T2, T3, T4, T5, T6, T7, T8);
impl_pk_arity!(9, T1, T2, T3, T4, T5, T6, T7, T8, T9);
impl_pk_arity!(10, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
impl_pk_arity!(11, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
impl_pk_arity!(12, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
