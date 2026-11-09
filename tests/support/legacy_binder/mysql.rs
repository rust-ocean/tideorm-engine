use super::LegacyValues;
use sql_builder::{OptionEnum, Value};

impl driver::IntoArguments<driver::mysql::MySql> for LegacyValues {
    fn into_arguments(self) -> driver::mysql::MySqlArguments {
        let mut args = driver::mysql::MySqlArguments::default();
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
                    let _ = args.add(i);
                }
                Value::Float(f) => {
                    let _ = args.add(f);
                }
                Value::Double(d) => {
                    let _ = args.add(d);
                }
                Value::String(s) => {
                    let _ = args.add(s.as_deref());
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
                    let _ = args.add(b.as_deref());
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
                    let _ = args
                        .add(Value::ChronoDateTimeWithTimeZone(t).chrono_as_naive_utc_in_string());
                }
                Value::Uuid(uuid) => {
                    let _ = args.add(uuid);
                }
                Value::Decimal(d) => {
                    let _ = args.add(d);
                }
                Value::Json(j) => {
                    let _ = args.add(j.as_deref());
                }
                Value::Array(_, _) => {
                    panic!("Mysql doesn't support array arguments");
                }
            }
        }
        args
    }
}
