//! A SQLite batch of default rows is one statement, with either runtime.
#![cfg(all(
    feature = "sqlite",
    any(feature = "runtime-tokio", feature = "runtime-async-std")
))]

use tideorm_engine::{ConnectionTrait, Database, sql_builder::Query};

async fn exercise() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    db.execute_unprepared("CREATE TABLE defaults (id INTEGER PRIMARY KEY)")
        .await
        .unwrap();
    let mut insert = Query::insert()
        .into_table("defaults")
        .or_default_values_many(3)
        .to_owned();
    let result = db.execute(&insert).await.unwrap();
    assert_eq!(result.rows_affected(), 3);
    assert_eq!(result.last_insert_id(), 3);
    insert.returning(Query::returning().column("id"));
    let rows = db.query_all(&insert).await.unwrap();
    let keys: Vec<i32> = rows.iter().map(|row| row.try_get("id").unwrap()).collect();
    assert_eq!(keys, [4, 5, 6]);
    db.execute_unprepared("CREATE TRIGGER stop_batch BEFORE INSERT ON defaults WHEN (SELECT COUNT(*) FROM defaults) >= 7 BEGIN SELECT RAISE(ABORT, 'stop'); END").await.unwrap();
    assert!(db.query_all(&insert).await.is_err());
    let rows = db
        .query_all(&Query::select().column("id").from("defaults").to_owned())
        .await
        .unwrap();
    assert_eq!(rows.len(), 6);
    db.execute_unprepared("DROP TRIGGER stop_batch; CREATE TRIGGER stop_batch BEFORE INSERT ON defaults WHEN (SELECT COUNT(*) FROM defaults) >= 7 BEGIN SELECT RAISE(ROLLBACK, 'rolled back by trigger'); END").await.unwrap();
    let error = db.query_all(&insert).await.unwrap_err();
    assert!(
        error.to_string().contains("rolled back by trigger"),
        "{error}"
    );
    db.close().await.unwrap();
}

#[cfg(feature = "runtime-tokio")]
#[test]
fn default_rows_on_tokio() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(exercise());
}

#[cfg(feature = "runtime-async-std")]
#[test]
fn default_rows_on_async_std() {
    async_std::task::block_on(exercise());
}
