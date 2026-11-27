use crate::{DbErr, Value};
use sql_builder::ValueTuple;

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
