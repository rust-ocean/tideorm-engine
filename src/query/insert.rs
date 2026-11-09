use crate::{
    ActiveModelTrait, ActiveValue, ColumnTrait, EntityName, EntityTrait, IntoActiveModel, Iterable,
    PrimaryKeyTrait, QueryTrait,
};
use core::marker::PhantomData;
use sql_builder::{Expr, InsertStatement, IntoIden, OnConflict, SimpleExpr, ValueTuple};

/// Single-row `INSERT` builder, returned by
/// [`EntityTrait::insert`](crate::EntityTrait::insert). Chain `.on_conflict(...)`
/// or `.try_insert()` then dispatch with `.exec(db)` or
/// `.exec_with_returning(db)`.
///
/// When obtained from [`TryInsert::on_conflict`], this can also represent empty
/// input. Execution then runs no SQL: row counts are zero, returned collections
/// are empty, and methods requiring one model/key return
/// [`crate::DbErr::RecordNotInserted`]. [`Self::try_insert`] preserves the empty
/// outcome as [`crate::TryInsertResult::Empty`].
#[derive(Debug)]
pub struct Insert<A>
where
    A: ActiveModelTrait,
{
    pub(crate) query: InsertStatement,
    pub(crate) primary_key: Option<ValueTuple>,
    pub(crate) empty: bool,
    pub(crate) model: PhantomData<A>,
}

/// Multi-row `INSERT` builder, returned by
/// [`EntityTrait::insert_many`](crate::EntityTrait::insert_many). Adds
/// `.exec_with_returning` and `.exec_with_returning_keys` for batch
/// inserts that need the resulting rows or primary keys.
///
/// A batch with no set fields inserts one row of database defaults per model,
/// in one statement on every backend (on SQLite it names the rowid, so a
/// `WITHOUT ROWID` table takes such rows one at a time). A model that leaves
/// unset a column another model sets gives that column `DEFAULT`, its
/// default on PostgreSQL and MySQL; SQLite has no `DEFAULT` value and
/// refuses such a batch, so insert those models separately there.
#[derive(Debug)]
pub struct InsertMany<A>
where
    A: ActiveModelTrait,
{
    pub(crate) query: InsertStatement,
    pub(crate) primary_key: Option<ValueTuple>,
    pub(crate) empty: bool,
    pub(crate) model: PhantomData<A>,
}

/// Wrapper of [`Insert`] / [`InsertMany`], treats "no row inserted/id returned" as a normal outcome.
///
/// Its `exec*` methods return [`crate::TryInsertResult`].
/// Mapping empty input to [`crate::TryInsertResult::Empty`] (no SQL executed) and
/// `DbErr::RecordNotInserted` to [`crate::TryInsertResult::Conflicted`].
///
/// Useful for idempotent inserts such as `ON CONFLICT ... DO NOTHING` (Postgres / SQLite) or the
/// MySQL polyfill (`ON DUPLICATE KEY UPDATE pk = pk`).
#[derive(Debug)]
pub struct TryInsert<A>
where
    A: ActiveModelTrait,
{
    pub(crate) insert_struct: Insert<A>,
}

impl<A> Insert<A>
where
    A: ActiveModelTrait,
{
    /// Insert one Model or ActiveModel
    pub fn one<M>(m: M) -> Self
    where
        M: IntoActiveModel<A>,
    {
        let mut query = InsertStatement::new();
        query
            .into_table(A::Entity::default().table_ref())
            .or_default_values();

        let mut am: A = m.into_active_model();
        let primary_key =
            if !<<A::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::auto_increment() {
                am.get_primary_key_value()
            } else {
                None
            };
        let mut columns = Vec::new();
        let mut values = Vec::new();

        for col in <A::Entity as EntityTrait>::Column::iter() {
            let av = am.take(col);

            match av {
                ActiveValue::Set(value) | ActiveValue::Unchanged(value) => {
                    columns.push(col);
                    values.push(col.save_enum_as(Expr::val(value)));
                }
                ActiveValue::NotSet => {}
            }
        }

        query.columns(columns);
        query.values_panic(values);

        Self {
            query,
            primary_key,
            empty: false,
            model: PhantomData,
        }
    }

    /// Insert many Model or ActiveModel.
    /// Alias to [`InsertMany::many`].
    pub fn many<M, I>(models: I) -> InsertMany<A>
    where
        M: IntoActiveModel<A>,
        I: IntoIterator<Item = M>,
    {
        InsertMany::many(models)
    }

    /// Set the ON CONFLICT clause: do nothing, or update (an upsert).
    pub fn on_conflict(mut self, on_conflict: OnConflict) -> Self {
        self.query.on_conflict(on_conflict);
        self
    }

    /// Set ON CONFLICT do nothing, but with MySQL specific polyfill.
    pub fn on_conflict_do_nothing_on<I>(mut self, columns: I) -> TryInsert<A>
    where
        I: IntoIterator<Item = <A::Entity as EntityTrait>::Column>,
    {
        self.query
            .on_conflict(do_nothing_on_conflict::<A, _>(columns));
        TryInsert::from_one(self)
    }

    /// Convert self into a `TryInsert`. It is just a wrapper for converting `DbErr::RecordNotInserted` -> `TryInsertResult::Conflicted`.
    pub fn try_insert(self) -> TryInsert<A>
    where
        A: ActiveModelTrait,
    {
        TryInsert::from_one(self)
    }

    /// Set ON CONFLICT on primary key do nothing, but with MySQL specific polyfill.
    pub fn on_conflict_do_nothing(mut self) -> TryInsert<A>
    where
        A: ActiveModelTrait,
    {
        self.query.on_conflict(on_conflict_primary_key::<A>());

        TryInsert::from_one(self)
    }
}

impl<A> InsertMany<A>
where
    A: ActiveModelTrait,
{
    /// Insert many Model or ActiveModel.
    pub fn many<M, I>(models: I) -> Self
    where
        M: IntoActiveModel<A>,
        I: IntoIterator<Item = M>,
    {
        let mut query = InsertStatement::new();
        query.into_table(A::Entity::default().table_ref());

        let mut columns: Vec<_> = <A::Entity as EntityTrait>::Column::iter()
            .map(|_| None)
            .collect();
        let mut all_values: Vec<Vec<SimpleExpr>> = Vec::new();
        let mut primary_key = None;

        for model in models.into_iter() {
            let mut am: A = model.into_active_model();
            primary_key =
                if !<<A::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::auto_increment() {
                    am.get_primary_key_value()
                } else {
                    None
                };
            let mut values = Vec::with_capacity(columns.len());
            for (idx, col) in <A::Entity as EntityTrait>::Column::iter().enumerate() {
                let av = am.take(col);
                match av {
                    ActiveValue::Set(value) | ActiveValue::Unchanged(value) => {
                        columns[idx] = Some(col); // mark the column as used
                        values.push(col.save_enum_as(Expr::val(value))); // same as add() above
                    }
                    ActiveValue::NotSet => values.push(Expr::keyword_default()),
                }
            }
            all_values.push(values);
        }

        let empty = all_values.is_empty();

        if !empty && columns.iter().all(Option::is_none) {
            query.or_default_values_many(all_values.len());
        }

        if !all_values.is_empty() {
            // filter only used column
            query.columns(columns.iter().copied().flatten());
        }

        for values in all_values {
            // since we've aligned the column set, this never panics
            query.values_panic(
                values
                    .into_iter()
                    .zip(&columns)
                    .filter_map(|(value, column)| column.is_some().then_some(value)),
            );
        }

        Self {
            query,
            primary_key,
            empty,
            model: PhantomData,
        }
    }

    /// Set ON CONFLICT logic
    pub fn on_conflict(mut self, on_conflict: OnConflict) -> Self {
        self.query.on_conflict(on_conflict);
        self
    }

    /// Set ON CONFLICT do nothing, but with MySQL specific polyfill.
    pub fn on_conflict_do_nothing_on<I>(mut self, columns: I) -> TryInsert<A>
    where
        I: IntoIterator<Item = <A::Entity as EntityTrait>::Column>,
    {
        self.query
            .on_conflict(do_nothing_on_conflict::<A, _>(columns));
        TryInsert::from_many(self)
    }

    /// Convert self into a `TryInsert`. It is just a wrapper for converting `DbErr::RecordNotInserted` -> `TryInsertResult::Conflicted`.
    pub fn try_insert(self) -> TryInsert<A>
    where
        A: ActiveModelTrait,
    {
        TryInsert::from_many(self)
    }

    /// Set ON CONFLICT on primary key do nothing, but with MySQL specific polyfill.
    /// See also [`Insert::on_conflict_do_nothing`].
    pub fn on_conflict_do_nothing(mut self) -> TryInsert<A>
    where
        A: ActiveModelTrait,
    {
        self.query.on_conflict(on_conflict_primary_key::<A>());

        TryInsert::from_many(self)
    }

    /// Keep the input's empty state when changing the execution wrapper.
    pub(crate) fn into_one(self) -> Insert<A> {
        let Self {
            query,
            primary_key,
            empty,
            model,
        } = self;

        Insert {
            query,
            primary_key,
            empty,
            model,
        }
    }
}

impl<A> QueryTrait for Insert<A>
where
    A: ActiveModelTrait,
{
    type QueryStatement = InsertStatement;

    fn query(&mut self) -> &mut InsertStatement {
        &mut self.query
    }

    fn as_query(&self) -> &InsertStatement {
        &self.query
    }

    fn into_query(self) -> InsertStatement {
        self.query
    }
}

impl<A> QueryTrait for InsertMany<A>
where
    A: ActiveModelTrait,
{
    type QueryStatement = InsertStatement;

    fn query(&mut self) -> &mut InsertStatement {
        &mut self.query
    }

    fn as_query(&self) -> &InsertStatement {
        &self.query
    }

    fn into_query(self) -> InsertStatement {
        self.query
    }
}

impl<A> TryInsert<A>
where
    A: ActiveModelTrait,
{
    fn from_one(insert: Insert<A>) -> Self {
        Self {
            insert_struct: insert,
        }
    }

    fn from_many(insert: InsertMany<A>) -> Self {
        Self::from_one(insert.into_one())
    }

    /// Try insert one item
    pub fn one<M>(m: M) -> Self
    where
        M: IntoActiveModel<A>,
    {
        Self::from_one(Insert::one(m))
    }

    /// Try insert many items
    pub fn many<M, I>(models: I) -> Self
    where
        M: IntoActiveModel<A>,
        I: IntoIterator<Item = M>,
    {
        Self::from_many(Insert::many(models))
    }

    /// Set ON CONFLICT logic and return the underlying [`Insert`].
    /// Empty input remains empty. Call [`Insert::try_insert`] on the result to
    /// retain [`crate::TryInsertResult::Empty`] / [`crate::TryInsertResult::Conflicted`]
    /// outcomes.
    pub fn on_conflict(mut self, on_conflict: OnConflict) -> Insert<A> {
        self.insert_struct.query.on_conflict(on_conflict);
        self.insert_struct
    }

    /// Set ON CONFLICT on primary key do nothing, but with MySQL specific polyfill.
    pub fn on_conflict_do_nothing(mut self) -> Self {
        self.insert_struct
            .query
            .on_conflict(on_conflict_primary_key::<A>());

        self
    }

    /// Set ON CONFLICT do nothing, but with MySQL specific polyfill.
    pub fn on_conflict_do_nothing_on<I>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = <A::Entity as EntityTrait>::Column>,
    {
        self.insert_struct
            .query
            .on_conflict(do_nothing_on_conflict::<A, _>(columns));
        self
    }
}

impl<A> QueryTrait for TryInsert<A>
where
    A: ActiveModelTrait,
{
    type QueryStatement = InsertStatement;

    fn query(&mut self) -> &mut InsertStatement {
        &mut self.insert_struct.query
    }

    fn as_query(&self) -> &InsertStatement {
        &self.insert_struct.query
    }

    fn into_query(self) -> InsertStatement {
        self.insert_struct.query
    }
}

fn on_conflict_primary_key<A: ActiveModelTrait>() -> OnConflict {
    do_nothing_on_conflict::<A, _>(<A::Entity as EntityTrait>::PrimaryKey::iter())
}

/// `ON CONFLICT (columns) DO NOTHING`, with the MySQL polyfill that updates
/// the primary key to itself.
fn do_nothing_on_conflict<A, C>(columns: impl IntoIterator<Item = C>) -> OnConflict
where
    A: ActiveModelTrait,
    C: IntoIden,
{
    let mut on_conflict = OnConflict::columns(columns);
    on_conflict.do_nothing_on(<A::Entity as EntityTrait>::PrimaryKey::iter());
    on_conflict
}
