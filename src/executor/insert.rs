use super::returning;
use crate::{
    ActiveModelTrait, ConnectionTrait, DbBackend, EntityTrait, Insert, InsertMany, IntoActiveModel,
    Iterable, PrimaryKeyTrait, QueryResult, TryFromU64, TryInsert, error::*,
};
use sql_builder::{FromValueTuple, Iden, InsertStatement, ValueTuple};
use std::marker::PhantomData;

type PrimaryKey<A> = <<A as ActiveModelTrait>::Entity as EntityTrait>::PrimaryKey;
type KeyOf<A> = <PrimaryKey<A> as PrimaryKeyTrait>::ValueType;

/// Lower-level executor that runs a raw `sql_builder` [`InsertStatement`].
/// Most code shouldn't need it directly — prefer
/// [`EntityTrait::insert`](crate::EntityTrait::insert) /
/// [`insert_many`](crate::EntityTrait::insert_many), which return
/// strongly-typed builders.
#[derive(Debug)]
pub struct Inserter<A>
where
    A: ActiveModelTrait,
{
    primary_key: Option<ValueTuple>,
    query: InsertStatement,
    model: PhantomData<A>,
}

/// Result of inserting a single ActiveModel: the primary key the database
/// assigned (or the one already on the ActiveModel if it had been `Set`).
#[derive(Debug)]
pub struct InsertResult<A>
where
    A: ActiveModelTrait,
{
    /// Primary key of the inserted row.
    pub last_insert_id: KeyOf<A>,
}

/// Result of inserting many ActiveModels: the primary key of the last row
/// inserted, or `None` if the iterator was empty.
#[derive(Debug)]
pub struct InsertManyResult<A>
where
    A: ActiveModelTrait,
{
    /// Primary key of the last inserted row, if any.
    pub last_insert_id: Option<KeyOf<A>>,
}

/// The result of executing a [`crate::TryInsert`].
///
/// This enum represents no‑op inserts (e.g. conflict `DO NOTHING`) without treating
/// them as errors.
#[derive(Debug)]
pub enum TryInsertResult<T> {
    /// There was nothing to insert, so no SQL was executed.
    ///
    /// This typically happens when creating a [`crate::TryInsert`] from an empty iterator or None.
    Empty,
    /// The statement was executed, but the engine could not get the inserted row / insert id.
    ///
    /// This is commonly caused by `ON CONFLICT ... DO NOTHING` (Postgres / SQLite) or the MySQL
    /// polyfill (`ON DUPLICATE KEY UPDATE pk = pk`).
    ///
    /// Note that this variant maps from `DbErr::RecordNotInserted`, so it can also represent other
    /// situations where the backend/driver reports no inserted row (e.g. an empty `RETURNING`
    /// result set or a "no-op" update in MySQL where `last_insert_id` is reported as `0`). In rare
    /// cases, this can be a false negative where a row was inserted but the backend did not report
    /// it.
    Conflicted,
    /// Successfully inserted
    Inserted(T),
}

impl<A> TryInsertResult<InsertResult<A>>
where
    A: ActiveModelTrait,
{
    /// Extract the last inserted id.
    ///
    /// - [`TryInsertResult::Empty`] => `Ok(None)`
    /// - [`TryInsertResult::Inserted`] => `Ok(Some(last_insert_id))`
    /// - [`TryInsertResult::Conflicted`] => `Err(DbErr::RecordNotInserted)`
    pub fn last_insert_id(self) -> Result<Option<KeyOf<A>>, DbErr> {
        match self {
            Self::Empty => Ok(None),
            Self::Inserted(v) => Ok(Some(v.last_insert_id)),
            Self::Conflicted => Err(DbErr::RecordNotInserted),
        }
    }
}

impl<T> TryInsertResult<T> {
    /// The result of an insert that ran, with one that wrote nothing as
    /// [`Conflicted`](Self::Conflicted).
    fn ran(result: Result<T, DbErr>) -> Result<Self, DbErr> {
        match result {
            Ok(value) => Ok(Self::Inserted(value)),
            Err(DbErr::RecordNotInserted) => Ok(Self::Conflicted),
            Err(error) => Err(error),
        }
    }
}

impl<A> TryInsert<A>
where
    A: ActiveModelTrait,
{
    async fn run<T>(
        self,
        operation: impl AsyncFnOnce(Insert<A>) -> Result<T, DbErr>,
    ) -> Result<TryInsertResult<T>, DbErr> {
        if self.insert_struct.empty {
            return Ok(TryInsertResult::Empty);
        }
        TryInsertResult::ran(operation(self.insert_struct).await)
    }

    /// Execute an insert operation
    pub async fn exec<C>(self, db: &C) -> Result<TryInsertResult<InsertResult<A>>, DbErr>
    where
        C: ConnectionTrait,
    {
        self.run(|insert: Insert<A>| insert.exec(db)).await
    }

    /// Execute an insert operation without returning (don't use `RETURNING` syntax)
    /// Number of rows affected is returned
    pub async fn exec_without_returning<C>(self, db: &C) -> Result<TryInsertResult<u64>, DbErr>
    where
        C: ConnectionTrait,
    {
        self.run(|insert: Insert<A>| insert.exec_without_returning(db))
            .await
    }

    /// Execute an insert operation and return the inserted model (use `RETURNING` syntax if supported)
    pub async fn exec_with_returning<C>(
        self,
        db: &C,
    ) -> Result<TryInsertResult<<A::Entity as EntityTrait>::Model>, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
    {
        self.run(|insert: Insert<A>| insert.exec_with_returning(db))
            .await
    }

    /// Execute an insert operation and return primary keys of inserted models
    pub async fn exec_with_returning_keys<C>(
        self,
        db: &C,
    ) -> Result<TryInsertResult<Vec<KeyOf<A>>>, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
    {
        self.run(|insert: Insert<A>| insert.exec_with_returning_keys(db))
            .await
    }

    /// Execute an insert operation and return all inserted models
    pub async fn exec_with_returning_many<C>(
        self,
        db: &C,
    ) -> Result<TryInsertResult<Vec<<A::Entity as EntityTrait>::Model>>, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
    {
        self.run(|insert: Insert<A>| insert.exec_with_returning_many(db))
            .await
    }
}

impl<A> Insert<A>
where
    A: ActiveModelTrait,
{
    /// Execute an insert operation
    pub async fn exec<'a, C>(self, db: &'a C) -> Result<InsertResult<A>, DbErr>
    where
        C: ConnectionTrait,
        A: 'a,
    {
        if self.empty {
            return Err(DbErr::RecordNotInserted);
        }
        let mut query = self.query;
        if returning::insert_returns(db, &query) {
            query.returning(returning::primary_key::<A::Entity>(
                db.get_database_backend(),
            ));
        }
        Inserter::<A>::new(self.primary_key, query).exec(db).await
    }

    /// Execute an insert operation without returning (don't use `RETURNING` syntax)
    /// Number of rows affected is returned
    pub async fn exec_without_returning<'a, C>(self, db: &'a C) -> Result<u64, DbErr>
    where
        C: ConnectionTrait,
        A: 'a,
    {
        if self.empty {
            return Ok(0);
        }
        Inserter::<A>::new(self.primary_key, self.query)
            .exec_without_returning(db)
            .await
    }

    /// Execute an insert operation and return the inserted model (uses
    /// `RETURNING` if the backend supports it).
    ///
    /// + To get back all inserted models, use [`InsertMany::exec_with_returning`].
    /// + To get back all inserted primary keys, use [`InsertMany::exec_with_returning_keys`].
    pub async fn exec_with_returning<'a, C>(
        self,
        db: &'a C,
    ) -> Result<<A::Entity as EntityTrait>::Model, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
        A: 'a,
    {
        if self.empty {
            return Err(DbErr::RecordNotInserted);
        }
        Inserter::<A>::new(self.primary_key, self.query)
            .exec_with_returning(db)
            .await
    }

    /// Execute an insert operation and return primary keys of inserted models
    pub async fn exec_with_returning_keys<'a, C>(self, db: &'a C) -> Result<Vec<KeyOf<A>>, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
        A: 'a,
    {
        if self.empty {
            return Ok(Vec::new());
        }
        Inserter::<A>::new(self.primary_key, self.query)
            .exec_with_returning_keys(db)
            .await
    }

    /// Execute an insert operation and return all inserted models
    pub async fn exec_with_returning_many<'a, C>(
        self,
        db: &'a C,
    ) -> Result<Vec<<A::Entity as EntityTrait>::Model>, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
        A: 'a,
    {
        if self.empty {
            return Ok(Vec::new());
        }
        Inserter::<A>::new(self.primary_key, self.query)
            .exec_with_returning_many(db)
            .await
    }
}

impl<A> InsertMany<A>
where
    A: ActiveModelTrait,
{
    /// Execute an insert operation
    pub async fn exec<C>(self, db: &C) -> Result<InsertManyResult<A>, DbErr>
    where
        C: ConnectionTrait,
    {
        let last_insert_id = if self.empty {
            None
        } else {
            Some(self.into_one().exec(db).await?.last_insert_id)
        };
        Ok(InsertManyResult { last_insert_id })
    }

    /// Execute an insert operation without returning (don't use `RETURNING` syntax)
    /// Number of rows affected is returned
    pub async fn exec_without_returning<C>(self, db: &C) -> Result<u64, DbErr>
    where
        C: ConnectionTrait,
    {
        if self.empty {
            return Ok(0);
        }
        self.into_one().exec_without_returning(db).await
    }

    /// Execute an insert operation and return all inserted models
    pub async fn exec_with_returning<C>(
        self,
        db: &C,
    ) -> Result<Vec<<A::Entity as EntityTrait>::Model>, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
    {
        if self.empty {
            return Ok(Vec::new());
        }
        self.into_one().exec_with_returning_many(db).await
    }

    /// Execute an insert operation and return primary keys of inserted models
    pub async fn exec_with_returning_keys<C>(self, db: &C) -> Result<Vec<KeyOf<A>>, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
    {
        if self.empty {
            return Ok(Vec::new());
        }
        self.into_one().exec_with_returning_keys(db).await
    }
}

impl<A> Inserter<A>
where
    A: ActiveModelTrait,
{
    /// Instantiate a new insert operation
    pub fn new(primary_key: Option<ValueTuple>, query: InsertStatement) -> Self {
        Self {
            primary_key,
            query,
            model: PhantomData,
        }
    }

    /// Execute an insert operation, returning the last inserted id
    pub async fn exec<'a, C>(self, db: &'a C) -> Result<InsertResult<A>, DbErr>
    where
        C: ConnectionTrait,
        A: 'a,
    {
        let primary_key = self
            .primary_key
            .map(KeyOf::<A>::from_value_tuple)
            .transpose()
            .map_err(|_| DbErr::UnpackInsertId)?;
        let last_insert_id = if returning::insert_returns(db, &self.query) {
            let row = db
                .query_all(&self.query)
                .await?
                .pop()
                .ok_or(DbErr::RecordNotInserted)?;
            read_key::<A>(&row, &key_columns::<A>())?
        } else {
            let result = db.execute(&self.query).await?;
            if result.rows_affected() == 0 {
                return Err(DbErr::RecordNotInserted);
            }
            match primary_key {
                Some(key) => key,
                None => {
                    let id = result.try_last_insert_id()?;
                    // MySQL counts 1 per row inserted, 2 per existing row
                    // updated and 0 per row set to its current values, so an
                    // upsert that changed nothing reports no id:
                    // https://dev.mysql.com/doc/refman/8.4/en/insert-on-duplicate.html
                    if matches!(
                        db.get_database_backend(),
                        DbBackend::MySQL | DbBackend::MariaDB
                    ) && id == 0
                    {
                        return Err(DbErr::RecordNotInserted);
                    }
                    KeyOf::<A>::try_from_u64(id)?
                }
            }
        };
        Ok(InsertResult { last_insert_id })
    }

    /// Execute an insert operation
    pub async fn exec_without_returning<'a, C>(self, db: &'a C) -> Result<u64, DbErr>
    where
        C: ConnectionTrait,
        A: 'a,
    {
        Ok(db.execute(&self.query).await?.rows_affected())
    }

    /// Execute an insert operation and return the inserted model (use `RETURNING` syntax if supported)
    pub async fn exec_with_returning<'a, C>(
        mut self,
        db: &'a C,
    ) -> Result<<A::Entity as EntityTrait>::Model, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
        A: 'a,
    {
        let found = if returning::insert_returns(db, &self.query) {
            self.query.returning(returning::all_columns::<A::Entity>(
                db.get_database_backend(),
            ));
            returning::one::<A::Entity, _, _>(&self.query, db).await?
        } else {
            let key = self.exec(db).await?.last_insert_id;
            <A::Entity as EntityTrait>::find_by_id(key).one(db).await?
        };
        found.ok_or_else(|| DbErr::RecordNotFound("Failed to find inserted item".to_owned()))
    }

    /// Execute an insert operation and return primary keys of inserted models
    pub async fn exec_with_returning_keys<'a, C>(
        mut self,
        db: &'a C,
    ) -> Result<Vec<KeyOf<A>>, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
        A: 'a,
    {
        let supported = returning::insert_returns(db, &self.query);
        let backend = returning::returning_backend(db, supported, "INSERT RETURNING")?;
        self.query
            .returning(returning::primary_key::<A::Entity>(backend));
        let columns = key_columns::<A>();
        let rows = db.query_all(&self.query).await?;
        super::decode_all(rows, |row| read_key::<A>(&row, &columns))
    }

    /// Execute an insert operation and return all inserted models
    pub async fn exec_with_returning_many<'a, C>(
        mut self,
        db: &'a C,
    ) -> Result<Vec<<A::Entity as EntityTrait>::Model>, DbErr>
    where
        <A::Entity as EntityTrait>::Model: IntoActiveModel<A>,
        C: ConnectionTrait,
        A: 'a,
    {
        let supported = returning::insert_returns(db, &self.query);
        let backend = returning::returning_backend(db, supported, "INSERT RETURNING")?;
        self.query
            .returning(returning::all_columns::<A::Entity>(backend));
        returning::all::<A::Entity, _, _>(&self.query, db).await
    }
}

fn key_columns<A: ActiveModelTrait>() -> Vec<String> {
    PrimaryKey::<A>::iter().map(|key| key.to_string()).collect()
}

/// The primary key a `RETURNING` row holds in `columns`, or the error decoding
/// it gave.
fn read_key<A: ActiveModelTrait>(row: &QueryResult, columns: &[String]) -> Result<KeyOf<A>, DbErr> {
    row.try_get_many(columns)
}
