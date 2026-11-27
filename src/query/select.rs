use crate::{
    ColumnTrait, EntityTrait, Iterable, Order, PrimaryKeyToColumn, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait,
};
use core::fmt::Debug;
use core::marker::PhantomData;
use sql_builder::{FunctionCall, IntoColumnRef, IntoIden, SelectExpr, SelectStatement, SimpleExpr};

/// A `SELECT` query against entity `E`. Returned by
/// [`EntityTrait::find`](crate::EntityTrait::find); chain filters, joins,
/// ordering, and projections onto it, then run it on a
/// [`ConnectionTrait`](crate::ConnectionTrait) with `.one(db)` /
/// `.all(db)` / `.paginate(db, n)`.
#[derive(Clone, Debug)]
pub struct Select<E>
where
    E: EntityTrait,
{
    pub(crate) query: SelectStatement,
    pub(crate) entity: PhantomData<E>,
}

/// Conversion into a [`SimpleExpr`]. Implemented for entity columns so they
/// can be used anywhere a `sql_builder` expression is expected.
pub trait IntoSimpleExpr {
    /// Build the [`SimpleExpr`].
    fn into_simple_expr(self) -> SimpleExpr;
}

/// Like [`IntoSimpleExpr`] but applies the engine's enum-to-text cast for
/// `ActiveEnum` columns appearing in a SELECT list.
pub trait ColumnAsExpr: IntoSimpleExpr {
    /// Build the [`SimpleExpr`], casting `ActiveEnum` columns to text;
    /// otherwise identical to [`IntoSimpleExpr::into_simple_expr`].
    fn into_column_as_expr(self) -> SimpleExpr;
}

macro_rules! impl_query_trait {
    ( $trait: ident ) => {
        impl<E> $trait for Select<E>
        where
            E: EntityTrait,
        {
            type QueryStatement = SelectStatement;

            fn query(&mut self) -> &mut SelectStatement {
                &mut self.query
            }
        }
    };
}

impl_query_trait!(QuerySelect);
impl_query_trait!(QueryFilter);
impl_query_trait!(QueryOrder);

impl<C> ColumnAsExpr for C
where
    C: ColumnTrait,
{
    fn into_column_as_expr(self) -> SimpleExpr {
        self.select_enum_as(self.as_column_ref().into_column_ref().into())
    }
}

impl ColumnAsExpr for SimpleExpr {
    fn into_column_as_expr(self) -> SimpleExpr {
        self.into_simple_expr()
    }
}

impl<C> IntoSimpleExpr for C
where
    C: ColumnTrait,
{
    fn into_simple_expr(self) -> SimpleExpr {
        SimpleExpr::Column(self.as_column_ref().into_column_ref())
    }
}

impl IntoSimpleExpr for SimpleExpr {
    fn into_simple_expr(self) -> SimpleExpr {
        self
    }
}

impl IntoSimpleExpr for FunctionCall {
    fn into_simple_expr(self) -> SimpleExpr {
        SimpleExpr::FunctionCall(self)
    }
}

pub(crate) trait ColumnSelectExt: ColumnTrait {
    fn into_select_expr(self) -> SelectExpr;
}

impl<C> ColumnSelectExt for C
where
    C: ColumnTrait,
{
    fn into_select_expr(self) -> SelectExpr {
        let column = self.into_expr();
        let expr = self.select_enum_as(column.clone());

        let alias = match &expr {
            SimpleExpr::Column(_) if expr == column => None,
            SimpleExpr::AsEnum(_, inner) if inner.as_ref() == &column => None,
            _ => Some(self.as_str().into_iden()),
        };

        SelectExpr {
            expr,
            alias,
            window: None,
        }
    }
}

impl<E> Select<E>
where
    E: EntityTrait,
{
    pub(crate) fn new() -> Self {
        Self {
            query: SelectStatement::new(),
            entity: PhantomData,
        }
        .prepare_select()
        .prepare_from()
    }

    fn prepare_select(mut self) -> Self {
        self.query.exprs(Self::column_list());
        self
    }

    fn column_list() -> Vec<SelectExpr> {
        E::Column::iter()
            .map(ColumnSelectExt::into_select_expr)
            .collect()
    }

    fn prepare_from(mut self) -> Self {
        self.query.from(E::default().table_ref());
        self
    }

    /// Apply order by primary key to the query statement
    pub fn order_by_id(mut self, order: Order) -> Self {
        for key in E::PrimaryKey::iter() {
            let col = key.into_column();
            self.query
                .order_by_expr(col.into_simple_expr(), order.clone());
        }
        self
    }
}

builds_statement!(SelectStatement, Select<E: EntityTrait>);
