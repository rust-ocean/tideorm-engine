use super::LegacyValues;
use sql_builder::{OptionEnum, Value};

impl driver::IntoArguments<driver::sqlite::Sqlite> for LegacyValues {
    fn into_arguments(self) -> driver::sqlite::SqliteArguments {
        let mut args = driver::sqlite::SqliteArguments::default();
        for arg in self.0.into_iter() {
            use driver::Arguments;
            match arg {
                Value::Bool(b) => {
                    let _ = args.add(b);
                }
                Value::TinyInt(i) => {
                    let _ = args.add(i);
                }
                Value::SmallInt(i) => {
                    let _ = args.add(i);
                }
                Value::Int(i) => {
                    let _ = args.add(i);
                }
                Value::BigInt(i) => {
                    let _ = args.add(i);
                }
                Value::TinyUnsigned(i) => {
                    let _ = args.add(i);
                }
                Value::SmallUnsigned(i) => {
                    let _ = args.add(i);
                }
                Value::Unsigned(i) => {
                    let _ = args.add(i);
                }
                Value::BigUnsigned(i) => {
                    let _ = args
                        .add(i.map(|i| <i64 as std::convert::TryFrom<u64>>::try_from(i).unwrap()));
                }
                Value::Float(f) => {
                    let _ = args.add(f);
                }
                Value::Double(d) => {
                    let _ = args.add(d);
                }
                Value::String(s) => {
                    let _ = args.add(s);
                }
                Value::Enum(e) => {
                    let value = match e {
                        OptionEnum::Some(v) => Some(v.value.into_owned()),
                        OptionEnum::None(_) => None,
                    };
                    let _ = args.add(value);
                }
                Value::Char(c) => {
                    let _ = args.add(c.map(|c| c.to_string()));
                }
                Value::Bytes(b) => {
                    let _ = args.add(b);
                }
                Value::ChronoDate(d) => {
                    let _ = args.add(d);
                }
                Value::ChronoTime(t) => {
                    let _ = args.add(t);
                }
                Value::ChronoDateTime(t) => {
                    let _ = args.add(t);
                }
                Value::ChronoDateTimeUtc(t) => {
                    let _ = args.add(t);
                }
                Value::ChronoDateTimeLocal(t) => {
                    let _ = args.add(t);
                }
                Value::ChronoDateTimeWithTimeZone(t) => {
                    let _ = args.add(t);
                }
                Value::Uuid(uuid) => {
                    let _ = args.add(uuid);
                }
                Value::Decimal(decimal) => {
                    let _ = args.add(decimal.map(|d| d.to_string()));
                }
                Value::Json(j) => {
                    let _ = args.add(j.map(|j| *j));
                }
                Value::Array(_, _) => {
                    panic!("Sqlite doesn't support array arguments");
                }
            }
        }
        args
    }
}
