use crate::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DbErr, EntityTrait, Iterable, PrimaryKeyToColumn,
    QueryFilter, QueryTrait,
};
use core::marker::PhantomData;
use sql_builder::{Expr, IntoIden, SimpleExpr, UpdateStatement};

/// Type-level entry point for `UPDATE` builders, e.g.
/// `Update::one(model)` and `Update::many(Entity)`. You normally call
/// [`EntityTrait::update`](crate::EntityTrait::update) /
/// [`update_many`](crate::EntityTrait::update_many) instead.
#[derive(Clone, Debug)]
pub struct Update;

/// A request to update an [`ActiveModel`](ActiveModelTrait).
///
/// The primary key must be set.
/// Otherwise, it's impossible to generate the SQL condition and find the record.
/// In that case, [`exec`][Self::exec] will return an error and not send any queries to the database.
///
/// If you want to use [`QueryTrait`] and access the generated SQL query,
/// you need to convert into [`ValidatedUpdateOne`] first.
#[derive(Clone, Debug)]
pub struct UpdateOne<A: ActiveModelTrait>(pub(crate) Result<ValidatedUpdateOne<A>, DbErr>);

/// A validated [`UpdateOne`] request, where the primary key is set
/// and it's possible to generate the right SQL condition.
#[derive(Clone, Debug)]
pub struct ValidatedUpdateOne<A: ActiveModelTrait> {
    pub(crate) query: UpdateStatement,
    pub(crate) model: A,
}

impl<A: ActiveModelTrait> TryFrom<UpdateOne<A>> for ValidatedUpdateOne<A> {
    type Error = DbErr;

    fn try_from(value: UpdateOne<A>) -> Result<Self, Self::Error> {
        value.0
    }
}

impl<A: ActiveModelTrait> UpdateOne<A> {
    /// Check whether the primary key is set and we can proceed with the operation.
    pub fn validate(self) -> Result<ValidatedUpdateOne<A>, DbErr> {
        self.try_into()
    }
}

/// Multi-row `UPDATE` builder, returned by
/// [`EntityTrait::update_many`](crate::EntityTrait::update_many). Add
/// `.col_expr(...)` to set columns and `.filter(...)` to scope which rows
/// are updated.
#[derive(Clone, Debug)]
pub struct UpdateMany<E>
where
    E: EntityTrait,
{
    pub(crate) query: UpdateStatement,
    pub(crate) entity: PhantomData<E>,
}

impl Update {
    /// Update one ActiveModel, by its primary key. One whose primary key is
    /// `NotSet` fails with [`DbErr::PrimaryKeyNotSet`] when executed.
    pub fn one<E, A>(model: A) -> UpdateOne<A>
    where
        E: EntityTrait,
        A: ActiveModelTrait<Entity = E>,
    {
        let mut myself = ValidatedUpdateOne {
            query: UpdateStatement::new()
                .table(A::Entity::default().table_ref())
                .to_owned(),
            model,
        };
        // Build the SQL condition from the primary key columns.
        for key in <A::Entity as EntityTrait>::PrimaryKey::iter() {
            let col = key.into_column();
            match myself.model.get(col) {
                ActiveValue::Set(value) | ActiveValue::Unchanged(value) => {
                    myself = myself.filter(col.eq(value));
                }
                ActiveValue::NotSet => {
                    return UpdateOne(Err(DbErr::PrimaryKeyNotSet { ctx: "UpdateOne" }));
                }
            }
        }
        // Set the values to update (from the other columns).
        for col in <A::Entity as EntityTrait>::Column::iter() {
            if <A::Entity as EntityTrait>::PrimaryKey::from_column(col).is_some() {
                continue;
            }
            match myself.model.get(col) {
                ActiveValue::Set(value) => {
                    let expr = col.save_enum_as(Expr::val(value));
                    myself.query.value(col, expr);
                }
                ActiveValue::Unchanged(_) | ActiveValue::NotSet => {}
            }
        }
        UpdateOne(Ok(myself))
    }

    /// Update many ActiveModel
    pub fn many<E>(entity: E) -> UpdateMany<E>
    where
        E: EntityTrait,
    {
        UpdateMany {
            query: UpdateStatement::new().table(entity.table_ref()).to_owned(),
            entity: PhantomData,
        }
    }
}

impl<A> QueryFilter for ValidatedUpdateOne<A>
where
    A: ActiveModelTrait,
{
    type QueryStatement = UpdateStatement;

    fn query(&mut self) -> &mut UpdateStatement {
        &mut self.query
    }
}

impl<E> QueryFilter for UpdateMany<E>
where
    E: EntityTrait,
{
    type QueryStatement = UpdateStatement;

    fn query(&mut self) -> &mut UpdateStatement {
        &mut self.query
    }
}

impl<A> QueryTrait for ValidatedUpdateOne<A>
where
    A: ActiveModelTrait,
{
    type QueryStatement = UpdateStatement;

    fn query(&mut self) -> &mut UpdateStatement {
        &mut self.query
    }

    fn as_query(&self) -> &UpdateStatement {
        &self.query
    }

    fn into_query(self) -> UpdateStatement {
        self.query
    }
}

impl<E> QueryTrait for UpdateMany<E>
where
    E: EntityTrait,
{
    type QueryStatement = UpdateStatement;

    fn query(&mut self) -> &mut UpdateStatement {
        &mut self.query
    }

    fn as_query(&self) -> &UpdateStatement {
        &self.query
    }

    fn into_query(self) -> UpdateStatement {
        self.query
    }
}

impl<E> UpdateMany<E>
where
    E: EntityTrait,
{
    /// Add the models to update to Self
    pub fn set<A>(mut self, model: A) -> Self
    where
        A: ActiveModelTrait<Entity = E>,
    {
        for col in E::Column::iter() {
            match model.get(col) {
                ActiveValue::Set(value) => {
                    let expr = col.save_enum_as(Expr::val(value));
                    self.query.value(col, expr);
                }
                ActiveValue::Unchanged(_) | ActiveValue::NotSet => {}
            }
        }
        self
    }

    /// Creates a [SimpleExpr] from a column
    pub fn col_expr<T>(mut self, col: T, expr: SimpleExpr) -> Self
    where
        T: IntoIden,
    {
        self.query.value(col, expr);
        self
    }
}
