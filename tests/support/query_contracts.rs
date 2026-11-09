use tideorm::orm::*;

#[test]
fn conversion_and_index_contracts() {
    assert_eq!(i8::try_from_u64(127).unwrap(), 127);
    assert!(matches!(
        i8::try_from_u64(128),
        Err(DbErr::TryIntoErr {
            from: "u64",
            into: "i8",
            ..
        })
    ));
    assert_eq!(u64::try_from_u64(u64::MAX).unwrap(), u64::MAX);
    assert_eq!(
        String::try_from_u64(u64::MAX).unwrap(),
        u64::MAX.to_string()
    );
    assert!(matches!(
        bool::try_from_u64(0),
        Err(DbErr::ConvertFromU64("bool"))
    ));
    assert!(matches!(
        <(i64, String)>::try_from_u64(1),
        Err(DbErr::ConvertFromU64(_))
    ));
    assert_eq!(ColIdx::as_str(&"id"), Some("id"));
    assert_eq!(ColIdx::as_usize(&3usize), Some(&3));
    assert_eq!(ColIdx::as_str(&3usize), None);
    assert_eq!(ColIdx::as_usize(&"id"), None);
    type Map = std::collections::BTreeMap<String, i64>;
    assert_eq!(
        Map::from_json_vec(serde_json::json!([{"a": 3}])).unwrap()[0]["a"],
        3
    );
    assert!(
        matches!(Map::from_json_vec(serde_json::json!({})), Err(TryGetError::DbErr(DbErr::Json(message))) if message == "Value is not an Array")
    );
}

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
mod live {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tideorm::internal::driver;

    #[derive(Debug, Copy, Clone)]
    struct Position(usize);
    impl ColIdx for Position {
        #[cfg(feature = "postgres")]
        type PostgresIndex = usize;
        #[cfg(feature = "mysql")]
        type MySqlIndex = usize;
        #[cfg(feature = "sqlite")]
        type SqliteIndex = usize;
        #[cfg(feature = "postgres")]
        fn as_postgres_index(&self) -> usize {
            self.0
        }
        #[cfg(feature = "mysql")]
        fn as_mysql_index(&self) -> usize {
            self.0
        }
        #[cfg(feature = "sqlite")]
        fn as_sqlite_index(&self) -> usize {
            self.0
        }
        fn as_str(&self) -> Option<&str> {
            None
        }
        fn as_usize(&self) -> Option<&usize> {
            Some(&self.0)
        }
    }

    #[derive(Debug, PartialEq)]
    struct Item {
        id: i64,
        label: String,
    }
    impl FromQueryResult for Item {
        fn from_query_result(row: &QueryResult) -> Result<Self, DbErr> {
            Ok(Self {
                id: row.try_get("id")?,
                label: row.try_get("label")?,
            })
        }
    }

    #[derive(Clone, Debug, PartialEq, serde::Deserialize)]
    struct Payload {
        label: String,
    }
    impl TryGetableFromJson for Payload {}

    fn statement(backend: DbBackend) -> Statement {
        let query = match backend {
            DbBackend::Postgres => {
                "SELECT $1 AS p_id, $2 AS p_label, $3 AS p_absent, $4 AS p_payload, $5 AS p_array, $6 AS p_map"
            }
            _ => {
                "SELECT ? AS p_id, ? AS p_label, ? AS p_absent, ? AS p_payload, ? AS p_array, ? AS p_map"
            }
        };
        Statement::from_sql_and_values(
            backend,
            query,
            [
                9_i64.into(),
                "retained label".into(),
                Option::<i64>::None.into(),
                serde_json::json!({"label":"custom"}).into(),
                serde_json::json!([{"label":"one"},{"label":"two"}]).into(),
                serde_json::json!({"value":17}).into(),
            ],
        )
    }

    fn nullable_and_index_errors(row: &QueryResult) {
        assert_eq!(row.try_get::<i64>("p_id").unwrap(), 9);
        assert_eq!(row.try_get_by::<i64, _>(Position(0)).unwrap(), 9);
        assert_eq!(row.try_get_by_index::<String>(1).unwrap(), "retained label");
        assert_eq!(
            row.try_get_by_index_nullable::<Option<i64>>(2).unwrap(),
            None
        );
        assert_eq!(row.try_get::<Option<i64>>("p_missing").unwrap(), None);
        assert!(
            matches!(row.try_get_by_nullable::<i64,_>("p_absent"), Err(TryGetError::Null(name)) if name == "\"p_absent\"")
        );
        assert!(
            matches!(row.try_get_by_index_nullable::<i64>(2), Err(TryGetError::Null(name)) if name == "2")
        );
        assert_eq!(
            row.try_get::<i64>("p_absent").unwrap_err().to_string(),
            "Type Error: A null value was encountered while decoding \"p_absent\""
        );
        assert!(
            matches!(row.try_get_by::<i64,_>("missing"), Err(DbErr::Query(RuntimeErr::DriverError(error))) if matches!(&*error,driver::Error::ColumnNotFound(_)))
        );
        assert!(
            matches!(row.try_get_by_index::<Option<i64>>(99), Err(DbErr::Query(RuntimeErr::DriverError(error))) if matches!(&*error,driver::Error::ColumnIndexOutOfBounds { .. }))
        );
        assert!(row.try_get_by_index::<i64>(1).is_err());
        let tuple: (i64, String) = row
            .try_get_many(&["p_id".into(), "p_label".into(), "ignored_extra".into()])
            .unwrap();
        assert_eq!(tuple, (9, "retained label".into()));
        assert_eq!(row.try_get_many_by_index::<(i64, String)>().unwrap(), tuple);
        assert_eq!(row.try_get_many_by_index::<(i64,)>().unwrap(), (9,));
        assert_eq!(row.try_get_many_by_index::<i64>().unwrap(), 9);
        assert_eq!(
            row.try_get_many::<(i64, String)>(&["p_id".into()])
                .unwrap_err()
                .to_string(),
            "Type Error: Expect 2 column names supplied but got slice of length 1"
        );
        assert_eq!(
            row.try_get::<Payload>("p_payload").unwrap(),
            Payload {
                label: "custom".into()
            }
        );
        assert_eq!(row.try_get::<Vec<Payload>>("p_array").unwrap().len(), 2);
        assert_eq!(
            row.try_get::<std::collections::HashMap<String, i64>>("p_map")
                .unwrap()["value"],
            17
        );
        assert_eq!(
            row.try_get::<std::collections::BTreeMap<String, i64>>("p_map")
                .unwrap()["value"],
            17
        );
    }

    async fn selectors(conn: &DatabaseConnection) {
        let backend = conn.get_database_backend();
        let stmt = Statement::from_string(
            backend,
            "SELECT id, label FROM query_contract_rows ORDER BY id",
        );
        let items = Item::find_by_statement(stmt.clone())
            .all(conn)
            .await
            .unwrap();
        assert_eq!(
            items,
            [
                Item {
                    id: 1,
                    label: "first".into()
                },
                Item {
                    id: 2,
                    label: "second".into()
                }
            ]
        );
        assert_eq!(
            Item::find_by_statement(stmt.clone())
                .one(conn)
                .await
                .unwrap(),
            Some(Item {
                id: 1,
                label: "first".into()
            })
        );
        assert_eq!(
            Item::find_by_statement(stmt.clone())
                .require_one(conn)
                .await
                .unwrap()
                .id,
            1
        );
        assert_eq!(
            Item::find_by_statement(stmt)
                .into_model::<Item>()
                .all(conn)
                .await
                .unwrap(),
            items
        );
        let empty = Statement::from_string(
            backend,
            "SELECT id, label FROM query_contract_rows WHERE id < 0",
        );
        assert_eq!(
            Item::find_by_statement(empty.clone())
                .one(conn)
                .await
                .unwrap(),
            None
        );
        assert!(
            Item::find_by_statement(empty.clone())
                .all(conn)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            matches!(Item::find_by_statement(empty).require_one(conn).await, Err(DbErr::RecordNotFound(message)) if message == "None of the models match the query")
        );
        println!("QUERY_CONTRACT selectors");
    }

    /// One-byte and unsigned arrays bind widened and read back from any
    /// integer array, range-checked.
    #[cfg(feature = "postgres")]
    async fn postgres_integer_arrays(conn: &DatabaseConnection) {
        use sql_builder::{ArrayType, Value};
        let array = |ty, values: Vec<Value>| Value::Array(ty, Some(Box::new(values)));
        let stmt = Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT $1 AS tiny, $2 AS small, $3 AS word, $4 AS big, $5::int4[] AS narrow",
            [
                array(ArrayType::TinyInt, vec![i8::MIN.into(), i8::MAX.into()]),
                array(ArrayType::SmallUnsigned, vec![u16::MAX.into()]),
                array(ArrayType::Unsigned, vec![u32::MAX.into()]),
                array(ArrayType::BigUnsigned, vec![(i64::MAX as u64).into()]),
                array(ArrayType::Int, vec![1_i32.into(), 70_000_i32.into()]),
            ],
        );
        let row = conn.query_one_raw(stmt).await.unwrap().unwrap();
        assert_eq!(row.try_get::<Vec<i8>>("tiny").unwrap(), [i8::MIN, i8::MAX]);
        assert_eq!(row.try_get::<Vec<u16>>("small").unwrap(), [u16::MAX]);
        assert_eq!(row.try_get::<Vec<u32>>("word").unwrap(), [u32::MAX]);
        assert_eq!(row.try_get::<Vec<u64>>("big").unwrap(), [i64::MAX as u64]);
        assert_eq!(row.try_get::<Vec<u64>>("narrow").unwrap(), [1, 70_000]);
        assert_eq!(row.try_get::<Vec<i64>>("narrow").unwrap(), [1, 70_000]);
        assert!(row.try_get::<Vec<u16>>("narrow").is_err());
        assert!(row.try_get::<Vec<i8>>("small").is_err());
        println!("QUERY_CONTRACT postgres_integer_arrays");
    }

    async fn queries(url: &str) {
        let mut options = ConnectOptions::new(url);
        options.max_connections(1);
        let mut conn = Database::connect(options).await.unwrap();
        let backend = conn.get_database_backend();
        let stmt = statement(backend);
        let row = conn.query_one_raw(stmt.clone()).await.unwrap().unwrap();
        assert_eq!(
            row.column_names(),
            [
                "p_id",
                "p_label",
                "p_absent",
                "p_payload",
                "p_array",
                "p_map"
            ]
        );
        nullable_and_index_errors(&row);
        #[cfg(feature = "postgres")]
        {
            assert_eq!(
                row.try_as_pg_row().is_some(),
                backend == DbBackend::Postgres
            );
            if backend == DbBackend::Postgres {
                assert_eq!(row.try_get_from_postgres::<i64, _>(0).unwrap().unwrap(), 9);
                assert!(row.try_get_from_postgres::<bool, _>(0).is_none());
                assert!(matches!(
                    row.try_get_from_postgres::<i64, _>("p_absent"),
                    Some(Err(TryGetError::Null(_)))
                ));
            }
        }
        #[cfg(feature = "mysql")]
        assert_eq!(
            row.try_as_mysql_row().is_some(),
            matches!(backend, DbBackend::MySQL | DbBackend::MariaDB)
        );
        #[cfg(feature = "sqlite")]
        assert_eq!(
            row.try_as_sqlite_row().is_some(),
            backend == DbBackend::SQLite
        );
        println!("QUERY_CONTRACT nullable_custom_decode");
        let metrics = Arc::new(Mutex::new(Vec::new()));
        let saved = metrics.clone();
        conn.set_metric_callback(move |info| {
            let failure = info.error.map(|error| matches!(error, DbErr::Query(_)));
            saved
                .lock()
                .unwrap()
                .push((failure, info.rows, info.statement.log));
        });
        let rows = conn.query_all_raw(stmt).await.unwrap();
        assert_eq!(rows.len(), 1);
        let empty = Statement::from_string(backend, "SELECT 1 WHERE 1 = 0");
        assert!(conn.query_one_raw(empty.clone()).await.unwrap().is_none());
        assert!(conn.query_all_raw(empty).await.unwrap().is_empty());
        assert!(matches!(
            conn.query_one_raw(Statement::from_string(
                backend,
                "SELECT * FROM missing_query_contract_table"
            ))
            .await,
            Err(DbErr::Query(_))
        ));
        let unlogged = Statement::from_string(backend, "SELECT 1").unlogged();
        assert!(conn.query_one_raw(unlogged).await.unwrap().is_some());
        // An unlogged statement is still timed, and says it is not to be logged.
        assert_eq!(
            *metrics.lock().unwrap(),
            [
                (None, Some(1), true),
                (None, Some(0), true),
                (None, Some(0), true),
                (Some(true), None, true),
                (None, Some(1), false),
            ]
        );
        println!("QUERY_CONTRACT empty_error_metrics");
        conn.execute_unprepared(
            "CREATE TEMPORARY TABLE query_contract_rows (id BIGINT PRIMARY KEY, label TEXT)",
        )
        .await
        .unwrap();
        let insert = sql_builder::Query::insert()
            .into_table("query_contract_rows")
            .columns(["id", "label"])
            .values_panic([1_i64.into(), "first".into()])
            .values_panic([2_i64.into(), "second".into()])
            .to_owned();
        assert_eq!(conn.execute(&insert).await.unwrap().rows_affected(), 2);
        let query = sql_builder::Query::select()
            .columns(["id", "label"])
            .from("query_contract_rows")
            .order_by("id", sql_builder::Order::Asc)
            .to_owned();
        assert_eq!(conn.query_all(&query).await.unwrap().len(), 2);
        assert_eq!(
            conn.query_one(&query)
                .await
                .unwrap()
                .unwrap()
                .try_get_by_index::<i64>(0)
                .unwrap(),
            1
        );
        selectors(&conn).await;
        #[cfg(feature = "postgres")]
        if backend == DbBackend::Postgres {
            postgres_integer_arrays(&conn).await;
        }
        conn.close_by_ref().await.unwrap();
        assert!(matches!(
            conn.query_all_raw(Statement::from_string(backend, "SELECT 1"))
                .await,
            Err(DbErr::ConnectionAcquire(ConnAcquireErr::ConnectionClosed))
        ));
        // Decoded rows own their data and outlive pool shutdown.
        nullable_and_index_errors(&row);
        println!("QUERY_CONTRACT builder_owned_rows");
    }

    async fn run() {
        let work = async {
            if let Ok(url) = std::env::var("ENGINE_PROBE_DATABASE_URL") {
                queries(&url).await;
            } else {
                #[cfg(feature = "sqlite")]
                queries("sqlite::memory:").await;
                #[cfg(not(feature = "sqlite"))]
                panic!("ENGINE_PROBE_DATABASE_URL is required for live query contracts");
            }
        };
        use futures_util::future::{Either, select};
        match select(
            Box::pin(work),
            Box::pin(async_io::Timer::after(std::time::Duration::from_secs(30))),
        )
        .await
        {
            Either::Left(_) => (),
            Either::Right(_) => panic!("query contracts timed out"),
        }
    }
    #[cfg(feature = "runtime-tokio")]
    #[test]
    #[cfg_attr(not(feature = "sqlite"), ignore = "requires a disposable server URL")]
    fn query_contracts_tokio() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run());
    }
    #[cfg(feature = "runtime-async-std")]
    #[test]
    #[cfg_attr(not(feature = "sqlite"), ignore = "requires a disposable server URL")]
    fn query_contracts_async_std() {
        async_std::task::block_on(run());
    }
}
