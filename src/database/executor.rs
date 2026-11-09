use crate::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, DbErr, ExecResult,
    QueryResult, Statement, TransactionOptions, TransactionTrait,
};

/// Either a borrowed [`DatabaseConnection`] / [`DatabaseTransaction`], or an
/// owned [`DatabaseTransaction`].
///
/// Implements [`ConnectionTrait`] and [`TransactionTrait`], so APIs that
/// need to accept "any of those three" can take a `DatabaseExecutor` and
/// not worry about which variant the caller had.
#[derive(Debug)]
pub enum DatabaseExecutor<'c> {
    /// Borrowed connection — use against a long-lived pool.
    Connection(&'c DatabaseConnection),
    /// Borrowed transaction — caller still owns the transaction handle.
    Transaction(&'c DatabaseTransaction),
    /// Owned transaction — the executor's holder commits or rolls it back
    /// when it is done.
    OwnedTransaction(DatabaseTransaction),
}

impl<'c> From<&'c DatabaseConnection> for DatabaseExecutor<'c> {
    fn from(conn: &'c DatabaseConnection) -> Self {
        Self::Connection(conn)
    }
}

impl<'c> From<&'c DatabaseTransaction> for DatabaseExecutor<'c> {
    fn from(trans: &'c DatabaseTransaction) -> Self {
        Self::Transaction(trans)
    }
}

/// Evaluates `$body` with `$target` bound to the connection or transaction
/// the executor runs on.
macro_rules! on_target {
    ($executor:expr, $target:ident => $body:expr) => {
        match $executor {
            DatabaseExecutor::Connection($target) => $body,
            DatabaseExecutor::Transaction($target) => $body,
            DatabaseExecutor::OwnedTransaction($target) => $body,
        }
    };
}

// Boxed: a caller's future holds every statement's, down to the driver's, and
// unbroken its layout nests past the compiler's query depth limit in the
// caller's crate (a model's generated methods, an application's `main`).
impl ConnectionTrait for DatabaseExecutor<'_> {
    fn get_database_backend(&self) -> DbBackend {
        on_target!(self, target => target.get_database_backend())
    }

    fn execute_raw(
        &self,
        stmt: Statement,
    ) -> impl Future<Output = Result<ExecResult, DbErr>> + Send {
        Box::pin(async move { on_target!(self, target => target.execute_raw(stmt).await) })
    }

    fn execute_unprepared(
        &self,
        sql: &str,
    ) -> impl Future<Output = Result<ExecResult, DbErr>> + Send {
        Box::pin(async move { on_target!(self, target => target.execute_unprepared(sql).await) })
    }

    fn query_one_raw(
        &self,
        stmt: Statement,
    ) -> impl Future<Output = Result<Option<QueryResult>, DbErr>> + Send {
        Box::pin(async move { on_target!(self, target => target.query_one_raw(stmt).await) })
    }

    fn query_all_raw(
        &self,
        stmt: Statement,
    ) -> impl Future<Output = Result<Vec<QueryResult>, DbErr>> + Send {
        Box::pin(async move { on_target!(self, target => target.query_all_raw(stmt).await) })
    }
}

impl TransactionTrait for DatabaseExecutor<'_> {
    type Transaction = DatabaseTransaction;

    async fn begin(&self) -> Result<DatabaseTransaction, DbErr> {
        on_target!(self, target => target.begin().await)
    }

    async fn begin_with_options(
        &self,
        options: TransactionOptions,
    ) -> Result<DatabaseTransaction, DbErr> {
        on_target!(self, target => target.begin_with_options(options).await)
    }
}

/// Conversion into a [`DatabaseExecutor`]. Implemented for
/// `&DatabaseConnection`, `&DatabaseTransaction`, and owned
/// `DatabaseTransaction` — let users hand any of them to functions that
/// take `impl IntoDatabaseExecutor<'_>`.
pub trait IntoDatabaseExecutor<'c>: Send
where
    Self: 'c,
{
    /// Build the [`DatabaseExecutor`].
    fn into_database_executor(self) -> DatabaseExecutor<'c>;
}

impl<'c> IntoDatabaseExecutor<'c> for DatabaseExecutor<'c> {
    fn into_database_executor(self) -> DatabaseExecutor<'c> {
        self
    }
}

impl<'c> IntoDatabaseExecutor<'c> for &'c DatabaseConnection {
    fn into_database_executor(self) -> DatabaseExecutor<'c> {
        DatabaseExecutor::Connection(self)
    }
}

impl<'c> IntoDatabaseExecutor<'c> for &'c DatabaseTransaction {
    fn into_database_executor(self) -> DatabaseExecutor<'c> {
        DatabaseExecutor::Transaction(self)
    }
}

impl IntoDatabaseExecutor<'static> for DatabaseTransaction {
    fn into_database_executor(self) -> DatabaseExecutor<'static> {
        DatabaseExecutor::OwnedTransaction(self)
    }
}

impl DatabaseExecutor<'_> {
    /// Returns `true` if this executor is backed by a transaction (borrowed or owned).
    pub fn is_transaction(&self) -> bool {
        matches!(
            self,
            DatabaseExecutor::Transaction(_) | DatabaseExecutor::OwnedTransaction(_)
        )
    }
}
