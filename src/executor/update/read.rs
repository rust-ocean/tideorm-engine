//! Read the rows an assignment-free update would match, without firing triggers.

use sql_builder::{ConditionHolderContents, SelectStatement, UpdateStatement};

pub(super) fn matching_rows(
    update: &UpdateStatement,
    mut select: SelectStatement,
) -> SelectStatement {
    debug_assert!(update.get_values().is_empty());
    select
        .from_clear()
        .limit(update.get_limit().unwrap_or(1).min(1));
    if let Some(table) = update.get_table() {
        select.from(table.clone());
    }
    for table in update.get_from() {
        select.from(table.clone());
    }
    match &update.get_where().contents {
        ConditionHolderContents::Empty => {}
        ConditionHolderContents::Chain(conditions) => {
            for condition in conditions {
                select.and_or_where(condition.clone());
            }
        }
        ConditionHolderContents::Condition(condition) => {
            select.cond_where(condition.clone());
        }
    }
    for order in update.get_orders() {
        select.add_order_by(order.clone());
    }
    if let Some(clause) = update.get_with() {
        select.with_cte(clause.clone());
    }
    select
}

#[cfg(test)]
#[path = "../../../tests/unit/update_read_tests.rs"]
mod tests;
