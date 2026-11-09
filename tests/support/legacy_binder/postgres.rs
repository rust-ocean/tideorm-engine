use sql_builder::prelude::{
    DateTime, Decimal, FixedOffset, Json, Local, NaiveDate, NaiveDateTime, NaiveTime, Utc, Uuid,
};
use sql_builder::{ArrayType, OptionEnum, Value};

use super::LegacyValues;

impl driver::IntoArguments<driver::postgres::Postgres> for LegacyValues {
    fn into_arguments(self) -> driver::postgres::PgArguments {
        let mut args = driver::postgres::PgArguments::default();
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
                    let _ = args.add(i.map(|i| i as i16));
                }
                Value::SmallUnsigned(i) => {
                    let _ = args.add(i.map(|i| i as i32));
                }
                Value::Unsigned(i) => {
                    let _ = args.add(i.map(|i| i as i64));
                }
                Value::BigUnsigned(i) => {
                    let _ = args.add(i.map(|i| <i64 as TryFrom<u64>>::try_from(i).unwrap()));
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
                    let _ = args.add(t);
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
                Value::Array(ty, v) => match ty {
                    ArrayType::Bool => {
                        let value: Option<Vec<bool>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Bool");
                        let _ = args.add(value);
                    }
                    ArrayType::TinyInt => {
                        let value: Option<Vec<i8>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::TinyInt");
                        let _ = args.add(value);
                    }
                    ArrayType::SmallInt => {
                        let value: Option<Vec<i16>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::SmallInt");
                        let _ = args.add(value);
                    }
                    ArrayType::Int => {
                        let value: Option<Vec<i32>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Int");
                        let _ = args.add(value);
                    }
                    ArrayType::BigInt => {
                        let value: Option<Vec<i64>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::BigInt");
                        let _ = args.add(value);
                    }
                    ArrayType::TinyUnsigned => {
                        let value: Option<Vec<u8>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::TinyUnsigned");
                        let value: Option<Vec<i16>> =
                            value.map(|vec| vec.into_iter().map(|i| i as i16).collect());
                        let _ = args.add(value);
                    }
                    ArrayType::SmallUnsigned => {
                        let value: Option<Vec<u16>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::SmallUnsigned");
                        let value: Option<Vec<i32>> =
                            value.map(|vec| vec.into_iter().map(|i| i as i32).collect());
                        let _ = args.add(value);
                    }
                    ArrayType::Unsigned => {
                        let value: Option<Vec<u32>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Unsigned");
                        let value: Option<Vec<i64>> =
                            value.map(|vec| vec.into_iter().map(|i| i as i64).collect());
                        let _ = args.add(value);
                    }
                    ArrayType::BigUnsigned => {
                        let value: Option<Vec<u64>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::BigUnsigned");
                        let value: Option<Vec<i64>> = value.map(|vec| {
                            vec.into_iter()
                                .map(|i| <i64 as TryFrom<u64>>::try_from(i).unwrap())
                                .collect()
                        });
                        let _ = args.add(value);
                    }
                    ArrayType::Float => {
                        let value: Option<Vec<f32>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Float");
                        let _ = args.add(value);
                    }
                    ArrayType::Double => {
                        let value: Option<Vec<f64>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Double");
                        let _ = args.add(value);
                    }
                    ArrayType::String => {
                        let value: Option<Vec<String>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::String");
                        let _ = args.add(value);
                    }
                    ArrayType::Enum(_) => {
                        let value: Option<Vec<String>> = v.map(|values| {
                            values
                                .into_iter()
                                .map(|value| match value {
                                    Value::Enum(OptionEnum::Some(value))  => value.value.into_owned(),
                                    _ => {
                                        panic!(
                                            "Value::Array(ArrayType::Enum) should contain Value::Enum"
                                        );
                                    }
                                })
                                .collect()
                        });
                        let _ = args.add(value);
                    }
                    ArrayType::Char => {
                        let value: Option<Vec<char>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Char");
                        let value: Option<Vec<String>> =
                            value.map(|vec| vec.into_iter().map(|c| c.to_string()).collect());
                        let _ = args.add(value);
                    }
                    ArrayType::Bytes => {
                        let value: Option<Vec<Vec<u8>>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Bytes");
                        let _ = args.add(value);
                    }
                    ArrayType::ChronoDate => {
                        let value: Option<Vec<NaiveDate>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::ChronoDate");
                        let _ = args.add(value);
                    }
                    ArrayType::ChronoTime => {
                        let value: Option<Vec<NaiveTime>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::ChronoTime");
                        let _ = args.add(value);
                    }
                    ArrayType::ChronoDateTime => {
                        let value: Option<Vec<NaiveDateTime>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::ChronoDateTime");
                        let _ = args.add(value);
                    }
                    ArrayType::ChronoDateTimeUtc => {
                        let value: Option<Vec<DateTime<Utc>>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::ChronoDateTimeUtc");
                        let _ = args.add(value);
                    }
                    ArrayType::ChronoDateTimeLocal => {
                        let value: Option<Vec<DateTime<Local>>> = Value::Array(ty, v).expect(
                            "This Value::Array should consist of Value::ChronoDateTimeLocal",
                        );
                        let _ = args.add(value);
                    }
                    ArrayType::ChronoDateTimeWithTimeZone => {
                        let value: Option<Vec<DateTime<FixedOffset>>> = Value::Array(ty, v).expect(
                            "This Value::Array should consist of Value::ChronoDateTimeWithTimeZone",
                        );
                        let _ = args.add(value);
                    }
                    ArrayType::Uuid => {
                        let value: Option<Vec<Uuid>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Uuid");
                        let _ = args.add(value);
                    }
                    ArrayType::Decimal => {
                        let value: Option<Vec<Decimal>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Decimal");
                        let _ = args.add(value);
                    }
                    ArrayType::Json => {
                        let value: Option<Vec<Json>> = Value::Array(ty, v)
                            .expect("This Value::Array should consist of Value::Json");
                        let _ = args.add(value);
                    }
                },
            }
        }
        args
    }
}
