use super::returning;
use crate::{
    ActiveModelTrait, ConnectionTrait, EntityTrait, IntoActiveModel, PrimaryKeyTrait, QueryTrait,
    UpdateMany, UpdateOne, ValidatedUpdateOne, error::*,
};
use sql_builder::{Expr, FromValueTuple, Query, UpdateStatement};

mod read;

/// Lower-level executor that runs a raw `sql_builder` [`UpdateStatement`].
/// Most code shouldn't need it directly — prefer
/// [`EntityTrait::update`](crate::EntityTrait::update) /
/// [`update_many`](crate::EntityTrait::update_many), which return
/// strongly-typed builders.
#[derive(Clone, Debug)]
pub struct Updater {
    query: UpdateStatement,
    check_record_exists: bool,
}

/// Result of an `UPDATE` that doesn't return rows: how many rows were
/// modified.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct UpdateResult {
    /// Number of rows touched by the statement.
    pub rows_affected: u64,
}

impl<A> ValidatedUpdateOne<A>
where
    A: ActiveModelTrait,
{
    /// Execute an UPDATE operation without a RETURNING clause, yielding an
    /// [`UpdateResult`] instead of the updated model. Returns
    /// [`DbErr::RecordNotUpdated`] if no row matches.
    /// If no fields are set, checks the complete query without issuing an
    /// update and returns zero affected rows when a record matches.
    pub async fn exec_without_returning<C>(self, db: &C) -> Result<UpdateResult, DbErr>
    where
        C: ConnectionTrait,
    {
        Updater::new(self.query)
            // If nothing is updated, return RecordNotUpdated error
            .check_record_exists()
            .exec(db)
            .await
    }

    /// Execute an UPDATE operation on an ActiveModel. Returns
    /// [`DbErr::RecordNotUpdated`] if no row matches.
    /// If no fields are set, reads the row matching the complete query without
    /// issuing an update, and returns [`DbErr::RecordNotFound`] if none does.
    /// Where the backend has no `RETURNING` (MySQL, MariaDB), the update and
    /// the read of the updated row are two statements: on a pool another
    /// writer can change the row between them, while inside a transaction the
    /// update's row lock keeps the row as written until the read.
    pub async fn exec<C>(self, db: &C) -> Result<<A::Entity as EntityTrait>::Model, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
    {
        Updater::new(self.query)
            .exec_update_and_return_updated(self.model, db)
            .await
    }
}

impl<A> UpdateOne<A>
where
    A: ActiveModelTrait,
{
    /// Execute an UPDATE operation without a RETURNING clause, yielding an
    /// [`UpdateResult`] instead of the updated model. Returns
    /// [`DbErr::RecordNotUpdated`] if no row matches.
    /// If no fields are set, checks the complete query without issuing an
    /// update and returns zero affected rows when a record matches.
    pub async fn exec_without_returning<C>(self, db: &C) -> Result<UpdateResult, DbErr>
    where
        C: ConnectionTrait,
    {
        self.0?.exec_without_returning(db).await
    }

    /// Execute an UPDATE operation on an ActiveModel. Returns
    /// [`DbErr::RecordNotUpdated`] if no row matches.
    /// If no fields are set, reads the row matching the complete query without
    /// issuing an update, and returns [`DbErr::RecordNotFound`] if none does.
    /// Where the backend has no `RETURNING` (MySQL, MariaDB), the update and
    /// the read of the updated row are two statements: on a pool another
    /// writer can change the row between them, while inside a transaction the
    /// update's row lock keeps the row as written until the read.
    pub async fn exec<C>(self, db: &C) -> Result<<A::Entity as EntityTrait>::Model, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
    {
        self.0?.exec(db).await
    }
}

impl<'a, E> UpdateMany<E>
where
    E: EntityTrait,
{
    /// Execute an update operation on multiple ActiveModels
    pub async fn exec<C>(self, db: &'a C) -> Result<UpdateResult, DbErr>
    where
        C: ConnectionTrait,
    {
        Updater::new(self.query).exec(db).await
    }
}

impl Updater {
    /// Instantiate an update using an [UpdateStatement]
    fn new(query: UpdateStatement) -> Self {
        Self {
            query,
            check_record_exists: false,
        }
    }

    fn check_record_exists(mut self) -> Self {
        self.check_record_exists = true;
        self
    }

    /// Execute an update operation
    pub async fn exec<C>(self, db: &C) -> Result<UpdateResult, DbErr>
    where
        C: ConnectionTrait,
    {
        if self.is_noop() {
            if self.check_record_exists {
                let select =
                    read::matching_rows(&self.query, Query::select().expr(Expr::val(1)).to_owned());
                if db.query_one(&select).await?.is_none() {
                    return Err(DbErr::RecordNotUpdated);
                }
            }
            return Ok(UpdateResult::default());
        }
        let result = db.execute(&self.query).await?;
        if self.check_record_exists && result.rows_affected() == 0 {
            return Err(DbErr::RecordNotUpdated);
        }
        Ok(UpdateResult {
            rows_affected: result.rows_affected(),
        })
    }

    async fn exec_update_and_return_updated<A, C>(
        mut self,
        model: A,
        db: &C,
    ) -> Result<<A::Entity as EntityTrait>::Model, DbErr>
    where
        A: ActiveModelTrait,
        C: ConnectionTrait,
    {
        if let Some(key) = model.get_primary_key_value() {
            <<A::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType::from_value_tuple(key)
                .map_err(|_| DbErr::UpdateGetPrimaryKey)?;
        }
        if self.is_noop() {
            let select = read::matching_rows(&self.query, A::Entity::find().into_query());
            return returning::one::<A::Entity, _, _>(&select, db)
                .await?
                .ok_or_else(not_found_after_update);
        }
        if db.get_database_backend().supports_returning() {
            self.query.returning(returning::all_columns::<A::Entity>(
                db.get_database_backend(),
            ));
            // No row back: the row to update does not exist.
            returning::one::<A::Entity, _, _>(&self.query, db)
                .await?
                .ok_or(DbErr::RecordNotUpdated)
        } else {
            self.check_record_exists = true;
            self.exec(db).await?;
            find_updated_model_by_id(model, db).await
        }
    }

    fn is_noop(&self) -> bool {
        self.query.get_values().is_empty()
    }
}

async fn find_updated_model_by_id<A, C>(
    model: A,
    db: &C,
) -> Result<<A::Entity as EntityTrait>::Model, DbErr>
where
    A: ActiveModelTrait,
    C: ConnectionTrait,
{
    type Entity<A> = <A as ActiveModelTrait>::Entity;
    type ValueType<A> = <<Entity<A> as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType;

    let primary_key_value = match model.get_primary_key_value() {
        Some(val) => {
            ValueType::<A>::from_value_tuple(val).map_err(|_| DbErr::UpdateGetPrimaryKey)?
        }
        None => return Err(DbErr::UpdateGetPrimaryKey),
    };
    let found = Entity::<A>::find_by_id(primary_key_value).one(db).await?;
    // If we cannot select the updated row from db by the cached primary key
    found.ok_or_else(not_found_after_update)
}

fn not_found_after_update() -> DbErr {
    DbErr::RecordNotFound("Failed to find updated item".to_owned())
}
