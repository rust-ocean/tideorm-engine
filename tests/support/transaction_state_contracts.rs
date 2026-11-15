#![cfg(all(feature = "sqlite", feature = "runtime-tokio"))]
use tideorm_engine::{ConnectionTrait, Database, DbBackend, DbErr, Statement, TransactionTrait};

#[test]
fn sqlite_reports_ended_transaction_and_rebalances_savepoints() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let db = Database::connect("sqlite::memory:").await.unwrap();
            db.execute_unprepared("CREATE TABLE items(id INTEGER PRIMARY KEY)")
                .await
                .unwrap();
            let tx = db.begin().await.unwrap();
            let savepoint = tx.begin().await.unwrap();
            assert!(!savepoint.has_server_transaction_ended().await.unwrap());
            savepoint
                .execute_unprepared("INSERT INTO items VALUES (1)")
                .await
                .unwrap();
            assert!(
                savepoint
                    .execute_unprepared("INSERT OR ROLLBACK INTO items VALUES (1)")
                    .await
                    .is_err()
            );
            assert!(savepoint.has_server_transaction_ended().await.unwrap());
            assert!(tx.has_server_transaction_ended().await.unwrap());
            // The engine refuses what would autocommit, on every level.
            for refused in [
                savepoint
                    .execute_unprepared("INSERT INTO items VALUES (2)")
                    .await
                    .unwrap_err(),
                tx.execute_unprepared("INSERT INTO items VALUES (2)")
                    .await
                    .unwrap_err(),
                savepoint.begin().await.unwrap_err(),
            ] {
                assert!(
                    matches!(&refused, DbErr::TransactionAborted { cause: Some(_), .. })
                        && refused
                            .to_string()
                            .contains("SQLite roll the whole transaction back"),
                    "{refused}"
                );
            }
            savepoint.rollback().await.unwrap();
            let refused = tx.commit().await.unwrap_err();
            assert!(refused.to_string().contains("cannot commit"), "{refused}");
            let count = db
                .query_one_raw(Statement::from_string(
                    DbBackend::SQLite,
                    "SELECT count(*) AS n FROM items",
                ))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(count.try_get::<i64>("n").unwrap(), 0);
            let tx = db.begin().await.unwrap();
            tx.execute_unprepared("INSERT INTO items VALUES (3)")
                .await
                .unwrap();
            tx.commit().await.unwrap();
            db.close().await.unwrap();
        });
}

/// An aborted transaction refuses statements, savepoints and its commit on
/// every level, and rolls back; the next transaction on the connection runs.
#[test]
fn aborted_transactions_only_roll_back() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let db = Database::connect("sqlite::memory:").await.unwrap();
            db.execute_unprepared("CREATE TABLE items(id INTEGER PRIMARY KEY)")
                .await
                .unwrap();
            let session = db.pin_session().await.unwrap();
            let tx = session.begin().await.unwrap();
            tx.execute_unprepared("INSERT INTO items VALUES (1)")
                .await
                .unwrap();
            let savepoint = tx.begin().await.unwrap();
            savepoint.abort("its handle leaked");
            savepoint.abort("kept only once");
            for refused in [
                tx.execute_unprepared("INSERT INTO items VALUES (2)")
                    .await
                    .unwrap_err(),
                savepoint.begin().await.unwrap_err(),
            ] {
                assert!(
                    refused.to_string().contains("its handle leaked"),
                    "{refused}"
                );
            }
            let refused = savepoint.commit().await.unwrap_err();
            assert!(refused.to_string().contains("cannot commit"), "{refused}");
            assert!(tx.commit().await.is_err());

            // Both rolled back, and the session's next transaction is clean.
            let tx = session.begin().await.unwrap();
            tx.execute_unprepared("INSERT INTO items VALUES (3)")
                .await
                .unwrap();
            tx.commit().await.unwrap();
            let count = session
                .query_one_raw(Statement::from_string(
                    DbBackend::SQLite,
                    "SELECT count(*) AS n FROM items",
                ))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(count.try_get::<i64>("n").unwrap(), 1);
            session.close().await.unwrap();
            db.close().await.unwrap();
        });
}

/// Options set a transaction up as it begins, so a savepoint, begun inside
/// one, refuses them before it reaches the server, on a pool's transaction and
/// on a pinned session's alike, and the enclosing transaction goes on.
#[test]
fn savepoints_refuse_transaction_options() {
    use tideorm_engine::{IsolationLevel, SqliteTransactionMode, TransactionOptions};
    let immediate = TransactionOptions {
        sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
        ..TransactionOptions::default()
    };
    let serializable = TransactionOptions {
        isolation_level: Some(IsolationLevel::Serializable),
        ..TransactionOptions::default()
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let db = Database::connect("sqlite::memory:").await.unwrap();
            db.execute_unprepared("CREATE TABLE items(id INTEGER PRIMARY KEY)")
                .await
                .unwrap();
            let tx = db.begin_with_options(immediate).await.unwrap();
            tx.execute_unprepared("INSERT INTO items VALUES (1)")
                .await
                .unwrap();
            for options in [immediate, serializable] {
                let refused = tx.begin_with_options(options).await.unwrap_err();
                assert!(refused.to_string().contains("savepoint"), "{refused}");
            }
            let savepoint = tx.begin().await.unwrap();
            savepoint
                .execute_unprepared("INSERT INTO items VALUES (2)")
                .await
                .unwrap();
            savepoint.commit().await.unwrap();
            tx.commit().await.unwrap();
            let count = db
                .query_one_raw(Statement::from_string(
                    DbBackend::SQLite,
                    "SELECT count(*) AS n FROM items",
                ))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(count.try_get::<i64>("n").unwrap(), 2);

            let session = db.pin_session().await.unwrap();
            let open = session.begin().await.unwrap();
            assert!(open.begin_with_options(immediate).await.is_err());
            open.commit().await.unwrap();
            session
                .begin_with_options(immediate)
                .await
                .unwrap()
                .commit()
                .await
                .unwrap();
            db.close().await.unwrap();
        });
}

/// SQLite cannot keep a transaction from writing, so it refuses a read-only
/// one as it begins, and the pool's connection stays usable.
#[test]
fn sqlite_refuses_read_only_transactions() {
    use tideorm_engine::{AccessMode, TransactionOptions};
    let with_access = |mode| TransactionOptions {
        access_mode: Some(mode),
        ..TransactionOptions::default()
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let db = Database::connect("sqlite::memory:").await.unwrap();
            let refused = db
                .begin_with_options(with_access(AccessMode::ReadOnly))
                .await
                .unwrap_err();
            assert!(
                matches!(refused, DbErr::BackendNotSupported { db: "SQLite", .. }),
                "{refused}"
            );
            let tx = db
                .begin_with_options(with_access(AccessMode::ReadWrite))
                .await
                .unwrap();
            tx.execute_unprepared("CREATE TABLE items(id INTEGER PRIMARY KEY)")
                .await
                .unwrap();
            tx.commit().await.unwrap();
            db.close().await.unwrap();
        });
}

/// A statement on a transaction while a savepoint is open in it waits for the
/// savepoint to end. It used to run inside the savepoint, and was rolled back
/// with it.
#[test]
fn a_statement_waits_for_an_open_savepoint() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let db = Database::connect("sqlite::memory:").await.unwrap();
            db.execute_unprepared("CREATE TABLE items(id INTEGER PRIMARY KEY)")
                .await
                .unwrap();
            let tx = db.begin().await.unwrap();
            let savepoint = tx.begin().await.unwrap();
            savepoint
                .execute_unprepared("INSERT INTO items VALUES (1)")
                .await
                .unwrap();
            let rollback = async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                savepoint.rollback().await
            };
            let (parent, rollback) = futures_util::future::join(
                tx.execute_unprepared("INSERT INTO items VALUES (2)"),
                rollback,
            )
            .await;
            parent.unwrap();
            rollback.unwrap();
            tx.commit().await.unwrap();
            let kept = db
                .query_one_raw(Statement::from_string(
                    DbBackend::SQLite,
                    "SELECT group_concat(id) AS ids FROM items",
                ))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(kept.try_get::<String>("ids").unwrap(), "2");
            db.close().await.unwrap();
        });
}

/// A transaction rolled back while a savepoint of it is still held refuses
/// its statements at once, instead of waiting for that savepoint to end.
#[test]
fn an_ended_transaction_refuses_without_waiting_for_its_savepoint() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let db = Database::connect("sqlite::memory:").await.unwrap();
            let tx = db.begin().await.unwrap();
            let savepoint = tx.begin().await.unwrap();
            tx.rollback_by_ref().await.unwrap();
            let refused = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tx.execute_unprepared("SELECT 1"),
            )
            .await
            .expect("an ended transaction must not wait for its savepoint");
            assert!(refused.is_err());
            // Closing the pool waits for the connection they hold.
            drop((savepoint, tx));
            db.close().await.unwrap();
        });
}
