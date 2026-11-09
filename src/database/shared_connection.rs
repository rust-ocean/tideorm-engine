//! The connection a transaction and the savepoints opened in it share.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};

use futures_util::lock::{Mutex, MutexGuard};

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
use crate::connector::{backend, driver_error_to_conn_err};
use crate::{
    DbErr,
    connector::{InnerConnection, on_connection},
};

/// The connection a transaction and its savepoints share, the transaction
/// owning each level open on it, and the work left by transactions dropped
/// while another future held it.
///
/// A transaction dropped open is rolled back, with the levels open above its
/// own. When the connection is busy at that moment, the rollback waits for
/// whoever takes it next, which sends it before anything else runs there; so
/// dropping a transaction never waits, and never panics.
pub(crate) struct SharedConnection {
    connection: Mutex<InnerConnection>,
    levels: std::sync::Mutex<Levels>,
    /// Set once a savepoint is dropped while it commits or rolls back, on a
    /// connection whose driver then no longer knows which level it is at, so
    /// the transactions on it run nothing more and only roll back.
    unsettled: AtomicBool,
    /// Whether the driver's level count survives an end dropped part-way, as
    /// SQLite's does: its worker answers every end it runs, and skips the
    /// rollback queued for one it ran unanswered. Never unsettled.
    exact_levels: bool,
    /// Why the transaction open on the connection was aborted, and the
    /// failure that aborted it: every level of it then refuses statements,
    /// savepoints and commits, and only rolls back.
    aborted: std::sync::Mutex<Option<(String, Option<Arc<DbErr>>)>>,
    /// Whether the connection reaches MariaDB.
    mariadb: bool,
    /// The identity of the pool, or session, it was taken from.
    identity: u64,
}

impl SharedConnection {
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(dead_code)
    )]
    pub(crate) fn new(connection: InnerConnection, identity: u64) -> Self {
        let (mariadb, exact_levels) = match &connection {
            #[cfg(feature = "mysql")]
            InnerConnection::MySql(connection) => (connection.is_mariadb(), false),
            #[cfg(feature = "sqlite")]
            InnerConnection::Sqlite(_) => (false, true),
            #[allow(unreachable_patterns)]
            _ => (false, false),
        };
        Self {
            connection: Mutex::new(connection),
            levels: std::sync::Mutex::default(),
            unsettled: AtomicBool::new(false),
            exact_levels,
            aborted: std::sync::Mutex::new(None),
            mariadb,
            identity,
        }
    }

    /// The identity of the pool, or session, the connection was taken from.
    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    /// Whether the connection reaches MariaDB, as the server said when it
    /// opened.
    pub(crate) fn is_mariadb(&self) -> bool {
        self.mariadb
    }

    /// Wait for the connection, then queue the rollbacks left for it.
    pub(crate) async fn lock(&self) -> MutexGuard<'_, InnerConnection> {
        let mut connection = self.connection.lock().await;
        self.settle(&mut connection);
        connection
    }

    /// Note the level a transaction begun in `parent` (its depth and serial;
    /// none at the top) opened, and return the new level's depth and serial.
    /// Fails when `parent` ended meanwhile, dropped on another thread: the
    /// new level is then rolled back with it.
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(dead_code)
    )]
    pub(crate) fn opened(&self, parent: Option<(usize, u64)>) -> Result<(usize, u64), DbErr> {
        let mut levels = self.levels();
        let depth = match parent {
            Some((depth, serial)) if levels.owns(depth, serial) => depth,
            Some(_) => {
                levels.abandoned += 1;
                return Err(ended_with_parent());
            }
            None => 0,
        };
        levels.open.truncate(depth);
        levels.serials += 1;
        let serial = levels.serials;
        levels.open.push(serial);
        Ok((depth + 1, serial))
    }

    /// Refuse a transaction whose level ended with the transaction it was
    /// open in.
    pub(crate) fn ensure_current(&self, depth: usize, serial: u64) -> Result<(), DbErr> {
        if self.levels().owns(depth, serial) {
            Ok(())
        } else {
            Err(ended_with_parent())
        }
    }

    /// Queue the rollbacks of the levels open above the one at `depth`, which
    /// is about to end: the driver ends whichever level is innermost. Whether
    /// there were any; fails when the level is no longer the transaction's.
    pub(crate) fn end_levels_above(
        &self,
        connection: &mut InnerConnection,
        depth: usize,
        serial: u64,
    ) -> Result<bool, DbErr> {
        let above = {
            let mut levels = self.levels();
            if !levels.owns(depth, serial) {
                return Err(ended_with_parent());
            }
            levels.abandon_from(depth + 1)
        };
        self.settle(connection);
        Ok(above > 0)
    }

    /// Note that the level at `depth` ended.
    pub(crate) fn ended(&self, depth: usize, serial: u64) {
        let mut levels = self.levels();
        if levels.owns(depth, serial) {
            levels.open.truncate(depth - 1);
        } else {
            // An enclosing transaction dropped meanwhile counted this level
            // among the rollbacks it left, which cannot have run yet: the
            // connection was held all along.
            levels.abandoned = levels.abandoned.saturating_sub(1);
        }
    }

    /// Roll back the level at `depth` with the levels open above it, unless
    /// it ended already: now if the connection is free, or else before
    /// whatever takes it next runs.
    pub(crate) fn abandon(&self, depth: usize, serial: u64) {
        {
            let mut levels = self.levels();
            if !levels.owns(depth, serial) {
                return;
            }
            levels.abandon_from(depth);
        }
        if let Some(mut connection) = self.connection.try_lock() {
            self.settle(&mut connection);
        }
    }

    /// Close the connection once whatever holds it lets go. The rollbacks
    /// still queued need not run: closing it ends every transaction on it.
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(dead_code)
    )]
    pub(crate) async fn close(&self) -> Result<(), DbErr> {
        let mut connection = self.connection.lock().await;
        {
            let mut levels = self.levels();
            levels.open.clear();
            levels.abandoned = 0;
        }
        match std::mem::replace(&mut *connection, InnerConnection::Closed) {
            #[cfg(feature = "mysql")]
            InnerConnection::MySql(connection) => {
                connection.close().await.map_err(driver_error_to_conn_err)
            }
            #[cfg(feature = "postgres")]
            InnerConnection::Postgres(connection) => {
                connection.close().await.map_err(driver_error_to_conn_err)
            }
            #[cfg(feature = "sqlite")]
            InnerConnection::Sqlite(connection) => {
                connection.close().await.map_err(driver_error_to_conn_err)
            }
            InnerConnection::Closed => Ok(()),
        }
    }

    /// Note that a savepoint ended unseen; see [`ensure_settled`](Self::ensure_settled).
    /// Where the driver's level count is exact, the savepoint ended or its
    /// queued rollback ends it, and the transaction it was open in goes on.
    pub(crate) fn unsettle(&self) {
        if !self.exact_levels {
            self.unsettled.store(true, Ordering::Release);
        }
    }

    /// Refuse to run anything but a rollback once a savepoint ended unseen,
    /// until no transaction is open on the connection: none of the engine's,
    /// and none by the driver's count or the server's. A transaction still
    /// open stays refused even when the driver reports none: the rollback
    /// queued for the savepoint may have ended it instead.
    #[allow(unused_variables)]
    pub(crate) async fn ensure_settled(
        &self,
        connection: &mut InnerConnection,
    ) -> Result<(), DbErr> {
        if !self.unsettled.load(Ordering::Acquire) {
            return Ok(());
        }
        if self.levels().open.is_empty() {
            let idle: bool = on_connection!(connection, connection => {
                backend::transaction_idle(connection).await
            })?;
            if idle {
                self.unsettled.store(false, Ordering::Release);
                return Ok(());
            }
        }
        Err(DbErr::TransactionAborted {
            reason: "a nested transaction was cancelled while it committed or rolled back, which \
                     leaves the transaction it was open in unusable: roll it back and retry it"
                .into(),
            cause: None,
        })
    }

    /// Abort the transaction open on the connection, keeping the first reason
    /// given.
    pub(crate) fn abort(&self, reason: String, cause: Option<Arc<DbErr>>) {
        self.aborted().get_or_insert((reason, cause));
    }

    /// Forget the abort of a transaction that ended, as the next one begins.
    #[cfg_attr(
        not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
        allow(dead_code)
    )]
    pub(crate) fn clear_abort(&self) {
        *self.aborted() = None;
    }

    /// Refuse a statement, or a savepoint, in an aborted transaction.
    pub(crate) fn ensure_not_aborted(&self) -> Result<(), DbErr> {
        match &*self.aborted() {
            Some((reason, cause)) => Err(DbErr::TransactionAborted {
                reason: format!(
                    "the statement was not run: {reason}; further statements on this \
                     transaction are refused, so retry the whole transaction"
                ),
                cause: cause.clone(),
            }),
            None => Ok(()),
        }
    }

    /// What the commit of an aborted transaction fails with.
    pub(crate) fn commit_refusal(&self) -> Option<DbErr> {
        self.aborted()
            .as_ref()
            .map(|(reason, cause)| DbErr::TransactionAborted {
                reason: format!("the transaction cannot commit: {reason}, so it was rolled back"),
                cause: cause.clone(),
            })
    }

    fn aborted(&self) -> std::sync::MutexGuard<'_, Option<(String, Option<Arc<DbErr>>)>> {
        self.aborted.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn levels(&self) -> std::sync::MutexGuard<'_, Levels> {
        self.levels.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queue the rollbacks left by the transactions dropped while the
    /// connection was held, which the driver sends before its next statement.
    #[allow(unused_variables)]
    fn settle(&self, connection: &mut InnerConnection) {
        let abandoned = std::mem::take(&mut self.levels().abandoned);
        if abandoned == 0 {
            return;
        }
        let _: Result<(), DbErr> = on_connection!(connection, connection => {
            for _ in 0..abandoned {
                backend::start_rollback(connection);
            }
            Ok(())
        });
    }
}

/// The levels open on a connection, which the driver numbers and ends
/// innermost first, each with the transaction that began it, so that a
/// transaction ends its own level and no other.
#[derive(Default)]
struct Levels {
    /// The serial of the transaction owning each level, outermost first.
    open: Vec<u64>,
    /// Levels taken off `open` whose rollbacks are not queued yet.
    abandoned: usize,
    /// The serials handed out so far.
    serials: u64,
}

impl Levels {
    /// Whether the level at `depth` is the one transaction `serial` began.
    fn owns(&self, depth: usize, serial: u64) -> bool {
        depth > 0 && self.open.get(depth - 1) == Some(&serial)
    }

    /// Take the levels from `depth` up off `open`, to roll back, and count
    /// them.
    fn abandon_from(&mut self, depth: usize) -> usize {
        let count = self.open.len().saturating_sub(depth - 1);
        self.open.truncate(depth - 1);
        self.abandoned += count;
        count
    }
}

/// What a transaction whose level was rolled back with the transaction it
/// was open in fails with.
fn ended_with_parent() -> DbErr {
    DbErr::TransactionAborted {
        reason: "the transaction it was open in has ended, which rolled this one back".into(),
        cause: None,
    }
}
