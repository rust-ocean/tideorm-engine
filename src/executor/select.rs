use crate::{
    ConnectionTrait, DbBackend, EntityTrait, FromQueryResult, QueryResult, Select, Statement,
    TryGetableMany, error::*,
};

use sql_builder::SelectStatement;
use std::marker::PhantomData;

mod entity;

fn record_not_found() -> DbErr {
    DbErr::RecordNotFound("None of the models match the query".to_owned())
}

fn require<T>(item: Option<T>) -> Result<T, DbErr> {
    item.ok_or_else(record_not_found)
}

pub(crate) fn decode_one<T>(
    row: Option<QueryResult>,
    decode: impl FnOnce(QueryResult) -> Result<T, DbErr>,
) -> Result<Option<T>, DbErr> {
    row.map(decode).transpose()
}

pub(crate) fn decode_all<T>(
    rows: impl IntoIterator<Item = QueryResult>,
    decode: impl FnMut(QueryResult) -> Result<T, DbErr>,
) -> Result<Vec<T>, DbErr> {
    rows.into_iter().map(decode).collect()
}

/// Runs `query` limited to one row and decodes that row, if any.
pub(crate) async fn select_one<C, T>(
    db: &C,
    mut query: SelectStatement,
    decode: impl FnOnce(QueryResult) -> Result<T, DbErr>,
) -> Result<Option<T>, DbErr>
where
    C: ConnectionTrait,
{
    query.limit(1);
    decode_one(db.query_one(&query).await?, decode)
}

/// Runs `query` and decodes every row.
pub(crate) async fn select_all<C, T>(
    db: &C,
    query: &SelectStatement,
    decode: impl FnMut(QueryResult) -> Result<T, DbErr>,
) -> Result<Vec<T>, DbErr>
where
    C: ConnectionTrait,
{
    decode_all(db.query_all(query).await?, decode)
}

/// A ready-to-execute `SELECT` query backed by a [`SelectStatement`]. The
/// type parameter `S` (a [`SelectorTrait`]) determines what each row is
/// decoded into. Build one via
/// [`Select::into_model`](crate::Select::into_model), then
/// call `.one(db)` / `.all(db)` / `.paginate(db, n)`.
#[derive(Clone, Debug)]
pub struct Selector<S>
where
    S: SelectorTrait,
{
    pub(crate) query: SelectStatement,
    selector: PhantomData<S>,
}

/// Like [`Selector`] but executes a raw [`Statement`] instead of a
/// `sql_builder` query.
#[derive(Clone, Debug)]
pub struct SelectorRaw<S>
where
    S: SelectorTrait,
{
    pub(crate) stmt: Statement,
    pub(super) selector: PhantomData<S>,
}

/// Decodes one row of a [`Selector`] / [`SelectorRaw`] result into a value of
/// type [`Item`](Self::Item). Implemented by the `SelectModel*` types below;
/// you usually never name this trait directly.
pub trait SelectorTrait {
    /// Type produced for each row.
    type Item: Sized;

    /// Decode one row.
    fn from_raw_query_result(res: QueryResult) -> Result<Self::Item, DbErr>;
}

/// [`SelectorTrait`] adapter that decodes each row positionally into the
/// tuple type `T`.
#[derive(Debug)]
pub struct SelectGetableTuple<T>
where
    T: TryGetableMany,
{
    model: PhantomData<T>,
}

/// [`SelectorTrait`] for a query that yields a single model per row.
#[derive(Debug)]
pub struct SelectModel<M>
where
    M: FromQueryResult,
{
    model: PhantomData<M>,
}

impl<T> SelectorTrait for SelectGetableTuple<T>
where
    T: TryGetableMany,
{
    type Item = T;

    fn from_raw_query_result(res: QueryResult) -> Result<Self::Item, DbErr> {
        T::try_get_many_by_index(&res).map_err(Into::into)
    }
}

impl<M> SelectorTrait for SelectModel<M>
where
    M: FromQueryResult + Sized,
{
    type Item = M;

    fn from_raw_query_result(res: QueryResult) -> Result<Self::Item, DbErr> {
        M::from_query_result(&res)
    }
}

impl<S> Selector<S>
where
    S: SelectorTrait,
{
    pub(crate) fn new(query: SelectStatement) -> Self {
        Self {
            query,
            selector: PhantomData,
        }
    }

    /// Get the SQL statement
    pub fn into_statement(self, builder: DbBackend) -> Result<Statement, DbErr> {
        builder.build(&self.query)
    }

    /// Get an item from the Select query
    pub async fn one<C>(self, db: &C) -> Result<Option<S::Item>, DbErr>
    where
        C: ConnectionTrait,
    {
        select_one(db, self.query, S::from_raw_query_result).await
    }

    /// Get exactly one item from the Select query, returning
    /// [`DbErr::RecordNotFound`] when no row matches.
    ///
    /// This is the non-optional counterpart to [`one`](Self::one): reach for it
    /// when a missing row is an error rather than an expected `None`, so the
    /// call site can use `?` instead of unwrapping an `Option`.
    pub async fn require_one<C>(self, db: &C) -> Result<S::Item, DbErr>
    where
        C: ConnectionTrait,
    {
        require(self.one(db).await?)
    }

    /// Get all items from the Select query
    pub async fn all<C>(self, db: &C) -> Result<Vec<S::Item>, DbErr>
    where
        C: ConnectionTrait,
    {
        select_all(db, &self.query, S::from_raw_query_result).await
    }
}

impl<S> SelectorRaw<S>
where
    S: SelectorTrait,
{
    pub(crate) fn new(stmt: Statement) -> Self {
        Self {
            stmt,
            selector: PhantomData,
        }
    }

    /// Select a custom Model from a raw SQL [Statement].
    pub fn from_statement<M>(stmt: Statement) -> SelectorRaw<SelectModel<M>>
    where
        M: FromQueryResult,
    {
        SelectorRaw::new(stmt)
    }

    /// Read each row as the model `M`.
    pub fn into_model<M>(self) -> SelectorRaw<SelectModel<M>>
    where
        M: FromQueryResult,
    {
        SelectorRaw::new(self.stmt)
    }

    /// Get the SQL statement
    pub fn into_statement(self) -> Statement {
        self.stmt
    }

    /// Get an item from the Select query
    pub async fn one<C>(self, db: &C) -> Result<Option<S::Item>, DbErr>
    where
        C: ConnectionTrait,
    {
        let row = db.query_one_raw(self.stmt).await?;
        decode_one(row, S::from_raw_query_result)
    }

    /// Get exactly one item from the query, returning [`DbErr::RecordNotFound`]
    /// when no row matches. The non-optional counterpart to [`one`](Self::one).
    pub async fn require_one<C>(self, db: &C) -> Result<S::Item, DbErr>
    where
        C: ConnectionTrait,
    {
        require(self.one(db).await?)
    }

    /// Get all items from the Select query
    pub async fn all<C>(self, db: &C) -> Result<Vec<S::Item>, DbErr>
    where
        C: ConnectionTrait,
    {
        decode_all(db.query_all_raw(self.stmt).await?, S::from_raw_query_result)
    }
}
