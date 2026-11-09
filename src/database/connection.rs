use std::future::Future;

use crate::{DbBackend, DbErr, ExecResult, QueryResult, Statement, StatementBuilder};

/// A connection (or transaction) that can run queries against the database.
///
/// Implemented by [`DatabaseConnection`](crate::DatabaseConnection),
/// [`DatabaseTransaction`](crate::DatabaseTransaction) and
/// [`DatabaseExecutor`](crate::DatabaseExecutor). Most query and mutation
/// methods in the engine (`.one(db)`, `.all(db)`, `.exec(db)`, ...) take any
/// `&impl ConnectionTrait`, so the same code works on a pool or a
/// transaction.
///
/// Its futures are `Send`, so code generic over a connection can run on a
/// multi-threaded runtime; an implementation writes them as `async fn`.
pub trait ConnectionTrait: Sync {
    /// Get the database backend for the connection. This depends on feature flags enabled.
    fn get_database_backend(&self) -> DbBackend;

    /// Execute a [Statement]
    fn execute_raw(
        &self,
        stmt: Statement,
    ) -> impl Future<Output = Result<ExecResult, DbErr>> + Send;

    /// Execute a [`StatementBuilder`]
    fn execute<S: StatementBuilder>(
        &self,
        stmt: &S,
    ) -> impl Future<Output = Result<ExecResult, DbErr>> + Send {
        let statement = stmt.build(&self.get_database_backend());
        async move { self.execute_raw(statement?).await }
    }

    /// Execute a unprepared [Statement]
    fn execute_unprepared(
        &self,
        sql: &str,
    ) -> impl Future<Output = Result<ExecResult, DbErr>> + Send;

    /// Execute a [Statement] and return a single row of `QueryResult`
    fn query_one_raw(
        &self,
        stmt: Statement,
    ) -> impl Future<Output = Result<Option<QueryResult>, DbErr>> + Send;

    /// Execute a [`StatementBuilder`] and return a single row of `QueryResult`
    fn query_one<S: StatementBuilder>(
        &self,
        stmt: &S,
    ) -> impl Future<Output = Result<Option<QueryResult>, DbErr>> + Send {
        let statement = stmt.build(&self.get_database_backend());
        async move { self.query_one_raw(statement?).await }
    }

    /// Execute a [Statement] and return a vector of `QueryResult`
    fn query_all_raw(
        &self,
        stmt: Statement,
    ) -> impl Future<Output = Result<Vec<QueryResult>, DbErr>> + Send;

    /// Execute a [`StatementBuilder`] and return a vector of `QueryResult`
    fn query_all<S: StatementBuilder>(
        &self,
        stmt: &S,
    ) -> impl Future<Output = Result<Vec<QueryResult>, DbErr>> + Send {
        let statement = stmt.build(&self.get_database_backend());
        async move { self.query_all_raw(statement?).await }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
/// Isolation level
pub enum IsolationLevel {
    /// Consistent reads within the same transaction read the snapshot established by the first read.
    RepeatableRead,
    /// Each consistent read, even within the same transaction, sets and reads its own fresh snapshot.
    ReadCommitted,
    /// SELECT statements are performed in a nonlocking fashion, but a possible earlier version of a row might be used.
    ReadUncommitted,
    /// All statements of the current transaction can only see rows committed before the first query or data-modification statement was executed in this transaction.
    Serializable,
}

impl std::fmt::Display for IsolationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IsolationLevel::RepeatableRead => write!(f, "REPEATABLE READ"),
            IsolationLevel::ReadCommitted => write!(f, "READ COMMITTED"),
            IsolationLevel::ReadUncommitted => write!(f, "READ UNCOMMITTED"),
            IsolationLevel::Serializable => write!(f, "SERIALIZABLE"),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
/// Access mode
pub enum AccessMode {
    /// Data can't be modified in this transaction
    ReadOnly,
    /// Data can be modified in this transaction (default)
    ReadWrite,
}

impl std::fmt::Display for AccessMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessMode::ReadOnly => write!(f, "READ ONLY"),
            AccessMode::ReadWrite => write!(f, "READ WRITE"),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
/// Which kind of transaction to start. Only supported by SQLite.
/// <https://www.sqlite.org/lang_transaction.html>
pub enum SqliteTransactionMode {
    /// The default. Transaction starts when the next statement is executed, and
    /// will be a read or write transaction depending on that statement.
    Deferred,
    /// Start a write transaction as soon as the BEGIN statement is received.
    Immediate,
    /// Start a write transaction as soon as the BEGIN statement is received.
    /// When in non-WAL mode, also block all other transactions from reading the
    /// database.
    Exclusive,
}

impl SqliteTransactionMode {
    /// The keyword used to start a transaction in this mode (the word coming after "BEGIN").
    pub fn sqlite_keyword(&self) -> &'static str {
        match self {
            SqliteTransactionMode::Deferred => "DEFERRED",
            SqliteTransactionMode::Immediate => "IMMEDIATE",
            SqliteTransactionMode::Exclusive => "EXCLUSIVE",
        }
    }
}

/// Configuration for starting a transaction
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct TransactionOptions {
    /// Isolation level for the new transaction
    pub isolation_level: Option<IsolationLevel>,
    /// Access mode for the new transaction. SQLite refuses
    /// [`AccessMode::ReadOnly`].
    pub access_mode: Option<AccessMode>,
    /// Transaction mode (deferred, immediate, exclusive) for the new transaction. Supported only by SQLite.
    pub sqlite_transaction_mode: Option<SqliteTransactionMode>,
}

/// Begin a database transaction.
///
/// Implemented by [`DatabaseConnection`](crate::DatabaseConnection) and
/// [`DatabaseTransaction`](crate::DatabaseTransaction) (allowing nested
/// transactions via SAVEPOINTs). [`begin`](Self::begin) opens a transaction
/// that is committed or rolled back by hand.
///
/// [`Self::Transaction`] is a fixed point — its own transaction type is
/// itself — so a transaction nested at any depth has one type.
pub trait TransactionTrait: Sync {
    /// The concrete type for the transaction
    type Transaction: ConnectionTrait + TransactionTrait<Transaction = Self::Transaction> + Send;

    /// Execute SQL `BEGIN` transaction.
    /// Returns a Transaction that can be committed or rolled back
    fn begin(&self) -> impl Future<Output = Result<Self::Transaction, DbErr>> + Send;

    /// Execute SQL `BEGIN` transaction with isolation level and/or access mode.
    /// Returns a Transaction that can be committed or rolled back
    fn begin_with_options(
        &self,
        options: TransactionOptions,
    ) -> impl Future<Output = Result<Self::Transaction, DbErr>> + Send;
}
