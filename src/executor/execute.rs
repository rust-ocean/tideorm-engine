/// Result of a non-`SELECT` statement: an `INSERT`, `UPDATE`, `DELETE`, or
/// DDL execution. Carries the row count
/// ([`rows_affected`](Self::rows_affected)) and, where the backend supports
/// it, the last auto-generated primary key
/// ([`last_insert_id`](Self::last_insert_id)).
#[derive(Debug)]
pub struct ExecResult {
    pub(crate) result: ExecResultHolder,
}

/// Holds a result depending on the database backend chosen by the feature flag
#[derive(Debug)]
pub(crate) enum ExecResultHolder {
    /// Holds the result of executing an operation on a MySQL database
    #[cfg(feature = "mysql")]
    MySql(driver::mysql::MySqlQueryResult),
    /// Holds the result of executing an operation on a PostgreSQL database
    #[cfg(feature = "postgres")]
    Postgres(driver::postgres::PgQueryResult),
    /// Holds the result of executing an operation on a SQLite database
    #[cfg(feature = "sqlite")]
    Sqlite(driver::sqlite::SqliteQueryResult),
}

// ExecResult //

impl ExecResult {
    /// The affected rows and insert id the driver reported, read through
    /// the drivers' common result contract (generically, since MySQL's own
    /// `last_insert_id`, a `u64`, would shadow the contract's).
    fn driver_outcome(&self) -> (u64, Option<i128>) {
        #[cfg(any(feature = "mysql", feature = "postgres", feature = "sqlite"))]
        fn read<R: driver_core::database::QueryResult>(result: &R) -> (u64, Option<i128>) {
            (result.rows_affected(), result.last_insert_id())
        }
        match &self.result {
            #[cfg(feature = "mysql")]
            ExecResultHolder::MySql(result) => read(result),
            #[cfg(feature = "postgres")]
            ExecResultHolder::Postgres(result) => read(result),
            #[cfg(feature = "sqlite")]
            ExecResultHolder::Sqlite(result) => read(result),
            #[allow(unreachable_patterns)]
            _ => unreachable!(),
        }
    }

    /// The auto-increment primary key value assigned by the database on the
    /// most recent `INSERT`.
    ///
    /// # Panics
    ///
    /// PostgreSQL does not expose `last_insert_id` directly — use
    /// `exec_with_returning` / `exec_with_returning_keys` instead. A negative
    /// SQLite rowid also panics; [`try_last_insert_id`](Self::try_last_insert_id)
    /// returns an error for both.
    pub fn last_insert_id(&self) -> u64 {
        match self.driver_outcome().1 {
            None => panic!("Should not retrieve last_insert_id this way"),
            // MySQL's identifiers are `u64`s and SQLite's `i64`s, so only a
            // negative rowid does not fit.
            Some(id) => {
                u64::try_from(id).unwrap_or_else(|_| unreachable!("negative last_insert_rowid"))
            }
        }
    }

    /// [`last_insert_id`](Self::last_insert_id), failing with
    /// [`DbErr::UnpackInsertId`](crate::DbErr::UnpackInsertId) on PostgreSQL
    /// and for a negative SQLite rowid instead of panicking.
    pub fn try_last_insert_id(&self) -> Result<u64, crate::DbErr> {
        self.driver_outcome()
            .1
            .and_then(|id| u64::try_from(id).ok())
            .ok_or(crate::DbErr::UnpackInsertId)
    }

    /// Number of rows affected by the statement.
    pub fn rows_affected(&self) -> u64 {
        self.driver_outcome().0
    }
}
