use std::sync::{
    Arc, PoisonError,
    atomic::{AtomicU8, Ordering},
};

use futures_util::lock::{Mutex, MutexGuard, OwnedMutexGuard};
use tracing::instrument;

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
use crate::connector::backend;
use crate::{
    ConnectionTrait, DbBackend, DbErr, ExecResult, QueryResult, Statement, TransactionOptions,
    TransactionTrait,
    connector::{InnerConnection, on_connection},
    database::SharedConnection,
    error::*,
    metric::Callback,
};

/// A live database transaction.
///
/// Obtain one with
/// [`TransactionTrait::begin`](crate::TransactionTrait::begin) and end it
/// with [`commit`](Self::commit) or [`rollback`](Self::rollback). Like
/// [`DatabaseConnection`](crate::DatabaseConnection) it implements
/// [`ConnectionTrait`](crate::ConnectionTrait), so the engine's query and
/// mutation methods work against it transparently. Calling `begin` on a
/// transaction starts a nested transaction via `SAVEPOINT`.
///
/// A transaction ends its own level and the levels nested in it: a rollback
/// rolls back the nested transactions still open with it, and a commit while
/// one is open is refused and rolls them all back. A nested transaction whose
/// enclosing one ended refuses its statements, and its commit fails.
///
/// Dropped before it ends, it rolls back: at once when its connection is
/// free, and otherwise before the next statement on it. On PostgreSQL and
/// MySQL, a savepoint dropped while it commits or rolls back leaves its
/// connection at a level the driver no longer knows, so the transactions on
/// that connection refuse everything but a rollback from then on. SQLite's
/// driver keeps count through it, and the enclosing transaction goes on.
///
/// A failure that abandons the whole transaction (SQLite rolling it back by
/// itself, a deadlock), or a call to [`abort`](Self::abort), aborts it with
/// every level: from then on their statements, savepoints and commits are
/// refused with [`DbErr::TransactionAborted`], and they only roll back. The
/// commit of a transaction a failed statement aborted on the server
/// (PostgreSQL's "current transaction is aborted") rolls it back and fails
/// the same way.
pub struct DatabaseTransaction {
    conn: Arc<SharedConnection>,
    backend: DbBackend,
    /// Its level on the connection: 1 for a transaction, more for a savepoint.
    depth: usize,
    /// Which transaction it is among those begun on the connection, so that a
    /// level opened later at the same depth is not taken for its own.
    serial: u64,
    /// [`OPEN`], [`ENDING`] while it commits or rolls back, or [`ENDED`].
    state: AtomicU8,
    metric_callback: Option<Callback>,
    /// Taken by each savepoint opened in it until that one ends; see
    /// [`nested`](Self::nested).
    savepoints: Arc<Mutex<()>>,
    /// For a savepoint, the enclosing transaction's `savepoints`, held until
    /// it ends. Fields drop after [`Drop::drop`], so a savepoint waiting
    /// beside a dropped one begins once that one's rollback is on its way.
    slot: std::sync::Mutex<Option<OwnedMutexGuard<()>>>,
}

/// A transaction that has neither committed nor rolled back.
const OPEN: u8 = 0;
/// A transaction whose commit or rollback is running.
const ENDING: u8 = 1;
/// A transaction that committed or rolled back.
const ENDED: u8 = 2;

impl std::fmt::Debug for DatabaseTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DatabaseTransaction")
    }
}

impl DatabaseTransaction {
    /// Begins a transaction on `conn`, or a savepoint inside the one open
    /// there. A begin stopped part-way, by an error or a cancellation, leaves
    /// no level open: the driver rolls back what of it started.
    ///
    /// `options` set up a transaction as it begins, so a savepoint, which
    /// begins inside one (`parent`, by its depth and serial), refuses them.
    #[instrument(level = "trace", skip(conn, metric_callback, slot))]
    pub(crate) async fn begin(
        conn: Arc<SharedConnection>,
        backend: DbBackend,
        metric_callback: Option<Callback>,
        options: TransactionOptions,
        slot: Option<OwnedMutexGuard<()>>,
        parent: Option<(usize, u64)>,
    ) -> Result<DatabaseTransaction, DbErr> {
        let (depth, serial) = {
            let connection = &mut *conn.lock().await;
            conn.ensure_settled(connection).await?;
            if let Some((depth, serial)) = parent {
                conn.ensure_current(depth, serial)?;
                conn.ensure_not_aborted()?;
                if options != TransactionOptions::default() {
                    return Err(query_err(
                        "transaction options apply to a transaction as it begins, \
                         not to a savepoint inside one",
                    ));
                }
            } else {
                conn.clear_abort();
            }
            on_connection!(connection, connection => {
                backend::Driver::begin(connection, options).await
            })?;
            conn.opened(parent)?
        };
        Ok(DatabaseTransaction {
            conn,
            backend,
            depth,
            serial,
            state: AtomicU8::new(OPEN),
            metric_callback,
            savepoints: Arc::default(),
            slot: std::sync::Mutex::new(slot),
        })
    }

    /// The identity of the pool, or session, the transaction was begun on;
    /// see [`DatabaseConnection::identity`](crate::DatabaseConnection::identity).
    pub fn identity(&self) -> u64 {
        self.conn.identity()
    }

    /// Abort the transaction, with the one it is open in and every savepoint
    /// open in it: their statements, savepoints and commits are refused from
    /// then on, with `reason`, and they only roll back. For a failure the
    /// caller tells apart itself, such as a nested transaction's handle
    /// outliving its scope. The first reason given is kept.
    pub fn abort(&self, reason: impl Into<String>) {
        self.conn.abort(reason.into(), None);
    }

    /// Whether the server has ended this transaction while its handle is still
    /// open: SQLite rolls a transaction back by itself after some errors, and
    /// MySQL commits one implicitly before DDL.
    ///
    /// It reads the driver's last known state and sends no SQL. SQLite's is
    /// current once the failing statement returns; a network driver's reflects
    /// the replies it has read so far. Fails once the transaction has been
    /// committed or rolled back.
    pub async fn has_server_transaction_ended(&self) -> Result<bool, DbErr> {
        let mut connection = self.connection().await?;
        on_connection!(&mut *connection, connection => {
            Ok(backend::transaction_ended(connection))
        })
    }

    /// Whether a failed statement has aborted this transaction on the server,
    /// which then refuses every statement and answers `COMMIT` with a
    /// rollback: PostgreSQL's "current transaction is aborted". The other
    /// backends keep a transaction going after a failed statement.
    ///
    /// It reads the replies still pending on the connection and sends no SQL.
    /// Fails once the transaction has been committed or rolled back.
    pub async fn has_server_transaction_failed(&self) -> Result<bool, DbErr> {
        let mut connection = self.connection().await?;
        on_connection!(&mut *connection, connection => {
            backend::transaction_failed(connection).await
        })
    }

    /// Commit a transaction
    #[instrument(level = "trace")]
    pub async fn commit(self) -> Result<(), DbErr> {
        self.end(true).await
    }

    /// Rolls back a transaction explicitly
    pub async fn rollback(self) -> Result<(), DbErr> {
        self.rollback_by_ref().await
    }

    /// Roll the transaction back as [`rollback`](Self::rollback) does, through
    /// a shared reference: for a transaction whose handle is shared, so it
    /// cannot be consumed. Statements and transactions begun on it afterwards
    /// are refused, and dropping it rolls nothing back again.
    #[instrument(level = "trace")]
    pub async fn rollback_by_ref(&self) -> Result<(), DbErr> {
        self.end(false).await
    }

    /// Commits or rolls back the transaction, with the levels still open in
    /// it rolled back first; it then refuses further use. A commit the
    /// transaction cannot make rolls it back instead, and fails. A failed end
    /// leaves it open, so dropping it rolls it back.
    #[allow(unused_variables)]
    async fn end(&self, commit: bool) -> Result<(), DbErr> {
        let connection = &mut *self.conn.lock().await;
        self.ensure_open()?;
        if commit {
            self.conn.ensure_settled(connection).await?;
        }
        let nested_open = match self
            .conn
            .end_levels_above(connection, self.depth, self.serial)
        {
            Ok(nested_open) => nested_open,
            // Its level was rolled back with the transaction it was open in.
            Err(ended) => {
                self.finish();
                return if commit { Err(ended) } else { Ok(()) };
            }
        };
        let refusal = if commit {
            match self.conn.commit_refusal() {
                Some(refusal) => Some(refusal),
                None if nested_open => Some((
                    "the transaction cannot commit while a transaction nested in it is open".into(),
                    None,
                )),
                // PostgreSQL answers the `COMMIT` of a failed transaction with
                // a rollback and no error.
                None => {
                    let failed: bool = on_connection!(connection, connection => {
                        backend::transaction_failed(connection).await
                    })?;
                    failed.then(|| {
                        (
                            "the transaction cannot commit: a statement inside it failed, which \
                             aborted it"
                                .into(),
                            None,
                        )
                    })
                }
            }
        } else {
            None
        };
        self.state.store(ENDING, Ordering::Release);
        let ended = on_connection!(connection, connection => {
            if commit && refusal.is_none() {
                backend::commit(connection).await
            } else {
                backend::rollback(connection).await
            }
        });
        if ended.is_ok() {
            self.conn.ended(self.depth, self.serial);
            self.finish();
        } else {
            self.state.store(OPEN, Ordering::Release);
        }
        match (refusal, ended) {
            (None, ended) => ended,
            (Some((reason, cause)), Ok(())) => Err(DbErr::TransactionAborted {
                reason: format!("{reason}, so it was rolled back"),
                cause,
            }),
            // Still open: dropping it rolls it back.
            (Some((reason, cause)), Err(rollback)) => Err(DbErr::TransactionAborted {
                reason: format!("{reason}, and rolling it back failed: {rollback}"),
                cause,
            }),
        }
    }

    /// Mark the transaction ended, letting a sibling waiting for its slot
    /// begin.
    fn finish(&self) {
        self.state.store(ENDED, Ordering::Release);
        drop(
            self.slot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }

    /// Wait for the connection to run something in this transaction, unless
    /// it has ended or can no longer go on.
    async fn connection(&self) -> Result<MutexGuard<'_, InnerConnection>, DbErr> {
        let mut connection = self.conn.lock().await;
        self.ensure_open()?;
        self.conn.ensure_current(self.depth, self.serial)?;
        self.conn.ensure_settled(&mut connection).await?;
        Ok(connection)
    }

    /// The connection, for a statement, unless the transaction was aborted.
    async fn statement_connection(&self) -> Result<MutexGuard<'_, InnerConnection>, DbErr> {
        let connection = self.connection().await?;
        self.conn.ensure_not_aborted()?;
        Ok(connection)
    }

    /// Abort the transaction when the failure of a statement abandoned all of
    /// it: what runs after would run outside it.
    #[allow(unused_variables)]
    fn note_failure<T>(
        &self,
        connection: &InnerConnection,
        result: Result<T, DbErr>,
    ) -> Result<T, DbErr> {
        if let Err(error) = &result {
            let aborted: Result<bool, DbErr> = on_connection!(connection, connection => {
                Ok(backend::transaction_aborted(connection))
            });
            if aborted.unwrap_or(false) {
                let reason = if self.backend == DbBackend::SQLite {
                    "a failed statement made SQLite roll the whole transaction back"
                } else {
                    "a statement inside it deadlocked, and the database rolled the whole \
                     transaction back"
                };
                self.conn
                    .abort(reason.into(), Some(Arc::new(error.clone())));
            }
        }
        result
    }

    /// Refuse a transaction that was already committed or rolled back.
    fn ensure_open(&self) -> Result<(), DbErr> {
        if self.state.load(Ordering::Acquire) == ENDED {
            Err(conn_err("the transaction has already ended"))
        } else {
            Ok(())
        }
    }

    /// Starts a transaction nested in this one, as a savepoint.
    ///
    /// The driver names a savepoint by how many are open on the connection,
    /// so one ending while a later one is open would end that one in its
    /// place. A savepoint therefore waits until another open in this
    /// transaction ends, holding the transaction's slot meanwhile, which it
    /// takes before the connection so neither waits for the other.
    async fn nested(&self, options: TransactionOptions) -> Result<DatabaseTransaction, DbErr> {
        self.ensure_open()?;
        let slot = Arc::clone(&self.savepoints).lock_owned().await;
        self.ensure_open()?;
        DatabaseTransaction::begin(
            Arc::clone(&self.conn),
            self.backend,
            self.metric_callback.clone(),
            options,
            Some(slot),
            Some((self.depth, self.serial)),
        )
        .await
    }
}

impl Drop for DatabaseTransaction {
    /// Roll back a transaction dropped before it ended, with the levels still
    /// open in it. One dropped while it ended leaves the driver's level count
    /// in doubt (except on SQLite), which for a savepoint stops the
    /// transactions it was open in.
    fn drop(&mut self) {
        match *self.state.get_mut() {
            ENDED => return,
            ENDING if self.depth > 1 => self.conn.unsettle(),
            _ => {}
        }
        self.conn.abandon(self.depth, self.serial);
    }
}

impl ConnectionTrait for DatabaseTransaction {
    fn get_database_backend(&self) -> DbBackend {
        // The connection's handshake, read without locking it.
        if self.conn.is_mariadb() {
            DbBackend::MariaDB
        } else {
            self.backend
        }
    }

    #[instrument(level = "trace", skip(stmt))]
    #[allow(unused_variables)]
    async fn execute_raw(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        let connection = &mut *self.statement_connection().await?;
        let result = on_connection!(connection, connection => {
            backend::execute(connection, &stmt, self.metric_callback.as_ref()).await
        });
        self.note_failure(connection, result)
    }

    #[instrument(level = "trace", skip(sql))]
    #[allow(unused_variables)]
    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        let connection = &mut *self.statement_connection().await?;
        let result = on_connection!(connection, connection => {
            backend::execute_unprepared(connection, sql, self.metric_callback.as_ref()).await
        });
        self.note_failure(connection, result)
    }

    #[instrument(level = "trace", skip(stmt))]
    #[allow(unused_variables)]
    async fn query_one_raw(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        let connection = &mut *self.statement_connection().await?;
        let result = on_connection!(connection, connection => {
            backend::query_one(connection, &stmt, self.metric_callback.as_ref()).await
        });
        self.note_failure(connection, result)
    }

    #[instrument(level = "trace", skip(stmt))]
    #[allow(unused_variables)]
    async fn query_all_raw(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        let connection = &mut *self.statement_connection().await?;
        let result = on_connection!(connection, connection => {
            backend::query_all(connection, &stmt, self.metric_callback.as_ref()).await
        });
        self.note_failure(connection, result)
    }
}

impl TransactionTrait for DatabaseTransaction {
    type Transaction = DatabaseTransaction;

    #[instrument(level = "trace")]
    async fn begin(&self) -> Result<DatabaseTransaction, DbErr> {
        self.nested(TransactionOptions::default()).await
    }

    #[instrument(level = "trace")]
    async fn begin_with_options(
        &self,
        options: TransactionOptions,
    ) -> Result<DatabaseTransaction, DbErr> {
        self.nested(options).await
    }
}
