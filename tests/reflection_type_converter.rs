//! TODO §9 (D) — reflection `TypeConverter` consumption (string -> value).
//!
//! Drives `TypeConverter::Get` + `TryConvertFromString` — the exact coercion
//! path the XAML parser uses for a typed attribute string — for built-in /
//! reflected types and asserts the unboxed result. A stubbed `convert_from_string`
//! (returning null) fails every positive assertion below.
//!
//! Custom (Rust-backed) reflection `TypeConverter` *registration* is DEFERRED: in
//! 3.2.13 `TypeConverter::Get` resolves converters through an internal registry
//! that `TypeConverterMetaData` + `Factory::RegisterComponent` do not drive at
//! runtime (verified: a synthetic converter type registers in the Factory yet
//! `Get` still returns null). See TODO.md "Known SDK limitations".

use noesis_runtime::reflection::convert_from_string;

#[test]
fn type_converter_string_coercion_round_trips() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        // bool: "true" -> boxed bool true.
        let b =
            convert_from_string("Bool", "true").expect("convert_from_string(Bool) returned None");
        assert_eq!(b.as_bool(), Some(true));
        assert_eq!(b.as_i32(), None, "boxed bool must not unbox as i32");
        drop(b);

        // int32: "123" -> boxed int 123.
        let n =
            convert_from_string("Int32", "123").expect("convert_from_string(Int32) returned None");
        assert_eq!(n.as_i32(), Some(123));
        drop(n);

        // A malformed integer string must be rejected by the converter.
        assert!(
            convert_from_string("Int32", "not-a-number").is_none(),
            "malformed int should not convert"
        );

        // Color: "#FF0000" parses to a value (a boxed Color, not a primitive).
        let c = convert_from_string("Color", "#FF0000")
            .expect("convert_from_string(Color) returned None");
        assert!(c.as_bool().is_none() && c.as_i32().is_none());
        drop(c);

        // An unregistered type name resolves no converter.
        assert!(
            convert_from_string("DmTest.NoSuchType", "x").is_none(),
            "unknown type should resolve no converter"
        );
    }

    noesis_runtime::shutdown();
}
