use crate::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DbErr, EntityTrait, IntoActiveModel, Iterable,
    PrimaryKeyToColumn, QueryFilter, QueryTrait,
};
use core::marker::PhantomData;
use sql_builder::DeleteStatement;

/// Type-level entry point for `DELETE` builders, e.g.
/// `Delete::one(model)` and `Delete::many(Entity)`. You normally call
/// [`EntityTrait::delete`](crate::EntityTrait::delete) /
/// [`delete_many`](crate::EntityTrait::delete_many) instead.
#[derive(Clone, Debug)]
pub struct Delete;

/// A request to delete an [`ActiveModel`](ActiveModelTrait).
///
/// The primary key must be set.
/// Otherwise, it's impossible to generate the SQL condition and find the record.
/// In that case, [`exec`][Self::exec] will return an error and not send any queries to the database.
///
/// If you want to use [`QueryTrait`] and access the generated SQL query,
/// you need to convert into [`ValidatedDeleteOne`] first.
#[derive(Clone, Debug)]
pub struct DeleteOne<E: EntityTrait>(pub(crate) Result<ValidatedDeleteOne<E>, DbErr>);

/// A validated [`DeleteOne`] request, where the primary key is set
/// and it's possible to generate the right SQL condition.
#[derive(Clone, Debug)]
pub struct ValidatedDeleteOne<E: EntityTrait> {
    pub(crate) query: DeleteStatement,
    pub(crate) entity: PhantomData<E>,
}

impl<E: EntityTrait> TryFrom<DeleteOne<E>> for ValidatedDeleteOne<E> {
    type Error = DbErr;

    fn try_from(value: DeleteOne<E>) -> Result<Self, Self::Error> {
        value.0
    }
}

impl<E: EntityTrait> DeleteOne<E> {
    /// Check whether the primary key is set and we can proceed with the operation.
    pub fn validate(self) -> Result<ValidatedDeleteOne<E>, DbErr> {
        self.try_into()
    }
}

/// Multi-row `DELETE` builder, returned by
/// [`EntityTrait::delete_many`](crate::EntityTrait::delete_many). Add
/// `.filter(...)` to scope which rows are deleted.
#[derive(Clone, Debug)]
pub struct DeleteMany<E>
where
    E: EntityTrait,
{
    pub(crate) query: DeleteStatement,
    pub(crate) entity: PhantomData<E>,
}

impl Delete {
    /// Delete one Model or ActiveModel, by its primary key. One whose primary
    /// key is `NotSet` fails with [`DbErr::PrimaryKeyNotSet`] when executed.
    pub fn one<E, A, M>(model: M) -> DeleteOne<E>
    where
        E: EntityTrait,
        A: ActiveModelTrait<Entity = E>,
        M: IntoActiveModel<A>,
    {
        let model = model.into_active_model();
        let mut myself = ValidatedDeleteOne {
            query: DeleteStatement::new()
                .from_table(A::Entity::default().table_ref())
                .to_owned(),
            entity: PhantomData,
        };
        // Build the SQL condition from the primary key columns.
        for key in <A::Entity as EntityTrait>::PrimaryKey::iter() {
            let col = key.into_column();
            let av = model.get(col);
            match av {
                ActiveValue::Set(value) | ActiveValue::Unchanged(value) => {
                    myself = myself.filter(col.eq(value));
                }
                ActiveValue::NotSet => {
                    return DeleteOne(Err(DbErr::PrimaryKeyNotSet { ctx: "DeleteOne" }));
                }
            }
        }
        DeleteOne(Ok(myself))
    }

    /// Delete many ActiveModel
    pub fn many<E>(entity: E) -> DeleteMany<E>
    where
        E: EntityTrait,
    {
        DeleteMany {
            query: DeleteStatement::new()
                .from_table(entity.table_ref())
                .to_owned(),
            entity: PhantomData,
        }
    }
}

builds_statement!(DeleteStatement, ValidatedDeleteOne<E: EntityTrait>, filtered);
builds_statement!(DeleteStatement, DeleteMany<E: EntityTrait>, filtered);
