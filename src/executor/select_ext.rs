use std::future::Future;

use crate::{ConnectionTrait, DbErr, EntityTrait, Select, Selector, SelectorRaw, SelectorTrait};
use sql_builder::{Expr, SelectStatement};

/// Helper trait for selectors with convenient methods
pub trait SelectExt {
    /// This method is unstable and is only used for internal testing.
    /// It may be removed in the future.
    #[doc(hidden)]
    fn exists_query(self) -> SelectStatement;
    /// Check if any records exist
    fn exists<C>(self, db: &C) -> impl Future<Output = Result<bool, DbErr>> + Send
    where
        C: ConnectionTrait,
        Self: Send + Sized,
    {
        let stmt = self.exists_query();
        async move { Ok(db.query_one(&stmt).await?.is_some()) }
    }
}

fn into_exists_query(mut stmt: SelectStatement) -> SelectStatement {
    stmt.clear_selects();
    // Expr::Custom has fewer branches, but this may not have any significant impact on performance.
    stmt.expr(Expr::cust("1"));
    // Without a limit MySQL sends every matching row, however few are read.
    stmt.limit(1);
    stmt.reset_offset();
    stmt.clear_order_by();
    stmt
}

impl<S> SelectExt for Selector<S>
where
    S: SelectorTrait,
{
    fn exists_query(self) -> SelectStatement {
        into_exists_query(self.query)
    }
}

impl<S> SelectExt for SelectorRaw<S>
where
    S: SelectorTrait,
{
    fn exists_query(self) -> SelectStatement {
        let stmt = self.stmt;
        let source = super::raw::sub_query_source(&stmt.sql, stmt.db_backend);
        let exists_sql = format!("1 {source} LIMIT 1");

        let mut query = SelectStatement::new();
        query.expr(if let Some(values) = stmt.values {
            stmt.db_backend
                .dialect()
                .expression(exists_sql, values.0)
                .unwrap_or_else(Expr::Invalid)
        } else {
            Expr::cust(exists_sql)
        });
        query
    }

    /// Runs the raw SQL with its bindings as they are, unlike
    /// [`exists_query`](SelectExt::exists_query): a query builder reads its
    /// `?` and `$N` markers again, collapsing `$$` and `??` and rejecting a
    /// marker without a value.
    async fn exists<C>(self, db: &C) -> Result<bool, DbErr>
    where
        C: ConnectionTrait,
        Self: Send + Sized,
    {
        let statement = super::raw::limited(self.stmt, 1, None);
        Ok(db.query_one_raw(statement).await?.is_some())
    }
}

impl<E> SelectExt for Select<E>
where
    E: EntityTrait,
{
    fn exists_query(self) -> SelectStatement {
        into_exists_query(self.query)
    }
}
