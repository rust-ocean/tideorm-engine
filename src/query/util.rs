use crate::{Condition, DbErr, ExprTrait, Identity, Value};
use sql_builder::{ColumnRef, DynIden, Expr, IntoColumnRef, TableName, TableRef, ValueTuple};

/// `values` as a key; a key of no column is an error.
pub(crate) fn value_tuple(values: impl IntoIterator<Item = Value>) -> Result<ValueTuple, DbErr> {
    let mut values = values.into_iter();
    Ok(match (values.next(), values.next(), values.next()) {
        (None, _, _) => return Err(DbErr::Type("Identity zero?".into())),
        (Some(a), None, _) => ValueTuple::One(a),
        (Some(a), Some(b), None) => ValueTuple::Two(a, b),
        (Some(a), Some(b), Some(c)) => match values.next() {
            None => ValueTuple::Three(a, b, c),
            Some(d) => ValueTuple::Many([a, b, c, d].into_iter().chain(values).collect()),
        },
    })
}

/// Constructs a `WHERE (c1, c2, ...) IN ((v11, v12, ...), (v21, v22, ...), ...)` expression.
/// Degenerates to `WHERE col IN (v1, v2, ...)` when arity = 1.
pub fn column_tuple_in_condition(
    table: &TableRef,
    to: &Identity,
    keys: &[ValueTuple],
) -> Result<Condition, DbErr> {
    let arity = to.arity();
    let values = unique_keys(keys)
        .map(|key| {
            let values = checked_key(arity, key)?.clone().into_iter().map(Expr::val);
            Ok(tuple_or_single(values.collect()))
        })
        .collect::<Result<Vec<_>, DbErr>>()?;
    let columns = tuple_or_single(create_table_columns(table, to));
    Ok(columns.is_in(values).into())
}

/// `expressions` as one: the only one, or a tuple of them.
fn tuple_or_single(expressions: Vec<Expr>) -> Expr {
    match <[Expr; 1]>::try_from(expressions) {
        Ok([expression]) => expression,
        Err(expressions) => Expr::tuple(expressions),
    }
}

/// `keys` without repeats, each kept where it first appears.
pub(crate) fn unique_keys(keys: &[ValueTuple]) -> impl Iterator<Item = &ValueTuple> {
    let mut seen = std::collections::HashSet::new();
    keys.iter().filter(move |key| seen.insert(*key))
}

/// `key`, if it has one value for each of `arity` columns.
pub(crate) fn checked_key(arity: usize, key: &ValueTuple) -> Result<&ValueTuple, DbErr> {
    if key.arity() == arity {
        Ok(key)
    } else {
        Err(DbErr::Type(format!(
            "arity mismatch: expected {arity}, got {} in {key:?}",
            key.arity()
        )))
    }
}

/// The name a table reference goes by: the table's own name (without its
/// schema), or the alias of a subquery, value list or function call.
pub fn table_iden(table: &TableRef) -> &DynIden {
    match table {
        TableRef::Table(TableName(_, name), _)
        | TableRef::SubQuery(_, name)
        | TableRef::ValuesList(_, name)
        | TableRef::FunctionCall(_, name) => name,
    }
}

pub fn table_column(tbl: &TableRef, col: &DynIden) -> ColumnRef {
    (table_iden(tbl).to_owned(), col.clone()).into_column_ref()
}

/// Create a vector of `Expr::col` from the table and identity, e.g. [Expr::col((table, col1)), Expr::col((table, col2)), ...]
pub fn create_table_columns(table: &TableRef, cols: &Identity) -> Vec<Expr> {
    cols.iter()
        .map(|col| table_column(table, col))
        .map(Expr::col)
        .collect()
}
