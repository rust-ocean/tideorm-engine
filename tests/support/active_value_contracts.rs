use std::cell::Cell;
use tideorm::orm::{ActiveValue, IntoActiveValue, NotSet, Set, Unchanged, Value};

#[test]
fn omitted_null_and_unchanged_remain_distinct() {
    let omitted = ActiveValue::<Option<i32>>::default();
    let null = Set(None::<i32>);
    let existing_null = Unchanged(None::<i32>);
    assert!(omitted.is_not_set());
    assert_ne!(omitted, null);
    assert_ne!(null, existing_null);
    assert_eq!(omitted.into_value(), None);
    assert_eq!(null.into_value(), Some(Value::Int(None)));
    assert_eq!(existing_null.into_value(), Some(Value::Int(None)));
    assert_eq!(Set(7).into_wrapped_value(), Set(Value::Int(Some(7))));
    assert_eq!(
        Unchanged(7).into_wrapped_value(),
        Unchanged(Value::Int(Some(7)))
    );
    assert_eq!(ActiveValue::<i32>::not_set().into_wrapped_value(), NotSet);
    assert_eq!(format!("{:?}", Set(7).clone()), "Set(7)");
    assert_eq!(format!("{:?}", Unchanged(7)), "Unchanged(7)");
    assert_eq!(format!("{:?}", ActiveValue::<i32>::NotSet), "NotSet");
    assert_ne!(Set(f64::NAN), Set(f64::NAN));
}

#[test]
fn take_reset_and_borrows_preserve_state_and_ownership() {
    for (mut value, expected) in [
        (ActiveValue::set(String::from("new")), Some("new")),
        (ActiveValue::unchanged(String::from("old")), Some("old")),
        (ActiveValue::not_set(), None),
    ] {
        assert_eq!(value.try_as_ref().map(String::as_str), expected);
        if let Some(expected) = expected {
            assert_eq!(AsRef::<String>::as_ref(&value), expected);
            assert_eq!(value.clone().unwrap(), expected);
        }
        value.reset();
        assert_eq!(value.is_set(), expected.is_some());
        assert_eq!(value.take().as_deref(), expected);
        assert!(value.is_not_set());
        assert_eq!(value.take(), None);
    }
}

#[test]
fn state_predicates_only_run_for_the_matching_variant() {
    for value in [Set(3), Unchanged(3), NotSet] {
        let calls = Cell::new(0);
        assert_eq!(
            value.is_set_and(|v| {
                calls.set(calls.get() + 1);
                *v == 3
            }),
            value.is_set()
        );
        assert_eq!(calls.get(), usize::from(value.is_set()));
        assert_eq!(
            value.is_unchanged_and(|v| {
                calls.set(calls.get() + 1);
                *v == 3
            }),
            value.is_unchanged()
        );
        assert_eq!(calls.get(), usize::from(!value.is_not_set()));
        assert!(!value.is_set_and(|_| false));
        assert!(!value.is_unchanged_and(|_| false));
    }
}

#[test]
fn conditional_setters_preserve_unchanged_only_when_equal() {
    for initial in [Set(7), Unchanged(7), NotSet] {
        for replacement in [7, 8] {
            let expected = if initial.is_unchanged() && replacement == 7 {
                Unchanged(7)
            } else {
                Set(replacement)
            };
            let mut actual = initial.clone();
            actual.set_ne(replacement);
            assert_eq!(actual, expected);
            let mut alias = initial.clone();
            alias.set_if_not_equals(replacement);
            assert_eq!(alias, expected);
        }
    }
}

#[test]
fn conditional_predicate_applies_only_to_already_set_values() {
    // In particular, a false predicate does NOT reject a replacement of NotSet
    // or an unequal Unchanged value. This observable behavior must survive.
    for initial in [Set(7), Unchanged(7), NotSet] {
        for replacement in [7, 8] {
            for accepted in [false, true] {
                let expected = match initial {
                    Unchanged(7) if replacement == 7 => Unchanged(7),
                    Set(7) if !accepted => Set(7),
                    _ => Set(replacement),
                };
                for alias in [false, true] {
                    let mut actual = initial.clone();
                    let calls = Cell::new(0);
                    let predicate = |_: &i32| {
                        calls.set(calls.get() + 1);
                        accepted
                    };
                    if alias {
                        actual.set_if_not_equals_and(replacement, predicate);
                    } else {
                        actual.set_ne_and(replacement, predicate);
                    }
                    assert_eq!(actual, expected);
                    assert_eq!(calls.get(), usize::from(initial.is_set()));
                }
            }
        }
    }
}

#[test]
fn unset_helpers_are_lazy_and_do_not_replace_explicit_null() {
    for initial in [
        Set(Some(7)),
        Unchanged(Some(7)),
        Set(None),
        Unchanged(None),
        NotSet,
    ] {
        let mut actual = initial.clone();
        actual.set_if_unset(Some(8));
        assert_eq!(
            actual,
            if initial.is_not_set() {
                Set(Some(8))
            } else {
                initial.clone()
            }
        );
        let mut actual = initial.clone();
        let calls = Cell::new(0);
        actual.set_if_unset_with(|| {
            calls.set(calls.get() + 1);
            Some(9)
        });
        assert_eq!(calls.get(), usize::from(initial.is_not_set()));
        assert_eq!(
            actual,
            if initial.is_not_set() {
                Set(Some(9))
            } else {
                initial.clone()
            }
        );
        let mut actual = initial.clone();
        actual.set_if_unset_default();
        assert_eq!(
            actual,
            if initial.is_not_set() {
                Set(None)
            } else {
                initial
            }
        );
    }
}

#[test]
fn optional_conversions_preserve_omitted_versus_explicit_null() {
    let some: ActiveValue<i32> = Some(7).into_active_value();
    let none: ActiveValue<i32> = None::<i32>.into_active_value();
    assert_eq!(some, Set(7));
    assert_eq!(none, NotSet);
    for (input, expected) in [
        (Some(Some(7)), Set(Some(7))),
        (Some(None), Set(None)),
        (None, NotSet),
    ] {
        let actual: ActiveValue<Option<i32>> = input.into_active_value();
        assert_eq!(actual, expected);
    }
    for (input, expected) in [
        (Set(7), Set(Some(7))),
        (Unchanged(7), Unchanged(Some(7))),
        (NotSet, NotSet),
    ] {
        let actual: ActiveValue<Option<i32>> = input.into();
        assert_eq!(actual, expected);
    }
    for (input, expected) in [
        (Set(Some(7)), Some(7)),
        (Unchanged(Some(7)), Some(7)),
        (Set(None), None),
        (Unchanged(None), None),
        (NotSet, None),
    ] {
        assert_eq!(input.as_option().copied(), expected);
        assert_eq!(input.into_option(), expected);
    }
}

#[test]
fn supported_external_and_primitive_types_have_conversion_impls() {
    fn accepts<T: Into<Value> + IntoActiveValue<T>>() {}
    accepts::<bool>();
    accepts::<i8>();
    accepts::<i16>();
    accepts::<i32>();
    accepts::<i64>();
    accepts::<u8>();
    accepts::<u16>();
    accepts::<u32>();
    accepts::<u64>();
    accepts::<f32>();
    accepts::<f64>();
    accepts::<&'static str>();
    accepts::<String>();
    accepts::<Vec<u8>>();
    accepts::<serde_json::Value>();
    accepts::<chrono::NaiveDate>();
    accepts::<chrono::NaiveTime>();
    accepts::<chrono::NaiveDateTime>();
    accepts::<chrono::DateTime<chrono::FixedOffset>>();
    accepts::<chrono::DateTime<chrono::Utc>>();
    accepts::<chrono::DateTime<chrono::Local>>();
    accepts::<rust_decimal::Decimal>();
    accepts::<uuid::Uuid>();
    assert_eq!(
        serde_json::json!({"present": null})
            .into_active_value()
            .into_value(),
        Some(serde_json::json!({"present": null}).into())
    );
    assert_eq!(
        uuid::Uuid::nil().into_active_value().unwrap(),
        uuid::Uuid::nil()
    );
    assert_eq!(
        rust_decimal::Decimal::new(123, 2)
            .into_active_value()
            .unwrap(),
        rust_decimal::Decimal::new(123, 2)
    );
}

#[test]
fn downstream_custom_type_needs_no_clone_eq_or_default_for_basic_operations() {
    struct Owned(i32);
    impl From<Owned> for Value {
        fn from(value: Owned) -> Value {
            value.0.into()
        }
    }
    impl IntoActiveValue<Owned> for Owned {
        fn into_active_value(self) -> ActiveValue<Owned> {
            Set(self)
        }
    }
    let mut value = ActiveValue::unchanged(Owned(7));
    assert_eq!(value.try_as_ref().unwrap().0, 7);
    value.reset();
    assert_eq!(value.take().unwrap().0, 7);
    assert!(value.is_not_set());
    value.set_if_unset_with(|| Owned(8));
    assert_eq!(value.into_value(), Some(Value::Int(Some(8))));
    assert!(Owned(9).into_active_value().is_set());
    let _: ActiveValue<Owned> = Default::default();
}

#[test]
#[should_panic(expected = "Cannot unwrap ActiveValue::NotSet")]
fn omitted_unwrap_keeps_its_panic_contract() {
    ActiveValue::<i32>::NotSet.unwrap();
}

#[test]
#[should_panic(expected = "Cannot borrow ActiveValue::NotSet")]
fn omitted_borrow_keeps_its_panic_contract() {
    let _ = AsRef::<i32>::as_ref(&ActiveValue::NotSet);
}
