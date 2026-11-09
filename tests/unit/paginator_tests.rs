use super::*;
use crate::{Database, QueryResult};

#[derive(Debug, PartialEq)]
struct Id(i64);

impl FromQueryResult for Id {
    fn from_query_result(row: &QueryResult) -> Result<Self, DbErr> {
        Ok(Self(row.try_get("id")?))
    }
}

#[test]
fn a_short_page_ends_the_stream() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(async {
            let db = Database::connect("sqlite::memory:")
                .await
                .expect("an in-memory database");
            let run = async |sql: &str| db.execute_unprepared(sql).await.expect("the SQL runs");
            run("CREATE TABLE ids (id INTEGER PRIMARY KEY)").await;
            run("INSERT INTO ids (id) VALUES (1), (2), (3), (4)").await;
            let select =
                Statement::from_string(crate::DbBackend::SQLite, "SELECT id FROM ids ORDER BY id");
            let mut stream = SelectorRaw::<SelectModel<Id>>::from_statement::<Id>(select)
                .paginate(&db, 3)
                .into_stream();
            let mut page = async || stream.next().await.expect("a page").expect("rows");
            assert_eq!(page().await, [Id(1), Id(2), Id(3)]);
            assert_eq!(page().await, [Id(4)]);
            // These rows would fill the next page, which the stream no longer reads.
            run("INSERT INTO ids (id) VALUES (5), (6), (7)").await;
            assert!(stream.next().await.is_none());
            drop(stream);
            db.close().await.expect("the database closes");
        });
}
