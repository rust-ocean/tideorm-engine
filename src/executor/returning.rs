//! Writes that read their rows back through `RETURNING`.
use crate::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, FromQueryResult, Iterable,
    PrimaryKeyToColumn, StatementBuilder, error::*,
};
use sql_builder::{BinOper, Expr, ExprTrait, InsertStatement, Query, ReturningClause};

/// The backend of `db`, or an error naming `clause` (such as
/// `"INSERT RETURNING"`) when the write cannot return its rows there.
pub(super) fn returning_backend<C>(
    db: &C,
    supported: bool,
    clause: &'static str,
) -> Result<DbBackend, DbErr>
where
    C: ConnectionTrait,
{
    let backend = db.get_database_backend();
    if supported {
        Ok(backend)
    } else {
        Err(DbErr::BackendNotSupported {
            db: backend.as_str(),
            ctx: clause,
        })
    }
}

/// Whether `insert` can read back the rows it writes on `db`: wherever
/// [`DbBackend::supports_returning`] says so, and on MariaDB when it has no
/// `ON DUPLICATE KEY` clause, whose `RETURNING` reports the existing row even
/// when it left it alone.
pub(super) fn insert_returns<C: ConnectionTrait>(db: &C, insert: &InsertStatement) -> bool {
    match db.get_database_backend() {
        DbBackend::MariaDB => !insert.has_on_conflict(),
        backend => backend.supports_returning(),
    }
}

/// `RETURNING` every column of `E`, each read the way a select reads it.
pub(super) fn all_columns<E: EntityTrait>(backend: DbBackend) -> ReturningClause {
    Query::returning().exprs(E::Column::iter().map(|column| returning_column(column, backend)))
}

/// `RETURNING` the primary key of `E`.
pub(super) fn primary_key<E: EntityTrait>(backend: DbBackend) -> ReturningClause {
    Query::returning()
        .exprs(E::PrimaryKey::iter().map(|key| returning_column(key.into_column(), backend)))
}

/// A read conversion can change the result's name (for example, CAST on SQLite).
/// Models and primary keys decode by column name, so preserve it with a quoted alias.
fn returning_column<C: ColumnTrait>(column: C, backend: DbBackend) -> Expr {
    let expr = column.select_enum_as(column.into_returning_expr(backend));
    let name = Expr::col(column);
    if expr == name {
        expr
    } else {
        expr.binary(BinOper::As, name)
    }
}

/// Runs `statement` and reads the first row it returns as a model of `E`.
pub(super) async fn one<E, C, S>(statement: &S, db: &C) -> Result<Option<E::Model>, DbErr>
where
    E: EntityTrait,
    C: ConnectionTrait,
    S: StatementBuilder,
{
    let row = db.query_one(statement).await?;
    super::decode_one(row, |row| E::Model::from_query_result(&row))
}

/// Runs `statement` and reads every row it returns as a model of `E`.
pub(super) async fn all<E, C, S>(statement: &S, db: &C) -> Result<Vec<E::Model>, DbErr>
where
    E: EntityTrait,
    C: ConnectionTrait,
    S: StatementBuilder,
{
    let rows = db.query_all(statement).await?;
    super::decode_all(rows, |row| E::Model::from_query_result(&row))
}
