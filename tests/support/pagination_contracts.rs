use futures_util::{FutureExt, StreamExt};
use std::sync::{Arc, Mutex};
use tideorm::orm::*;

#[derive(Debug, PartialEq)]
struct Id(i64);

impl FromQueryResult for Id {
    fn from_query_result(row: &QueryResult) -> Result<Self, DbErr> {
        Ok(Self(row.try_get("id")?))
    }
}

fn raw(stmt: Statement) -> SelectorRaw<SelectModel<Id>> {
    SelectorRaw::<SelectModel<Id>>::from_statement(stmt)
}

#[derive(Debug, Clone)]
struct Capture {
    backend: DbBackend,
    fail: bool,
    statements: Arc<Mutex<Vec<Statement>>>,
}

impl ConnectionTrait for Capture {
    fn get_database_backend(&self) -> DbBackend {
        self.backend
    }
    async fn execute_raw(&self, _: Statement) -> Result<ExecResult, DbErr> {
        panic!("pagination must not write")
    }
    async fn execute_unprepared(&self, _: &str) -> Result<ExecResult, DbErr> {
        panic!("pagination must use prepared statements")
    }
    async fn query_one_raw(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        self.statements.lock().unwrap().push(stmt);
        if self.fail {
            Err(DbErr::Custom("count failure".into()))
        } else {
            Ok(None)
        }
    }
    async fn query_all_raw(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        self.statements.lock().unwrap().push(stmt);
        if self.fail {
            Err(DbErr::Custom("page failure".into()))
        } else {
            Ok(vec![])
        }
    }
}

#[test]
fn state_sql_laziness_and_errors() {
    async {
        for backend in [DbBackend::Postgres, DbBackend::MySQL, DbBackend::SQLite] {
            let db = Capture {
                backend,
                fail: false,
                statements: Default::default(),
            };
            let placeholder = if backend == DbBackend::Postgres { "$1" } else { "?" };
            let stmt = Statement::from_sql_and_values(
                backend,
                format!("  select id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id  "),
                [2i64.into()],
            );
            let mut pages = raw(stmt.clone()).paginate(&db, 3);
            assert_eq!(pages.cur_page(), 0);
            assert!(db.statements.lock().unwrap().is_empty());
            assert!(pages.fetch_page(2).await.unwrap().is_empty());
            assert_eq!(pages.cur_page(), 0);
            pages.set_page(4);
            assert!(pages.fetch().await.unwrap().is_empty());
            pages.next();
            assert_eq!(pages.cur_page(), 5);
            assert!(pages.fetch_and_next().await.unwrap().is_none());
            assert_eq!(pages.cur_page(), 6);
            assert!(pages.fetch_and_next().await.unwrap().is_none());
            assert_eq!(pages.cur_page(), 7);
            assert_eq!(pages.num_items().await.unwrap(), 0);
            assert_eq!(pages.num_pages().await.unwrap(), 0);
            let totals = pages.num_items_and_pages().await.unwrap();
            assert_eq!(totals.clone().number_of_items, 0);
            assert_eq!(totals.number_of_pages, 0);
            assert_eq!(raw(stmt.clone()).count(&db).await.unwrap(), 0);
            assert_eq!(db.statements.lock().unwrap().len(), 8);
            let stream = raw(stmt.clone()).paginate(&db, 3).into_stream();
            drop(stream);
            assert_eq!(db.statements.lock().unwrap().len(), 8);
            let mut stream = raw(stmt.clone()).paginate(&db, 3).into_stream();
            assert!(stream.next().await.is_none());
            assert!(stream.next().await.is_none());
            assert_eq!(db.statements.lock().unwrap().len(), 9);
            println!("PAGINATION_SQL {:?} {:?}", backend, db.statements.lock().unwrap());

            let failed = Capture { fail: true, ..db.clone() };
            let mut pages = raw(stmt.clone()).paginate(&failed, 3);
            pages.set_page(7);
            assert!(matches!(pages.fetch_and_next().await, Err(DbErr::Custom(m)) if m == "page failure"));
            assert_eq!(pages.cur_page(), 7);
            assert!(matches!(pages.num_items().await, Err(DbErr::Custom(m)) if m == "count failure"));
            assert!(matches!(raw(stmt.clone()).count(&failed).await, Err(DbErr::Custom(m)) if m == "count failure"));
            let before = failed.statements.lock().unwrap().len();
            let mut stream = pages.into_stream();
            assert!(matches!(stream.next().await, Some(Err(DbErr::Custom(m))) if m == "page failure"));
            assert!(stream.next().await.is_none());
            assert_eq!(failed.statements.lock().unwrap().len(), before + 1);
        }
    }
    .now_or_never()
    .expect("scripted connection operations must be immediately ready");
}

#[test]
#[should_panic(expected = "page_size should not be zero")]
fn zero_page_size_panics_before_database_use() {
    let db = Capture {
        backend: DbBackend::SQLite,
        fail: false,
        statements: Default::default(),
    };
    raw(Statement::from_string(DbBackend::SQLite, "SELECT id")).paginate(&db, 0);
}

#[test]
fn overflowing_page_offsets_fail_before_database_use() {
    async {
        let db = Capture {
            backend: DbBackend::SQLite,
            fail: false,
            statements: Default::default(),
        };
        let mut pages =
            raw(Statement::from_string(DbBackend::SQLite, "SELECT 1 AS id")).paginate(&db, 2);
        pages.set_page(u64::MAX);
        assert!(matches!(
            pages.fetch_and_next().await,
            Err(DbErr::Custom(_))
        ));
        assert_eq!(pages.cur_page(), u64::MAX);
        assert!(db.statements.lock().unwrap().is_empty());
        pages.next();
        assert_eq!(pages.cur_page(), u64::MAX);
    }
    .now_or_never()
    .expect("overflow must be detected synchronously");
}

#[test]
fn raw_pagination_preserves_numbered_parameters_and_sql_literals() {
    async {
        let db = Capture {
            backend: DbBackend::Postgres,
            fail: false,
            statements: Default::default(),
        };
        let sql = "SELECT $2 AS id WHERE $1 = $1 AND '$3 ?' = '$3 ?' AND $$a$$ = $$a$$ -- $4 ?";
        let statement = Statement::from_sql_and_values(db.backend, sql, [7i64.into(), 9i64.into()]);
        let pages = raw(statement.clone()).paginate(&db, 2);
        pages.fetch_page(3).await.unwrap();
        pages.num_items().await.unwrap();
        assert!(!raw(statement.clone()).exists(&db).await.unwrap());
        let executable_sql = sql.strip_suffix(" -- $4 ?").unwrap();
        let captured = db.statements.lock().unwrap().clone();
        for captured in &captured {
            assert!(captured.sql.contains(executable_sql));
            assert!(!captured.sql.contains("$4 ?"));
            assert_eq!(captured.db_backend, statement.db_backend);
        }
        // The page's offset is bound after the statement's own values.
        assert!(
            captured[0]
                .sql
                .ends_with(") AS sub_query LIMIT 2 OFFSET $3"),
            "{}",
            captured[0].sql
        );
        let mut paged = statement.values.clone().unwrap();
        paged.0.push(6u64.into());
        assert_eq!(captured[0].values, Some(paged));
        for captured in &captured[1..] {
            assert_eq!(captured.values, statement.values);
        }
        let literal = Statement::from_string(db.backend, "SELECT '; --' AS id");
        raw(literal.clone())
            .paginate(&db, 2)
            .fetch_page(0)
            .await
            .unwrap();
        assert!(
            db.statements
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .sql
                .contains(&literal.sql)
        );
        // A marker without a value is the database's to refuse.
        let unbound = Statement::from_sql_and_values(db.backend, "SELECT $2 AS id", [1i64.into()]);
        assert!(!raw(unbound.clone()).exists(&db).await.unwrap());
        raw(unbound).paginate(&db, 1).fetch().await.unwrap();
        let fetched = db.statements.lock().unwrap().pop().unwrap();
        assert!(
            fetched.sql.ends_with("LIMIT 1 OFFSET $3"),
            "{}",
            fetched.sql
        );
    }
    .now_or_never()
    .unwrap();
}

#[test]
fn mysql_and_sqlite_pages_follow_a_query_that_sets_no_bounds() {
    async {
        for (backend, offset) in [(DbBackend::MySQL, "?"), (DbBackend::SQLite, "?2")] {
            let db = Capture {
                backend,
                fail: false,
                statements: Default::default(),
            };
            for (sql, expected) in [
                (
                    "SELECT a.id, b.id FROM t a, t b WHERE a.id = ? ORDER BY a.id; -- note",
                    format!(
                        "SELECT a.id, b.id FROM t a, t b WHERE a.id = ? ORDER BY a.id LIMIT 2 OFFSET {offset}"
                    ),
                ),
                (
                    "SELECT id FROM (SELECT id FROM t LIMIT 5) AS u WHERE id > ?",
                    format!(
                        "SELECT id FROM (SELECT id FROM t LIMIT 5) AS u WHERE id > ? LIMIT 2 OFFSET {offset}"
                    ),
                ),
                (
                    "SELECT id FROM t WHERE id > ? limit 5",
                    format!(
                        "SELECT * FROM (SELECT id FROM t WHERE id > ? limit 5) AS sub_query LIMIT 2 OFFSET {offset}"
                    ),
                ),
            ] {
                let statement = Statement::from_sql_and_values(backend, sql, [1i64.into()]);
                raw(statement.clone())
                    .paginate(&db, 2)
                    .fetch_page(1)
                    .await
                    .unwrap();
                let fetched = db.statements.lock().unwrap().pop().unwrap();
                assert_eq!(fetched.sql, expected);
                assert_eq!(
                    fetched.values.unwrap().0,
                    [Value::from(1i64), Value::from(2u64)]
                );
                assert!(!raw(statement).exists(&db).await.unwrap());
                let checked = db.statements.lock().unwrap().pop().unwrap();
                let bounded = expected.split(" OFFSET ").next().unwrap();
                assert_eq!(checked.sql, bounded.replace("LIMIT 2", "LIMIT 1"));
            }
        }
        // A plain `?` is numbered past SQLite's numbered and named markers.
        let db = Capture {
            backend: DbBackend::SQLite,
            fail: false,
            statements: Default::default(),
        };
        let statement = Statement::from_sql_and_values(
            db.backend,
            "SELECT $2 AS id WHERE ?3 = ?3",
            [1i64.into(), 2i64.into()],
        );
        raw(statement).paginate(&db, 2).fetch().await.unwrap();
        let fetched = db.statements.lock().unwrap().pop().unwrap();
        assert!(fetched.sql.ends_with("LIMIT 2 OFFSET ?4"), "{}", fetched.sql);
    }
    .now_or_never()
    .unwrap();
}

#[test]
fn mysql_raw_wrapping_uses_mysql_comment_rules() {
    async {
        let db = Capture {
            backend: DbBackend::MySQL,
            fail: false,
            statements: Default::default(),
        };
        for sql in [
            "SELECT 1 AS id # trailing comment",
            "SELECT 1 AS id; # trailing comment",
        ] {
            let statement = Statement::from_string(db.backend, sql);
            assert!(!raw(statement.clone()).exists(&db).await.unwrap());
            assert!(
                raw(statement)
                    .paginate(&db, 1)
                    .fetch()
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        for captured in db.statements.lock().unwrap().iter() {
            assert!(!captured.sql.contains("# trailing comment"));
            assert!(captured.sql.contains("SELECT 1 AS id"));
        }

        let expression = Statement::from_string(db.backend, "SELECT 1 AS id WHERE 1--2 = 2;");
        assert!(!raw(expression.clone()).exists(&db).await.unwrap());
        raw(expression).paginate(&db, 1).fetch().await.unwrap();
        for captured in db.statements.lock().unwrap().iter().skip(4) {
            assert!(captured.sql.contains("1--2 = 2"), "{}", captured.sql);
        }
    }
    .now_or_never()
    .unwrap();
}

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
mod live {
    use super::*;
    use std::time::Duration;

    struct DecodeFailure;

    /// The names of a row's columns.
    struct Names(Vec<String>);
    impl FromQueryResult for Names {
        fn from_query_result(row: &QueryResult) -> Result<Self, DbErr> {
            Ok(Self(row.column_names()))
        }
    }
    impl FromQueryResult for DecodeFailure {
        fn from_query_result(_: &QueryResult) -> Result<Self, DbErr> {
            Err(DbErr::Custom("decode failure".into()))
        }
    }

    async fn contracts(url: &str) {
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(3));
        let db = Database::connect(options).await.unwrap();
        db.execute_unprepared("DROP TABLE IF EXISTS pagination_rows")
            .await
            .unwrap();
        db.execute_unprepared("CREATE TABLE pagination_rows (id BIGINT NOT NULL PRIMARY KEY)")
            .await
            .unwrap();
        db.execute_unprepared("INSERT INTO pagination_rows (id) VALUES (1), (2), (3), (4), (5)")
            .await
            .unwrap();
        let backend = db.get_database_backend();
        let optimizer_before = if matches!(backend, DbBackend::MySQL | DbBackend::MariaDB) {
            Some(
                db.query_one_raw(Statement::from_string(
                    backend,
                    "SELECT @@optimizer_switch AS switches",
                ))
                .await
                .unwrap()
                .unwrap()
                .try_get::<String>("switches")
                .unwrap(),
            )
        } else {
            None
        };
        let placeholder = if backend == DbBackend::Postgres {
            "$1"
        } else {
            "?"
        };
        let stmt = Statement::from_sql_and_values(
            backend,
            format!("SELECT id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id"),
            [2i64.into()],
        );
        // Pagination wraps the complete raw query, preserving CTEs, comments,
        // embedded bounds, and bindings instead of treating SQL as a SELECT list.
        for sql in [
            format!(
                "WITH chosen AS (SELECT id FROM pagination_rows WHERE id >= {placeholder}) SELECT id FROM chosen ORDER BY id LIMIT 3"
            ),
            format!(
                "/* 🌊 SELECT */ SELECT id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id LIMIT 3"
            ),
            format!(
                "SELECT id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id LIMIT 3 -- trailing comment"
            ),
            format!(
                "SELECT id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id LIMIT 3;  "
            ),
            format!(
                "SELECT id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id LIMIT 3; -- trailing comment"
            ),
            format!(
                "SELECT id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id LIMIT 3; /* trailing comment */"
            ),
        ] {
            let query = Statement::from_sql_and_values(backend, sql, [2i64.into()]);
            let pages = raw(query.clone()).paginate(&db, 2);
            assert_eq!(pages.fetch_page(0).await.unwrap(), [Id(2), Id(3)]);
            assert_eq!(pages.fetch_page(1).await.unwrap(), [Id(4)]);
            assert!(pages.fetch_page(2).await.unwrap().is_empty());
            assert_eq!(pages.num_items().await.unwrap(), 3);
            assert_eq!(raw(query).count(&db).await.unwrap(), 3);
        }
        // A query setting no bounds of its own is paged as written, so its
        // columns keep names they share and a CTE runs on MariaDB too.
        for sql in [
            format!(
                "WITH chosen AS (SELECT id FROM pagination_rows WHERE id >= {placeholder}) SELECT id FROM chosen ORDER BY id"
            ),
            format!(
                "/* 🌊 LIMIT */ SELECT id, id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id; -- trailing comment"
            ),
            format!(
                "SELECT id FROM (SELECT id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id LIMIT 4) AS chosen ORDER BY id DESC"
            ),
        ] {
            let query = Statement::from_sql_and_values(backend, sql.clone(), [2i64.into()]);
            let pages = raw(query.clone()).paginate(&db, 2);
            let mut rows = pages.fetch_page(0).await.unwrap();
            rows.extend(pages.fetch_page(1).await.unwrap());
            rows.sort_by_key(|row| row.0);
            assert_eq!(rows, [Id(2), Id(3), Id(4), Id(5)], "{sql}");
            assert!(raw(query).exists(&db).await.unwrap(), "{sql}");
        }
        let shared = Statement::from_sql_and_values(
            backend,
            format!("SELECT id, id FROM pagination_rows WHERE id >= {placeholder} ORDER BY id"),
            [2i64.into()],
        );
        let page: Vec<Names> = SelectorRaw::<SelectModel<Names>>::from_statement(shared)
            .paginate(&db, 2)
            .fetch_page(1)
            .await
            .unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].0, ["id", "id"]);
        let literal_comment = Statement::from_string(
            backend,
            "SELECT id FROM pagination_rows WHERE id = 1 AND '--' = '--';",
        );
        let pages = raw(literal_comment.clone()).paginate(&db, 1);
        assert_eq!(pages.fetch().await.unwrap(), [Id(1)]);
        assert_eq!(pages.num_items().await.unwrap(), 1);
        assert_eq!(raw(literal_comment).count(&db).await.unwrap(), 1);
        for sql in [
            "SELECT 1 AS id -- trailing comment",
            "SELECT 1 AS id; -- trailing comment",
            "SELECT 1 AS id -- trailing comment\n;",
            "SELECT 1 AS id /* trailing comment */",
            "SELECT 1 AS id; /* trailing comment */",
        ] {
            assert!(
                raw(Statement::from_string(backend, sql))
                    .exists(&db)
                    .await
                    .unwrap(),
                "{sql}"
            );
        }
        if backend == DbBackend::SQLite {
            for sql in [
                "SELECT 1 AS id FROM (SELECT 2 AS [value--]) WHERE [value--] = 2;",
                r"SELECT 1 AS id WHERE '\' = '\';",
                "SELECT 1 AS id /* outer /* inner */;",
            ] {
                let query = Statement::from_string(backend, sql);
                let pages = raw(query.clone()).paginate(&db, 1);
                assert_eq!(pages.fetch().await.unwrap(), [Id(1)], "{sql}");
                assert_eq!(pages.num_items().await.unwrap(), 1, "{sql}");
                assert_eq!(raw(query.clone()).count(&db).await.unwrap(), 1, "{sql}");
                assert!(raw(query).exists(&db).await.unwrap(), "{sql}");
            }
            // The driver gives a plain `?` the next value whatever named or
            // numbered markers precede it, so the offset's is numbered.
            for sql in [
                "SELECT id FROM pagination_rows WHERE id >= $1 ORDER BY id",
                "SELECT id FROM pagination_rows WHERE id >= ?1 ORDER BY id",
            ] {
                let query = Statement::from_sql_and_values(backend, sql, [3i64.into()]);
                let pages = raw(query).paginate(&db, 2);
                assert_eq!(pages.fetch_page(1).await.unwrap(), [Id(5)], "{sql}");
            }
        }
        let invalid = raw(Statement::from_string(backend, "SEL")).paginate(&db, 1);
        assert!(matches!(invalid.fetch().await, Err(DbErr::Query(_))));
        for suffix in ["", " LIMIT 3", " LIMIT 3 OFFSET 1"] {
            let statement = Statement::from_string(
                backend,
                format!("SELECT id FROM pagination_rows ORDER BY id DESC{suffix}"),
            );
            let expected = raw(statement.clone()).all(&db).await.unwrap();
            let pages = raw(statement).paginate(&db, 2);
            assert_eq!(pages.fetch_page(0).await.unwrap(), expected[..2]);
            assert_eq!(
                pages.fetch_page(1).await.unwrap(),
                expected[2..expected.len().min(4)]
            );
        }
        if let Some(before) = optimizer_before {
            let after = db
                .query_one_raw(Statement::from_string(
                    backend,
                    "SELECT @@optimizer_switch AS switches",
                ))
                .await
                .unwrap()
                .unwrap()
                .try_get::<String>("switches")
                .unwrap();
            assert_eq!(
                before, after,
                "pagination must not change pooled session settings"
            );
        }
        if backend == DbBackend::Postgres {
            let reordered = Statement::from_sql_and_values(
                backend,
                "SELECT id FROM pagination_rows WHERE id <= $2 AND id >= $1 AND id >= $1 ORDER BY id",
                [2i64.into(), 4i64.into()],
            );
            let pages = raw(reordered).paginate(&db, 2);
            assert_eq!(pages.fetch_page(0).await.unwrap(), [Id(2), Id(3)]);
            assert_eq!(pages.fetch_page(1).await.unwrap(), [Id(4)]);
            assert_eq!(pages.num_items().await.unwrap(), 3);
        }
        let mut pages = raw(stmt.clone()).paginate(&db, 3);
        assert_eq!(pages.num_items().await.unwrap(), 4);
        assert_eq!(pages.num_pages().await.unwrap(), 2);
        let totals = pages.num_items_and_pages().await.unwrap();
        assert_eq!((totals.number_of_items, totals.number_of_pages), (4, 2));
        assert_eq!(raw(stmt.clone()).count(&db).await.unwrap(), 4);
        assert_eq!(pages.fetch_page(1).await.unwrap(), [Id(5)]);
        assert_eq!(pages.cur_page(), 0);
        assert_eq!(pages.fetch().await.unwrap(), [Id(2), Id(3), Id(4)]);
        assert_eq!(
            pages.fetch_and_next().await.unwrap(),
            Some(vec![Id(2), Id(3), Id(4)])
        );
        assert_eq!(pages.fetch_and_next().await.unwrap(), Some(vec![Id(5)]));
        assert!(pages.fetch_and_next().await.unwrap().is_none());
        assert_eq!(pages.cur_page(), 3);
        pages.set_page(1);
        assert_eq!(pages.fetch().await.unwrap(), [Id(5)]);
        // Raw SQL is opaque: embedded limits remain inside count subqueries.
        let limited = Statement::from_string(
            backend,
            "SELECT id FROM pagination_rows ORDER BY id LIMIT 2 OFFSET 1",
        );
        assert_eq!(raw(limited.clone()).count(&db).await.unwrap(), 2);
        assert_eq!(raw(limited).paginate(&db, 3).num_items().await.unwrap(), 2);
        let empty = Statement::from_string(backend, "SELECT id FROM pagination_rows WHERE id < 0");
        let empty_pages = raw(empty).paginate(&db, 3);
        assert_eq!(empty_pages.num_items().await.unwrap(), 0);
        assert_eq!(empty_pages.num_pages().await.unwrap(), 0);
        assert!(empty_pages.into_stream().next().await.is_none());
        // Dropping an unpolled or partly consumed page stream releases all work.
        drop(raw(stmt.clone()).paginate(&db, 2).into_stream());
        let mut stream = raw(stmt.clone()).paginate(&db, 2).into_stream();
        assert_eq!(stream.next().await.unwrap().unwrap(), [Id(2), Id(3)]);
        drop(stream);
        assert_eq!(raw(stmt.clone()).count(&db).await.unwrap(), 4);
        let mut stream = raw(stmt.clone()).paginate(&db, 2).into_stream();
        assert_eq!(stream.next().await.unwrap().unwrap(), [Id(2), Id(3)]);
        assert_eq!(stream.next().await.unwrap().unwrap(), [Id(4), Id(5)]);
        assert!(stream.next().await.is_none());
        assert!(stream.next().await.is_none());
        let mut broken = raw(Statement::from_string(
            backend,
            "SELECT id FROM pagination_missing_table",
        ))
        .paginate(&db, 1)
        .into_stream();
        assert!(matches!(broken.next().await, Some(Err(DbErr::Query(_)))));
        assert!(broken.next().await.is_none());
        let mut decoding = raw(stmt.clone())
            .into_model::<DecodeFailure>()
            .paginate(&db, 1);
        assert!(
            matches!(decoding.fetch_and_next().await, Err(DbErr::Custom(m)) if m == "decode failure")
        );
        assert_eq!(decoding.cur_page(), 0);
        let mut decoding = decoding.into_stream();
        assert!(
            matches!(decoding.next().await, Some(Err(DbErr::Custom(m))) if m == "decode failure")
        );
        assert!(decoding.next().await.is_none());
        assert_eq!(raw(stmt).count(&db).await.unwrap(), 4);
        db.execute_unprepared("DROP TABLE pagination_rows")
            .await
            .unwrap();
        db.close_by_ref().await.unwrap();
        println!("PAGINATION_CONTRACT pages_counts_streams_errors_pool_reuse");
    }

    async fn run() {
        let work = async {
            if let Ok(url) = std::env::var("ENGINE_PROBE_DATABASE_URL") {
                contracts(&url).await;
            } else {
                #[cfg(feature = "sqlite")]
                contracts("sqlite::memory:").await;
                #[cfg(not(feature = "sqlite"))]
                panic!("ENGINE_PROBE_DATABASE_URL is required for live pagination contracts");
            }
        };
        match futures_util::future::select(
            Box::pin(work),
            Box::pin(async_io::Timer::after(Duration::from_secs(30))),
        )
        .await
        {
            futures_util::future::Either::Left(_) => (),
            futures_util::future::Either::Right(_) => panic!("pagination contracts timed out"),
        }
    }

    #[test]
    #[cfg(feature = "runtime-tokio")]
    #[cfg_attr(not(feature = "sqlite"), ignore = "requires a disposable server URL")]
    fn pagination_tokio() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run());
    }

    #[test]
    #[cfg(feature = "runtime-async-std")]
    #[cfg_attr(not(feature = "sqlite"), ignore = "requires a disposable server URL")]
    fn pagination_async_std() {
        async_std::task::block_on(run());
    }
}
