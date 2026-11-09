use super::{ActiveValue, ActiveValue::*};
use crate::{
    ColumnTrait, ConnectionTrait, DeleteResult, EntityTrait, Iterable, PrimaryKeyToColumn,
    UpdateResult, Value, error::*,
};
use sql_builder::ValueTuple;
use std::fmt::Debug;

mod model_trait;
pub use model_trait::ActiveModelTrait;

/// A type that can be converted into an `ActiveModel`; the `Model` derive
/// implements it for the model.
pub trait IntoActiveModel<A>
where
    A: ActiveModelTrait,
{
    /// Method to call to perform the conversion
    fn into_active_model(self) -> A;
}

impl<A> IntoActiveModel<A> for A
where
    A: ActiveModelTrait,
{
    fn into_active_model(self) -> A {
        self
    }
}
