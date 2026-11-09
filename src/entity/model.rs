use crate::{
    ColumnTrait, DbErr, EntityTrait, Iterable, PrimaryKeyToColumn, QueryResult, SelectModel,
    SelectorRaw, Statement,
};
pub use sql_builder::Value;
use sql_builder::{ArrayType, ValueTuple};

/// The interface implemented by every Model — an instance of an
/// [`EntityTrait`], roughly an OOP "object" whose fields are the table's
/// columns.
///
/// Implemented by TideORM's model derive. Pairs with an
/// [`ActiveModelTrait`](crate::ActiveModelTrait) type for mutations.
pub trait ModelTrait: Sized + Send {
    /// The [`EntityTrait`] this model belongs to.
    type Entity: EntityTrait;

    /// Read the value of one column.
    fn get(&self, c: <Self::Entity as EntityTrait>::Column) -> Value;

    /// Type of the value stored by a column, used by reflection helpers
    /// such as Arrow conversion.
    fn get_value_type(c: <Self::Entity as EntityTrait>::Column) -> ArrayType;

    /// Write a value to one column. Panics if the value's type doesn't match
    /// the column; prefer [`try_set`](Self::try_set) when the value comes
    /// from untrusted input.
    fn set(&mut self, c: <Self::Entity as EntityTrait>::Column, v: Value) {
        self.try_set(c, v)
            .unwrap_or_else(|e| panic!("Failed to set value for {:?}: {e:?}", c.as_column_ref()))
    }

    /// Write a value to one column, returning an error if the value's type
    /// does not match the column.
    fn try_set(&mut self, c: <Self::Entity as EntityTrait>::Column, v: Value) -> Result<(), DbErr>;

    /// Get the primary key value of the Model
    fn get_primary_key_value(&self) -> ValueTuple {
        let values = <Self::Entity as EntityTrait>::PrimaryKey::iter()
            .map(|key| self.get(key.into_column()));
        crate::query::value_tuple(values).expect("an entity has a primary key")
    }
}

/// Construct a value from a [`QueryResult`] row.
///
/// Implemented for every Model by TideORM's model derive, and by hand
/// for any other shape read out of a `SELECT`. See
/// [`find_by_statement`](Self::find_by_statement) for executing a raw SQL
/// query that materialises into the type.
pub trait FromQueryResult: Sized {
    /// Instantiate a Model from a [QueryResult]
    fn from_query_result(res: &QueryResult) -> Result<Self, DbErr>;

    /// Execute a raw SQL [`Statement`] and read each row as `Self`.
    fn find_by_statement(stmt: Statement) -> SelectorRaw<SelectModel<Self>> {
        SelectorRaw::<SelectModel<Self>>::from_statement(stmt)
    }
}

/// Fallible conversion into a [`ModelTrait`] value.
///
/// Implemented for [`ActiveModelTrait`](crate::ActiveModelTrait) so a
/// partially-filled `ActiveModel` can be turned into a full `Model` once every
/// column is `Set` or `Unchanged`; returns
/// [`DbErr::AttrNotSet`](crate::DbErr::AttrNotSet) otherwise.
pub trait TryIntoModel<M>
where
    M: ModelTrait,
{
    /// Attempt the conversion, returning an error if a required column is
    /// not set.
    fn try_into_model(self) -> Result<M, DbErr>;
}

impl<M> TryIntoModel<M> for M
where
    M: ModelTrait,
{
    fn try_into_model(self) -> Result<M, DbErr> {
        Ok(self)
    }
}
