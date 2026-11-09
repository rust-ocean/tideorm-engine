#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
pub use driver::error::Error as DriverError;

#[cfg(feature = "mysql")]
pub use driver::mysql::MySqlDatabaseError as MySqlDriverError;

#[cfg(feature = "postgres")]
pub use driver::postgres::PgDatabaseError as PostgresDriverError;

#[cfg(feature = "sqlite")]
pub use driver::sqlite::SqliteError as SqliteDriverError;

use std::sync::Arc;
use thiserror::Error;

/// An error from unsuccessful database operations
#[derive(Error, Debug, Clone)]
pub enum DbErr {
    /// This error can happen when the connection pool is fully-utilized
    #[error("Failed to acquire connection from pool: {0}")]
    ConnectionAcquire(#[source] ConnAcquireErr),
    /// Runtime type conversion error
    #[error("Error converting `{from}` into `{into}`: {source}")]
    TryIntoErr {
        /// From type
        from: &'static str,
        /// Into type
        into: &'static str,
        /// TryError
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
    /// There was a problem with the database connection
    #[error("Connection Error: {0}")]
    Conn(#[source] RuntimeErr),
    /// An operation did not execute successfully
    #[error("Execution Error: {0}")]
    Exec(#[source] RuntimeErr),
    /// An error occurred while performing a query
    #[error("Query Error: {0}")]
    Query(#[source] RuntimeErr),
    /// Type error: the specified type cannot be converted from u64. This is not a runtime error.
    #[error("Type '{0}' cannot be converted from u64")]
    ConvertFromU64(&'static str),
    /// After an insert statement it was impossible to retrieve the last_insert_id
    #[error("Failed to unpack last_insert_id")]
    UnpackInsertId,
    /// When updating, a model should know its primary key to check
    /// if the record has been correctly updated, otherwise this error will occur
    #[error("Failed to get primary key from model")]
    UpdateGetPrimaryKey,
    /// The record was not found in the database
    #[error("RecordNotFound Error: {0}")]
    RecordNotFound(String),
    /// Thrown by `TryFrom<ActiveModel>`, which assumes all attributes are set/unchanged
    #[error("Attribute {0} is NotSet")]
    AttrNotSet(String),
    /// A custom error
    #[error("Custom Error: {0}")]
    Custom(String),
    /// Error occurred while parsing value as target type
    #[error("Type Error: {0}")]
    Type(String),
    /// Error occurred while parsing json value as target type
    #[error("Json Error: {0}")]
    Json(String),
    /// None of the records are inserted,
    /// that probably means all of them conflict with existing records in the table
    #[error("None of the records are inserted")]
    RecordNotInserted,
    /// None of the records are updated, that means a WHERE condition has no matches.
    /// May be the table is empty or the record does not exist
    #[error("None of the records are updated")]
    RecordNotUpdated,
    /// This operation is not supported by the database backend
    #[error("Operation not supported by backend {db}: {ctx}")]
    BackendNotSupported {
        /// Database backend
        db: &'static str,
        /// Context
        ctx: &'static str,
    },
    /// (Primary) Key arity mismatch
    #[error("Key arity mismatch: expected {expected}, received {received}")]
    KeyArityMismatch {
        /// Expected value
        expected: u8,
        /// Received value
        received: u8,
    },
    /// Primay key not set for update / delete
    #[error("Primay key not set for {ctx}")]
    PrimaryKeyNotSet {
        /// Context
        ctx: &'static str,
    },
    /// A statement refused, before it reached the server, because the
    /// transaction it would run in was already rolled back or aborted
    #[error("Transaction aborted: {reason}")]
    TransactionAborted {
        /// Why the transaction can run no more statements
        reason: String,
        /// The failure that aborted it, when a statement's failure did
        #[source]
        cause: Option<Arc<DbErr>>,
    },
    /// A column whose value the Rust type it is read as cannot hold
    #[error("column '{column}' cannot be read: {source}")]
    Decode {
        /// The column's name, or its index
        column: String,
        /// Why the driver could not decode the value
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
}

/// An error from trying to get a row from a Model
#[derive(Debug)]
pub enum TryGetError {
    /// A database error was encountered as defined in [crate::DbErr]
    DbErr(DbErr),
    /// A null value was encountered
    Null(String),
}

/// Connection Acquire error
#[derive(Error, Debug, PartialEq, Eq, Copy, Clone)]
pub enum ConnAcquireErr {
    /// Connection pool timed out
    #[error("Connection pool timed out")]
    Timeout,
    /// Connection closed
    #[error("Connection closed")]
    ConnectionClosed,
}

/// Runtime error
#[derive(Error, Debug, Clone)]
pub enum RuntimeErr {
    /// A driver error
    #[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
    #[error("{0}")]
    DriverError(Arc<driver::error::Error>),
    /// Error generated from within the engine
    #[error("{0}")]
    Internal(String),
}

impl PartialEq for DbErr {
    fn eq(&self, other: &Self) -> bool {
        self.to_string() == other.to_string()
    }
}

impl Eq for DbErr {}

pub(crate) fn conn_err<T>(s: T) -> DbErr
where
    T: ToString,
{
    DbErr::Conn(RuntimeErr::Internal(s.to_string()))
}

pub(crate) fn query_err<T>(s: T) -> DbErr
where
    T: ToString,
{
    DbErr::Query(RuntimeErr::Internal(s.to_string()))
}

pub(crate) fn type_err<T>(s: T) -> DbErr
where
    T: ToString,
{
    DbErr::Type(s.to_string())
}

pub(crate) fn json_err<T>(s: T) -> DbErr
where
    T: ToString,
{
    DbErr::Json(s.to_string())
}
