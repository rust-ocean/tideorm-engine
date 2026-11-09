use crate::{QueryTrait, Statement, database::*};

/// Glue type used by the [`debug_query!`](crate::debug_query) /
/// [`debug_query_stmt!`](crate::debug_query_stmt) macros to render a query
/// for a specific backend or connection. Most code should use those macros
/// rather than naming this type directly.
#[derive(Debug)]
pub struct DebugQuery<'a, Q, T> {
    /// The query to render.
    pub query: &'a Q,
    /// Either a [`DbBackend`] or a connection to take the backend from.
    pub value: T,
}

macro_rules! debug_query_build {
    ($impl_obj:ty, $db_expr:expr) => {
        impl<'a, Q> DebugQuery<'a, Q, $impl_obj>
        where
            Q: QueryTrait,
        {
            /// This macro builds a [Statement] when invoked
            pub fn build(&self) -> Result<Statement, crate::DbErr> {
                let func = $db_expr;
                let db_backend = func(self);
                self.query.build(db_backend)
            }
        }
    };
}

debug_query_build!(DbBackend, |x: &DebugQuery<_, DbBackend>| x.value);
debug_query_build!(&DbBackend, |x: &DebugQuery<_, &DbBackend>| *x.value);
debug_query_build!(DatabaseConnection, |x: &DebugQuery<
    _,
    DatabaseConnection,
>| x.value.get_database_backend());
debug_query_build!(&DatabaseConnection, |x: &DebugQuery<
    _,
    &DatabaseConnection,
>| x.value.get_database_backend());

/// Helper to get a `Statement` from an object that impl `QueryTrait`.
#[macro_export]
macro_rules! debug_query_stmt {
    ($query:expr,$value:expr) => {
        $crate::DebugQuery {
            query: $query,
            value: $value,
        }
        .build();
    };
}

/// Helper to get a raw SQL string from an object that impl `QueryTrait`.
#[macro_export]
macro_rules! debug_query {
    ($query:expr,$value:expr) => {
        $crate::debug_query_stmt!($query, $value).map(|statement| statement.to_string());
    };
}
