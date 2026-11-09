use super::select::ColumnSelectExt;
use crate::{ColumnAsExpr, ColumnTrait, IntoIdentity, IntoSimpleExpr};
pub use sql_builder::{
    Condition, ConditionalStatement, DynIden, JoinType, Order, OrderedStatement,
};
use sql_builder::{
    Expr, IntoCondition, IntoIden, LockBehavior, LockType, NullOrdering, SelectExpr,
    SelectStatement, SimpleExpr,
};

use sql_builder::IntoColumnRef;

/// Methods for narrowing a query's projection, locking its rows, and adding
/// `GROUP BY` / `HAVING` clauses. Implemented for [`Select`](crate::Select).
pub trait QuerySelect: Sized {
    /// The underlying `sql_builder` statement type (typically [`SelectStatement`]).
    type QueryStatement;

    /// Add the select SQL statement
    fn query(&mut self) -> &mut SelectStatement;

    /// Clear the selection list
    fn select_only(mut self) -> Self {
        self.query().clear_selects();
        self
    }

    /// Add a select column
    ///
    /// Enum column will be casted into text (PostgreSQL only)
    fn column<C>(mut self, col: C) -> Self
    where
        C: ColumnTrait,
    {
        self.query().expr(col.into_select_expr());
        self
    }

    /// Add a select column with alias
    fn column_as<C, I>(mut self, col: C, alias: I) -> Self
    where
        C: ColumnAsExpr,
        I: IntoIdentity,
    {
        self.query().expr(SelectExpr {
            expr: col.into_column_as_expr(),
            alias: Some(DynIden::from(alias.into_identity())),
            window: None,
        });
        self
    }

    /// Select columns
    ///
    /// Conditionally select all columns expect a specific column
    ///
    /// Enum column will be casted into text (PostgreSQL only)
    fn columns<C, I>(mut self, cols: I) -> Self
    where
        C: ColumnTrait,
        I: IntoIterator<Item = C>,
    {
        for col in cols.into_iter() {
            self = self.column(col);
        }
        self
    }

    /// Add an offset expression. Passing in None would remove the offset.
    fn offset<T>(mut self, offset: T) -> Self
    where
        T: Into<Option<u64>>,
    {
        if let Some(offset) = offset.into() {
            self.query().offset(offset);
        } else {
            self.query().reset_offset();
        }
        self
    }

    /// Add a limit expression. Passing in None would remove the limit.
    fn limit<T>(mut self, limit: T) -> Self
    where
        T: Into<Option<u64>>,
    {
        if let Some(limit) = limit.into() {
            self.query().limit(limit);
        } else {
            self.query().reset_limit();
        }
        self
    }

    /// Add a group by column
    fn group_by<C>(mut self, col: C) -> Self
    where
        C: IntoSimpleExpr,
    {
        self.query().add_group_by([col.into_simple_expr()]);
        self
    }

    /// Add an AND HAVING expression
    fn having<F>(mut self, filter: F) -> Self
    where
        F: IntoCondition,
    {
        self.query().cond_having(filter.into_condition());
        self
    }

    /// Add a DISTINCT expression
    fn distinct(mut self) -> Self {
        self.query().distinct();
        self
    }

    /// Add a DISTINCT ON expression
    /// NOTE: only PostgreSQL supports `DISTINCT ON`
    fn distinct_on<T, I>(mut self, cols: I) -> Self
    where
        T: IntoColumnRef,
        I: IntoIterator<Item = T>,
    {
        self.query().distinct_on(cols);
        self
    }

    /// Select lock
    fn lock(mut self, lock_type: LockType) -> Self {
        self.query().lock(lock_type);
        self
    }

    /// Select lock shared
    fn lock_shared(mut self) -> Self {
        self.query().lock_shared();
        self
    }

    /// Select lock exclusive
    fn lock_exclusive(mut self) -> Self {
        self.query().lock_exclusive();
        self
    }

    /// Row locking with behavior (if supported).
    ///
    /// See [`SelectStatement::lock_with_behavior`].
    fn lock_with_behavior(mut self, r#type: LockType, behavior: LockBehavior) -> Self {
        self.query().lock_with_behavior(r#type, behavior);
        self
    }

    /// Add an expression to the select expression list.
    fn expr<T>(mut self, expr: T) -> Self
    where
        T: Into<SelectExpr>,
    {
        self.query().expr(expr);
        self
    }

    /// Add select expressions from vector of [`SelectExpr`].
    fn exprs<T, I>(mut self, exprs: I) -> Self
    where
        T: Into<SelectExpr>,
        I: IntoIterator<Item = T>,
    {
        self.query().exprs(exprs);
        self
    }

    /// Select column.
    fn expr_as<T, A>(mut self, expr: T, alias: A) -> Self
    where
        T: Into<SimpleExpr>,
        A: IntoIdentity,
    {
        self.query().expr_as(expr, alias.into_identity());
        self
    }

    /// Shorthand of `expr_as(Expr::col((T, C)), A)`.
    fn tbl_col_as<T, C, A>(mut self, (tbl, col): (T, C), alias: A) -> Self
    where
        T: IntoIden + 'static,
        C: IntoIden + 'static,
        A: IntoIdentity,
    {
        self.query()
            .expr_as(Expr::col((tbl, col)), alias.into_identity());
        self
    }
}

/// Methods for adding `ORDER BY` clauses to a query.
pub trait QueryOrder: Sized {
    /// The underlying `sql_builder` statement type.
    type QueryStatement: OrderedStatement;

    /// Add the query to perform an ORDER BY operation
    fn query(&mut self) -> &mut SelectStatement;

    /// Add an order_by expression
    fn order_by<C>(mut self, col: C, ord: Order) -> Self
    where
        C: IntoSimpleExpr,
    {
        self.query().order_by_expr(col.into_simple_expr(), ord);
        self
    }

    /// Add an order_by expression (ascending)
    fn order_by_asc<C>(mut self, col: C) -> Self
    where
        C: IntoSimpleExpr,
    {
        self.query()
            .order_by_expr(col.into_simple_expr(), Order::Asc);
        self
    }

    /// Add an order_by expression (descending)
    fn order_by_desc<C>(mut self, col: C) -> Self
    where
        C: IntoSimpleExpr,
    {
        self.query()
            .order_by_expr(col.into_simple_expr(), Order::Desc);
        self
    }

    /// Add an order_by expression with nulls ordering option
    fn order_by_with_nulls<C>(mut self, col: C, ord: Order, nulls: NullOrdering) -> Self
    where
        C: IntoSimpleExpr,
    {
        self.query()
            .order_by_expr_with_nulls(col.into_simple_expr(), ord, nulls);
        self
    }
}

/// Methods for adding `WHERE` / `AND` / `OR` conditions to a query. The
/// entry point most code uses is [`filter`](Self::filter).
pub trait QueryFilter: Sized {
    /// The underlying `sql_builder` statement type.
    type QueryStatement: ConditionalStatement;

    /// Add the query to perform a FILTER on
    fn query(&mut self) -> &mut Self::QueryStatement;

    /// Add an `AND WHERE` condition: a column comparison, a [`Condition`]
    /// tree built at runtime, or any `sql_builder` expression.
    fn filter<F>(mut self, filter: F) -> Self
    where
        F: IntoCondition,
    {
        self.query().cond_where(filter.into_condition());
        self
    }

    /// Like [`Self::filter`], but without consuming self
    fn filter_mut<F>(&mut self, filter: F)
    where
        F: IntoCondition,
    {
        self.query().cond_where(filter.into_condition());
    }
}
