use sql_builder::{IntoIden, StringLen};
use std::borrow::Cow;
use tideorm::orm::*;

#[derive(Clone, Copy, Debug, Default)]
struct Entity;
impl Iden for Entity {
    fn quoted(&self) -> Cow<'static, str> {
        Cow::Borrowed("column_contract_rows")
    }
    fn unquoted(&self) -> &str {
        "column_contract_rows"
    }
}
impl IdenStatic for Entity {
    fn as_str(&self) -> &'static str {
        "column_contract_rows"
    }
}
impl EntityName for Entity {
    fn table_name(&self) -> &'static str {
        "column_contract_rows"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Col {
    Id,
    Name,
    Score,
    Mood,
    Attrs,
    Nums,
    Moods,
    Unicode,
}
impl IdenStatic for Col {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::Score => "score",
            Self::Mood => "mood",
            Self::Attrs => "attrs",
            Self::Nums => "nums",
            Self::Moods => "moods",
            Self::Unicode => "na\"mé",
        }
    }
}
impl Iden for Col {
    fn quoted(&self) -> Cow<'static, str> {
        Cow::Borrowed(IdenStatic::as_str(self))
    }
    fn unquoted(&self) -> &str {
        IdenStatic::as_str(self)
    }
}
impl Iterable for Col {
    const ALL: &'static [Self] = &[
        Self::Id,
        Self::Name,
        Self::Score,
        Self::Mood,
        Self::Attrs,
        Self::Nums,
        Self::Moods,
        Self::Unicode,
    ];
}
fn mood_type() -> ColumnType {
    ColumnType::Enum {
        name: "column_mood".into_iden(),
        variants: vec!["happy".into_iden(), "sad".into_iden()],
    }
}
impl ColumnTrait for Col {
    type EntityName = Entity;
    fn def(&self) -> ColumnDef {
        match self {
            Self::Id => ColumnType::BigInteger.def(),
            Self::Name | Self::Unicode => ColumnType::String(StringLen::N(40)).def(),
            Self::Score => ColumnType::BigInteger.def().nullable(),
            Self::Mood => mood_type().def(),
            Self::Attrs => ColumnType::JsonBinary.def(),
            Self::Nums => ColumnType::Array(ColumnType::BigInteger.into()).def(),
            Self::Moods => ColumnType::Array(mood_type().into()).def(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Key {
    Id,
    Name,
}
impl Iden for Key {
    fn quoted(&self) -> Cow<'static, str> {
        Cow::Borrowed(IdenStatic::as_str(self))
    }
    fn unquoted(&self) -> &str {
        IdenStatic::as_str(self)
    }
}
impl IdenStatic for Key {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
        }
    }
}
impl Iterable for Key {
    const ALL: &'static [Self] = &[Self::Id, Self::Name];
}
impl PrimaryKeyTrait for Key {
    type ValueType = (i64, String);
    fn auto_increment() -> bool {
        false
    }
}
impl PrimaryKeyToColumn for Key {
    type Column = Col;
    fn into_column(self) -> Self::Column {
        match self {
            Self::Id => Col::Id,
            Self::Name => Col::Name,
        }
    }
    fn from_column(col: Self::Column) -> Option<Self> {
        match col {
            Col::Id => Some(Self::Id),
            Col::Name => Some(Self::Name),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mood {
    Happy,
    Sad,
}
impl Iterable for Mood {
    const ALL: &'static [Self] = &[Self::Happy, Self::Sad];
}
impl ActiveEnum for Mood {
    type Value = String;
    type ValueVec = Vec<String>;
    fn name() -> sql_builder::DynIden {
        "column_mood".into_iden()
    }
    fn to_value(&self) -> Self::Value {
        match self {
            Self::Happy => "happy",
            Self::Sad => "sad",
        }
        .into()
    }
    fn try_from_value(value: &Self::Value) -> Result<Self, DbErr> {
        match value.as_str() {
            "happy" => Ok(Self::Happy),
            "sad" => Ok(Self::Sad),
            _ => Err(DbErr::Custom("unknown mood".into())),
        }
    }
    fn db_type() -> ColumnDef {
        mood_type().def()
    }
}
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
#[derive(Clone, Copy, Debug)]
struct NativeMood(Mood);
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
impl Iterable for NativeMood {
    const ALL: &'static [Self] = &[Self(Mood::Happy), Self(Mood::Sad)];
}
#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
impl ActiveEnum for NativeMood {
    type Value = sql_builder::Enum;
    type ValueVec = Vec<sql_builder::Enum>;
    fn name() -> sql_builder::DynIden {
        Mood::name()
    }
    fn to_value(&self) -> Self::Value {
        sql_builder::Enum {
            type_name: "column_mood".into(),
            value: self.0.to_value().into(),
        }
    }
    fn try_from_value(value: &Self::Value) -> Result<Self, DbErr> {
        Mood::try_from_value(&value.value.to_string()).map(Self)
    }
    fn db_type() -> ColumnDef {
        mood_type().def()
    }
}

#[test]
fn column_definition_metadata() {
    let def = ColumnType::String(StringLen::N(80)).def().null();
    assert_eq!(def.get_column_type(), &ColumnType::String(StringLen::N(80)));
    assert!(def.is_null());
    assert_eq!(def.clone().def(), def);
    assert_eq!(
        ColumnType::Integer.def().null(),
        ColumnType::Integer.def().nullable()
    );
    assert!(!ColumnType::Integer.def().is_null());
    for ty in [
        mood_type(),
        ColumnType::Array(mood_type().into()),
        ColumnType::Array(ColumnType::Array(mood_type().into()).into()),
    ] {
        assert_eq!(ty.get_enum_name().unwrap().to_string(), "column_mood");
        assert_eq!(ty.def().get_enum_name().unwrap().to_string(), "column_mood");
    }
    assert!(ColumnType::Integer.get_enum_name().is_none());
    assert_eq!(Col::Id.entity_name().to_string(), "column_contract_rows");
    assert_eq!(Col::Id.as_column_ref().1.to_string(), "id");
}

#[test]
fn primary_key_and_enum_contracts() {
    assert!(!Key::auto_increment());
    assert_eq!(<Key as PrimaryKeyTrait>::ValueType::ARITY, 2);
    assert_eq!(Key::Id.into_column(), Col::Id);
    assert_eq!(Key::from_column(Col::Name), Some(Key::Name));
    assert_eq!(Key::from_column(Col::Score), None);
    assert_eq!(i64::ARITY, 1);
    macro_rules! arity {
        ($n:expr, $ty:ty) => {
            assert_eq!(<$ty>::ARITY, $n);
        };
    }
    arity!(1, (i64,));
    arity!(2, (i64, i64));
    arity!(3, (i64, i64, i64));
    arity!(4, (i64, i64, i64, i64));
    arity!(5, (i64, i64, i64, i64, i64));
    arity!(6, (i64, i64, i64, i64, i64, i64));
    arity!(7, (i64, i64, i64, i64, i64, i64, i64));
    arity!(8, (i64, i64, i64, i64, i64, i64, i64, i64));
    arity!(9, (i64, i64, i64, i64, i64, i64, i64, i64, i64));
    arity!(10, (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64));
    arity!(11, (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64));
    arity!(
        12,
        (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64)
    );
    assert_eq!(Mood::values(), ["happy", "sad"]);
    assert_eq!(Mood::Happy.into_value(), "happy");
    assert_eq!(Mood::try_from_value(&"sad".into()).unwrap(), Mood::Sad);
    assert!(Mood::try_from_value(&"invalid".into()).is_err());
    assert_eq!(Mood::db_type(), mood_type().def());
    assert!(
        matches!(Mood::try_from_u64(1), Err(DbErr::ConvertFromU64(message)) if message.contains("ActiveEnum"))
    );
    fn value_contract<T: ActiveEnumValue>() {}
    value_contract::<u8>();
    value_contract::<u16>();
    value_contract::<u32>();
    value_contract::<u64>();
    value_contract::<i8>();
    value_contract::<i16>();
    value_contract::<i32>();
    value_contract::<i64>();
    value_contract::<String>();
    value_contract::<sql_builder::Enum>();
}

#[cfg(feature = "postgres")]
fn json_array_value() -> Value {
    Value::Array(
        sql_builder::ArrayType::Json,
        Some(Box::new(vec![
            serde_json::json!({"x":1}).into(),
            Value::Json(None),
            42i64.into(),
            serde_json::Value::Null.into(),
        ])),
    )
}

#[cfg(any(feature = "postgres", feature = "mysql", feature = "sqlite"))]
mod live {
    use super::*;
    use sql_builder::{Expr, Query};
    use std::time::Duration;
    async fn ids(db: &DatabaseConnection, condition: Expr) -> Vec<i64> {
        let query = Query::select()
            .column(Col::Id)
            .from(Entity.table_ref())
            .and_where(condition)
            .order_by(Col::Id, sql_builder::Order::Asc)
            .to_owned();
        db.query_all(&query)
            .await
            .unwrap()
            .iter()
            .map(|r| r.try_get_by_index(0).unwrap())
            .collect()
    }
    async fn contracts(url: &str) {
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(3));
        let db = Database::connect(options).await.unwrap();
        let backend = db.get_database_backend();
        db.execute_unprepared("DROP TABLE IF EXISTS column_contract_rows")
            .await
            .unwrap();
        if backend == DbBackend::Postgres {
            db.execute_unprepared("DROP TYPE IF EXISTS column_mood")
                .await
                .unwrap();
            db.execute_unprepared("CREATE TYPE column_mood AS ENUM ('happy','sad')")
                .await
                .unwrap();
        }
        let ddl = match backend {
            DbBackend::Postgres => {
                "CREATE TABLE column_contract_rows (id BIGINT PRIMARY KEY, name VARCHAR(40), score BIGINT, mood column_mood, attrs JSONB, nums BIGINT[], moods column_mood[])"
            }
            DbBackend::MySQL | DbBackend::MariaDB => {
                "CREATE TABLE column_contract_rows (id BIGINT PRIMARY KEY, name VARCHAR(40), score BIGINT, mood ENUM('happy','sad'), attrs JSON)"
            }
            DbBackend::SQLite => {
                "CREATE TABLE column_contract_rows (id BIGINT PRIMARY KEY, name TEXT, score BIGINT, mood TEXT, attrs TEXT)"
            }
        };
        db.execute_unprepared(ddl).await.unwrap();
        db.execute_unprepared("INSERT INTO column_contract_rows (id,name,score,mood) VALUES (1,'ALPHA',NULL,'happy'),(2,'BETA',20,'sad'),(3,'GAMMA',30,'happy')").await.unwrap();
        assert_eq!(
            ids(&db, ColumnTrait::eq(&Col::Score, Option::<i64>::None)).await,
            [1]
        );
        assert_eq!(
            ids(&db, ColumnTrait::ne(&Col::Score, Option::<i64>::None)).await,
            [2, 3]
        );
        assert_eq!(ids(&db, Col::Id.between(2i64, 3i64)).await, [2, 3]);
        assert_eq!(ids(&db, Col::Id.not_between(2i64, 3i64)).await, [1]);
        assert_eq!(ids(&db, Col::Id.is_in([1i64, 3])).await, [1, 3]);
        assert!(ids(&db, Col::Id.is_in([] as [i64; 0])).await.is_empty());
        assert_eq!(ids(&db, Col::Id.is_not_in([] as [i64; 0])).await, [1, 2, 3]);
        assert_eq!(ids(&db, Col::Name.starts_with("AL")).await, [1]);
        assert_eq!(ids(&db, Col::Name.ends_with("TA")).await, [2]);
        assert_eq!(ids(&db, Col::Name.contains("AM")).await, [3]);
        assert_eq!(
            ids(&db, ColumnTrait::eq(&Col::Mood, Mood::Happy.to_value())).await,
            [1, 3]
        );
        assert_eq!(
            ids(
                &db,
                ColumnTrait::eq(&Col::Mood, NativeMood(Mood::Sad).to_value())
            )
            .await,
            [2]
        );
        let query = Query::select()
            .expr_as(Col::Mood.into_column_as_expr(), "mood")
            .from(Entity.table_ref())
            .and_where(ColumnTrait::eq(&Col::Id, 1i64))
            .to_owned();
        let row = db.query_one(&query).await.unwrap().unwrap();
        let value: sql_builder::Enum = row.try_get("mood").unwrap();
        assert!(value.type_name.is_empty());
        assert_eq!(value.value.as_ref(), "happy");
        assert!(matches!(
            <u64 as ActiveEnumValue>::try_get_vec_by(&row, "mood"),
            Err(TryGetError::DbErr(DbErr::BackendNotSupported {
                db: "Postgres",
                ctx: "ActiveEnumValue::try_get_vec_by"
            }))
        ));
        if backend == DbBackend::Postgres {
            #[cfg(feature = "postgres")]
            {
                db.execute_unprepared("UPDATE column_contract_rows SET nums = ARRAY[1,2]::BIGINT[], moods = ARRAY['happy','sad']::column_mood[], attrs = '[{\"x\":1},null]'::JSONB WHERE id = 1").await.unwrap();
                assert_eq!(ids(&db, Col::Id.eq_any([1i64, 3])).await, [1, 3]);
                assert_eq!(ids(&db, Col::Id.ne_all([1i64, 3])).await, [2]);
                assert!(ids(&db, Col::Id.eq_any([] as [i64; 0])).await.is_empty());
                assert_eq!(ids(&db, Col::Id.ne_all([] as [i64; 0])).await, [1, 2, 3]);
                assert_eq!(ids(&db, Col::Nums.array_contains([1i64])).await, [1]);
                assert_eq!(ids(&db, Col::Nums.array_contained([1i64, 2, 3])).await, [1]);
                assert_eq!(ids(&db, Col::Nums.array_overlap([2i64])).await, [1]);
                assert_eq!(
                    ids(&db, Col::Mood.eq_any(["happy".to_owned()])).await,
                    [1, 3]
                );
                assert_eq!(
                    ids(&db, ColumnTrait::eq(&Col::Attrs, json_array_value())).await,
                    [1]
                );
                let query = Query::select()
                    .column(Col::Nums)
                    .expr_as(Col::Moods.into_column_as_expr(), "moods")
                    .from(Entity.table_ref())
                    .and_where(ColumnTrait::eq(&Col::Id, 1i64))
                    .to_owned();
                let row = db.query_one(&query).await.unwrap().unwrap();
                assert_eq!(
                    <i64 as ActiveEnumValue>::try_get_vec_by(&row, "nums").unwrap(),
                    [1, 2]
                );
                let values =
                    <sql_builder::Enum as ActiveEnumValue>::try_get_vec_by(&row, "moods").unwrap();
                assert_eq!(
                    values.iter().map(|v| v.value.as_ref()).collect::<Vec<_>>(),
                    ["happy", "sad"]
                );
                assert!(values.iter().all(|v| v.type_name.is_empty()));
            }
        } else {
            assert!(<String as ActiveEnumValue>::try_get_vec_by(&row, "mood").is_err());
        }
        db.execute_unprepared("DROP TABLE column_contract_rows")
            .await
            .unwrap();
        if backend == DbBackend::Postgres {
            db.execute_unprepared("DROP TYPE column_mood")
                .await
                .unwrap();
        }
        db.close_by_ref().await.unwrap();
    }
    async fn run() {
        let work = async {
            if let Ok(url) = std::env::var("ENGINE_PROBE_DATABASE_URL") {
                contracts(&url).await;
            } else {
                #[cfg(feature = "sqlite")]
                contracts("sqlite::memory:").await;
                #[cfg(not(feature = "sqlite"))]
                panic!("ENGINE_PROBE_DATABASE_URL is required for live column contracts");
            }
        };
        match futures_util::future::select(
            Box::pin(work),
            Box::pin(async_io::Timer::after(Duration::from_secs(30))),
        )
        .await
        {
            futures_util::future::Either::Left(_) => (),
            futures_util::future::Either::Right(_) => panic!("column contracts timed out"),
        }
    }
    #[test]
    #[cfg(feature = "runtime-tokio")]
    #[cfg_attr(not(feature = "sqlite"), ignore = "requires a disposable server URL")]
    fn columns_tokio() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run());
    }
    #[test]
    #[cfg(feature = "runtime-async-std")]
    #[cfg_attr(not(feature = "sqlite"), ignore = "requires a disposable server URL")]
    fn columns_async_std() {
        async_std::task::block_on(run());
    }
}
