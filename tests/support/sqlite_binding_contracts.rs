use chrono::{Local, TimeZone, Utc};
use sql_builder::{Enum, OptionEnum, Value, Values};
use std::future::Future;

pub async fn sqlite_binding_contracts<F, Fut>(mut fetch: F)
where
    F: FnMut(String, Option<Values>) -> Fut,
    Fut: Future<Output = Option<String>>,
{
    let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
    let time = chrono::NaiveTime::from_hms_opt(12, 34, 56).unwrap();
    let naive = date.and_time(time);
    let utc = Utc.from_utc_datetime(&naive);
    let local = utc.with_timezone(&Local);
    let fixed = utc.with_timezone(&chrono::FixedOffset::east_opt(7200).unwrap());
    let uuid = uuid::Uuid::parse_str("01234567-89ab-cdef-0123-456789abcdef").unwrap();
    let cases: Vec<(&str, Value, String, bool)> = vec![
        ("bool", true.into(), "1".into(), false),
        ("i8", i8::MIN.into(), "-128".into(), false),
        ("i16", i16::MIN.into(), "-32768".into(), false),
        ("i32", i32::MIN.into(), "-2147483648".into(), false),
        ("i64", i64::MIN.into(), i64::MIN.to_string(), false),
        ("u8", u8::MAX.into(), "255".into(), false),
        ("u16", u16::MAX.into(), "65535".into(), false),
        ("u32", u32::MAX.into(), "4294967295".into(), false),
        ("u64", (i64::MAX as u64).into(), i64::MAX.to_string(), false),
        ("f32", 1.5_f32.into(), "1.5".into(), false),
        ("f64", (-2.75_f64).into(), "-2.75".into(), false),
        (
            "string",
            "λ'); SELECT 99; --\0end".into(),
            "λ'); SELECT 99; --\0end".into(),
            false,
        ),
        (
            "enum",
            Value::Enum(OptionEnum::Some(Box::new(Enum {
                type_name: "binding_kind".into(),
                value: "quoted'value".into(),
            }))),
            "quoted'value".into(),
            false,
        ),
        ("char", 'λ'.into(), "λ".into(), false),
        (
            "bytes",
            vec![0_u8, 1, 127, 128, 255].into(),
            "00017F80FF".into(),
            true,
        ),
        (
            "json",
            serde_json::json!({"a": [1, null, "λ"]}).into(),
            "{\"a\":[1,null,\"λ\"]}".into(),
            false,
        ),
        ("date", date.into(), "2026-10-25".into(), false),
        ("time", time.into(), "12:34:56".into(), false),
        (
            "datetime",
            naive.into(),
            "2026-10-25 12:34:56".into(),
            false,
        ),
        ("utc", utc.into(), utc.to_rfc3339(), false),
        ("local", local.into(), local.to_rfc3339(), false),
        ("offset", fixed.into(), fixed.to_rfc3339(), false),
        (
            "decimal",
            rust_decimal::Decimal::new(1234500, 4).into(),
            "123.4500".into(),
            false,
        ),
        (
            "uuid",
            uuid.into(),
            "0123456789ABCDEF0123456789ABCDEF".into(),
            true,
        ),
    ];
    for (name, value, expected, hexadecimal) in cases {
        let null = value.as_null();
        let sql = if hexadecimal {
            "SELECT hex(?) AS value"
        } else {
            "SELECT CAST(? AS TEXT) AS value"
        };
        assert_eq!(
            fetch(sql.into(), Some(Values(vec![value]))).await,
            Some(expected),
            "{name}"
        );
        assert_eq!(
            fetch(
                "SELECT CAST(? AS TEXT) AS value".into(),
                Some(Values(vec![null]))
            )
            .await,
            None,
            "{name} NULL"
        );
        println!("BINDING_SCALAR {name} value_and_null");
    }
    for parameters in [None, Some(Values(vec![]))] {
        assert_eq!(
            fetch("SELECT CAST(42 AS TEXT) AS value".into(), parameters).await,
            Some("42".into())
        );
    }
    assert_eq!(
        fetch(
            "SELECT CAST((? * 100) + (? * 10) + ? AS TEXT) AS value".into(),
            Some(Values(vec![1.into(), 2.into(), 3.into()]))
        )
        .await,
        Some("123".into())
    );
    assert_eq!(
        fetch(
            "SELECT CAST(? AS TEXT) AS value".into(),
            Some(Values(vec!["".into()]))
        )
        .await,
        Some("".into())
    );
    assert_eq!(
        fetch(
            "SELECT hex(?) AS value".into(),
            Some(Values(vec![Vec::<u8>::new().into()]))
        )
        .await,
        Some("".into())
    );
    println!("BINDING_ORDER absent_empty_order_and_empty_values");
}
