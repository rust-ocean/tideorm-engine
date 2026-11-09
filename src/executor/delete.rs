use crate::{ConnectionTrait, DeleteMany, DeleteOne, EntityTrait, ValidatedDeleteOne, error::*};
use sql_builder::DeleteStatement;

/// Lower-level executor that runs a raw `sql_builder` [`DeleteStatement`] and
/// collects the result. Most code shouldn't need it directly — prefer
/// [`EntityTrait::delete`](crate::EntityTrait::delete) /
/// [`delete_many`](crate::EntityTrait::delete_many) /
/// [`delete_by_id`](crate::EntityTrait::delete_by_id), which return
/// strongly-typed builders.
#[derive(Clone, Debug)]
pub struct Deleter {
    query: DeleteStatement,
}

/// Result of a `DELETE`: how many rows were removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteResult {
    /// Number of rows removed by the statement.
    pub rows_affected: u64,
}

impl<E> ValidatedDeleteOne<E>
where
    E: EntityTrait,
{
    /// Execute a DELETE operation on one ActiveModel
    pub async fn exec<C>(self, db: &C) -> Result<DeleteResult, DbErr>
    where
        C: ConnectionTrait,
    {
        Deleter::new(self.query).exec(db).await
    }
}

impl<E> DeleteOne<E>
where
    E: EntityTrait,
{
    /// Execute a DELETE operation on one ActiveModel
    pub async fn exec<C>(self, db: &C) -> Result<DeleteResult, DbErr>
    where
        C: ConnectionTrait,
    {
        self.0?.exec(db).await
    }
}

impl<'a, E> DeleteMany<E>
where
    E: EntityTrait,
{
    /// Execute a DELETE operation on many ActiveModels
    pub async fn exec<C>(self, db: &'a C) -> Result<DeleteResult, DbErr>
    where
        C: ConnectionTrait,
    {
        Deleter::new(self.query).exec(db).await
    }
}

impl Deleter {
    /// Instantiate a new [Deleter] by passing it a [DeleteStatement]
    pub fn new(query: DeleteStatement) -> Self {
        Self { query }
    }

    /// Execute a DELETE operation
    pub async fn exec<C>(self, db: &C) -> Result<DeleteResult, DbErr>
    where
        C: ConnectionTrait,
    {
        let result = db.execute(&self.query).await?;
        Ok(DeleteResult {
            rows_affected: result.rows_affected(),
        })
    }
}

impl DeleteResult {
    #[doc(hidden)]
    pub fn empty() -> Self {
        Self { rows_affected: 0 }
    }

    #[doc(hidden)]
    pub fn merge(&mut self, other: DeleteResult) {
        self.rows_affected += other.rows_affected;
    }
}
