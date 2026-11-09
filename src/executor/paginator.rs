use crate::{
    ConnectionTrait, EntityTrait, FromQueryResult, Select, SelectModel, Selector, SelectorRaw,
    SelectorTrait, Statement, error::*,
};
use futures_util::{Stream, StreamExt};
use sql_builder::{Expr, SelectStatement};
use std::{future::Future, marker::PhantomData, pin::Pin};

type PinBoxStream<'db, Item> = Pin<Box<dyn Stream<Item = Item> + 'db>>;

/// Fetches a [`Select`](crate::Select)'s results one page at a time with
/// `LIMIT` / `OFFSET`. Build one with
/// [`PaginatorTrait::paginate`](crate::PaginatorTrait::paginate); use
/// [`fetch_page`](Self::fetch_page) / [`fetch`](Self::fetch) to read rows
/// and [`num_pages`](Self::num_pages) /
/// [`num_items_and_pages`](Self::num_items_and_pages) for totals.
///
/// Add an `ORDER BY` to your query — without one, page contents are not
/// deterministic across calls.
#[derive(Clone, Debug)]
pub struct Paginator<'db, C, S>
where
    C: ConnectionTrait,
    S: SelectorTrait + 'db,
{
    query: PaginationQuery,
    pub(crate) page: u64,
    pub(crate) page_size: u64,
    pub(crate) db: &'db C,
    pub(crate) selector: PhantomData<S>,
}

#[derive(Clone, Debug)]
enum PaginationQuery {
    Select(Box<SelectStatement>),
    Raw(Statement),
}

/// Pair of totals returned by [`Paginator::num_items_and_pages`].
#[derive(Clone, Debug)]
pub struct ItemsAndPagesNumber {
    /// Total number of rows matched by the query.
    pub number_of_items: u64,
    /// Total number of pages at the configured page size.
    pub number_of_pages: u64,
}

impl ItemsAndPagesNumber {
    /// The pages `number_of_items` rows fill at `page_size` (never zero:
    /// `paginate` refuses it).
    pub(crate) fn new(number_of_items: u64, page_size: u64) -> Self {
        Self {
            number_of_items,
            number_of_pages: number_of_items.div_ceil(page_size),
        }
    }
}

/// The page-counter methods every paginator shares, built on its own
/// `fetch_page` and `num_items`; `$item` is the type of one row.
macro_rules! paginator_methods {
    ($item:ty) => {
        /// Fetch the current page
        pub async fn fetch(&self) -> Result<Vec<$item>, $crate::DbErr> {
            self.fetch_page(self.page).await
        }

        /// Get the total number of pages
        pub async fn num_pages(&self) -> Result<u64, $crate::DbErr> {
            Ok(self.num_items_and_pages().await?.number_of_pages)
        }

        /// Get the total number of items and pages
        pub async fn num_items_and_pages(
            &self,
        ) -> Result<$crate::ItemsAndPagesNumber, $crate::DbErr> {
            let number_of_items = self.num_items().await?;
            Ok($crate::ItemsAndPagesNumber::new(
                number_of_items,
                self.page_size,
            ))
        }

        /// Increment the page counter, saturating at `u64::MAX`.
        pub fn next(&mut self) {
            self.page = self.page.saturating_add(1);
        }

        /// Get current page number
        pub fn cur_page(&self) -> u64 {
            self.page
        }

        /// Fetch one page and increment the page counter
        pub async fn fetch_and_next(&mut self) -> Result<Option<Vec<$item>>, $crate::DbErr> {
            let page = self.fetch().await?;
            self.next();
            Ok(Some(page).filter(|page| !page.is_empty()))
        }
    };
}

/// The number of rows `query` selects: `SELECT COUNT(*) FROM (query)`.
async fn count_rows<C: ConnectionTrait>(
    db: &C,
    query: PaginationQuery,
    reset_bounds: bool,
) -> Result<u64, DbErr> {
    let count = match query {
        PaginationQuery::Select(mut query) => {
            query.clear_order_by();
            if reset_bounds {
                query.reset_limit().reset_offset();
            }
            db.get_database_backend().build(
                SelectStatement::new()
                    .expr(Expr::cust("COUNT(*) AS num_items"))
                    .from_subquery(*query, "sub_query"),
            )?
        }
        PaginationQuery::Raw(statement) => super::raw::wrap(statement, "COUNT(*) AS num_items"),
    };
    match db.query_one_raw(count).await? {
        Some(row) => Ok(row.try_get::<i64>("num_items")? as u64),
        None => Ok(0),
    }
}

impl<'db, C, S> Paginator<'db, C, S>
where
    C: ConnectionTrait,
    S: SelectorTrait + 'db,
{
    /// Fetch a specific page; page index starts from zero.
    /// Returns an error if the page offset overflows `u64`.
    pub async fn fetch_page(&self, page: u64) -> Result<Vec<S::Item>, DbErr> {
        let offset = self
            .page_size
            .checked_mul(page)
            .ok_or_else(|| DbErr::Custom("pagination page offset exceeds u64::MAX".into()))?;
        let query = match &self.query {
            PaginationQuery::Select(query) => self
                .db
                .get_database_backend()
                .build(query.as_ref().clone().limit(self.page_size).offset(offset))?,
            PaginationQuery::Raw(statement) => {
                super::raw::limited(statement.clone(), self.page_size, Some(offset))
            }
        };
        super::decode_all(
            self.db.query_all_raw(query).await?,
            S::from_raw_query_result,
        )
    }

    /// Get the total number of items, ignoring builder limits and offsets.
    /// Bounds embedded in raw SQL remain part of that query and are preserved.
    pub async fn num_items(&self) -> Result<u64, DbErr> {
        count_rows(self.db, self.query.clone(), true).await
    }

    paginator_methods!(S::Item);

    /// Set the page counter
    pub fn set_page(&mut self, page: u64) {
        self.page = page;
    }

    /// Convert self into an async stream
    ///
    /// The stream ends after the first page shorter than the page size (an
    /// empty one is not yielded), or after yielding an error, and then keeps
    /// returning `None`.
    pub fn into_stream(self) -> PinBoxStream<'db, Result<Vec<S::Item>, DbErr>> {
        Box::pin(
            futures_util::stream::unfold(Some(self), |streamer| async move {
                let mut streamer = streamer?;
                match streamer.fetch_and_next().await {
                    Ok(Some(vec)) => {
                        let full = vec.len() as u64 >= streamer.page_size;
                        Some((Ok(vec), full.then_some(streamer)))
                    }
                    Ok(None) => None,
                    Err(err) => Some((Err(err), None)),
                }
            })
            .fuse(),
        )
    }
}

/// Extension trait that adds `.paginate(db, page_size)` to anything
/// page-able — [`Select`](crate::Select), [`Selector`], [`SelectorRaw`].
pub trait PaginatorTrait<'db, C>
where
    C: ConnectionTrait,
{
    /// The [`SelectorTrait`] that materialises each row of the page.
    type Selector: SelectorTrait + Send + Sync + 'db;

    /// Paginate the result of a select operation.
    ///
    /// Raw SQL keeps its SQL and bindings: on MySQL, MariaDB and SQLite each
    /// page's `LIMIT` and `OFFSET` follow it, unless it bounds its own rows,
    /// when it is read as a derived table, as it always is on PostgreSQL and
    /// to count it. A derived table's columns need distinct names on MySQL
    /// and MariaDB. It must be a single query; invalid SQL is reported by the
    /// database when executed.
    ///
    /// # Panics
    /// Panics if `page_size` is zero.
    fn paginate(self, db: &'db C, page_size: u64) -> Paginator<'db, C, Self::Selector>;

    /// Count the number of rows this query selects, honoring any `limit` and
    /// `offset` set on it.
    ///
    /// This differs from [`Paginator::num_items`], which resets `limit`/`offset`
    /// to report the grand total for pagination. `count` wraps the query as-is in
    /// a `SELECT COUNT(*) FROM (..)` subquery, so a query with a `limit` counts at
    /// most that many rows.
    fn count(self, db: &'db C) -> impl Future<Output = Result<u64, DbErr>> + Send
    where
        Self: Send + Sized,
    {
        let query = self.paginate(db, 1).query;
        count_rows(db, query, false)
    }
}

impl<'db, C, S> PaginatorTrait<'db, C> for Selector<S>
where
    C: ConnectionTrait,
    S: SelectorTrait + Send + Sync + 'db,
{
    type Selector = S;

    fn paginate(self, db: &'db C, page_size: u64) -> Paginator<'db, C, S> {
        assert!(page_size != 0, "page_size should not be zero");
        Paginator {
            query: PaginationQuery::Select(Box::new(self.query)),
            page: 0,
            page_size,
            db,
            selector: PhantomData,
        }
    }
}

impl<'db, C, S> PaginatorTrait<'db, C> for SelectorRaw<S>
where
    C: ConnectionTrait,
    S: SelectorTrait + Send + Sync + 'db,
{
    type Selector = S;
    fn paginate(self, db: &'db C, page_size: u64) -> Paginator<'db, C, S> {
        assert!(page_size != 0, "page_size should not be zero");
        Paginator {
            query: PaginationQuery::Raw(self.stmt),
            page: 0,
            page_size,
            db,
            selector: PhantomData,
        }
    }
}

impl<'db, C, M, E> PaginatorTrait<'db, C> for Select<E>
where
    C: ConnectionTrait,
    E: EntityTrait<Model = M>,
    M: FromQueryResult + Sized + Send + Sync + 'db,
{
    type Selector = SelectModel<M>;

    fn paginate(self, db: &'db C, page_size: u64) -> Paginator<'db, C, Self::Selector> {
        self.into_model().paginate(db, page_size)
    }
}

#[cfg(all(test, feature = "sqlite", feature = "runtime-tokio"))]
#[path = "../../tests/unit/paginator_tests.rs"]
mod tests;
