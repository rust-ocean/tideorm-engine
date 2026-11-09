use super::*;
use sql_builder::{
    CommonTableExpression, Expr, ExprTrait, LogicalChainOper, Order, Query, Values, WithClause,
};

#[test]
fn update_read_preserves_chains_sources_ctes_order_and_zero_limit() {
    let cte = CommonTableExpression::new()
        .table_name("source")
        .query(Query::select().expr_as(Expr::val(7), "id").to_owned())
        .to_owned();
    let mut update = Query::update()
        .table("rows")
        .from("source")
        .with_cte(WithClause::new().cte(cte).to_owned())
        .and_or_where(LogicalChainOper::And(Expr::col(("rows", "id")).eq(1)))
        .and_or_where(LogicalChainOper::Or(Expr::col(("rows", "id")).eq(2)))
        .order_by_expr(Expr::val(3), Order::Desc)
        .limit(0)
        .to_owned();
    for backend in [
        crate::DbBackend::Postgres,
        crate::DbBackend::MySQL,
        crate::DbBackend::SQLite,
    ] {
        let select = matching_rows(&update, Query::select().expr(Expr::cust("1")).to_owned());
        let statement = backend.build(&select).expect("the statement renders");
        assert!(statement.sql.contains(" OR "));
        assert!(statement.sql.contains("source"));
        assert!(statement.sql.contains("ORDER BY"));
        assert!(statement.sql.ends_with("LIMIT 0"));
        assert_eq!(
            statement.values,
            Some(Values(vec![7.into(), 1.into(), 2.into(), 3.into()]))
        );
    }
    update.limit(100);
    assert!(
        crate::DbBackend::SQLite
            .build(&matching_rows(
                &update,
                Query::select().expr(Expr::cust("1")).to_owned()
            ))
            .expect("the statement renders")
            .sql
            .ends_with("LIMIT 1")
    );
}
