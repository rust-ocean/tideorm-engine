use crate::{DbBackend, Statement, StatementBuilder};

/// Common operations on a query builder: borrow the underlying
/// `sql_builder` statement, build it into a backend-specific [`Statement`], or
/// apply optional modifications via [`apply_if`](Self::apply_if).
///
/// Implemented by [`Select`](crate::Select), [`Insert`](crate::Insert),
/// [`Update`](crate::Update), [`Delete`](crate::Delete), and their multi-row
/// variants.
pub trait QueryTrait {
    /// The underlying `sql_builder` statement type this builder produces.
    type QueryStatement: StatementBuilder;

    /// Mutable access to the underlying statement.
    fn query(&mut self) -> &mut Self::QueryStatement;

    /// Shared access to the underlying statement.
    fn as_query(&self) -> &Self::QueryStatement;

    /// Consume the builder and return the underlying statement.
    fn into_query(self) -> Self::QueryStatement;

    /// Render the query for `db_backend` as a [`Statement`] (SQL + bound
    /// parameters). Useful for inspecting generated SQL in tests.
    fn build(&self, db_backend: DbBackend) -> Result<Statement, crate::DbErr> {
        StatementBuilder::build(self.as_query(), &db_backend)
    }

    /// Apply an operation on the [QueryTrait::QueryStatement] if the given `Option<T>` is `Some(_)`
    fn apply_if<T, F>(self, val: Option<T>, if_some: F) -> Self
    where
        Self: Sized,
        F: FnOnce(Self, T) -> Self,
    {
        if let Some(val) = val {
            if_some(self, val)
        } else {
            self
        }
    }
}
