//! Built-in `TypeConverter` string coercion round-trips through Noesis.
//! Custom `TypeConverter` registration is not supported at runtime; see LIMITATIONS.md.

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
        let b =
            convert_from_string("Bool", "true").expect("convert_from_string(Bool) returned None");
        assert_eq!(b.as_bool(), Some(true));
        assert_eq!(b.as_i32(), None, "boxed bool must not unbox as i32");
        drop(b);

        let n =
            convert_from_string("Int32", "123").expect("convert_from_string(Int32) returned None");
        assert_eq!(n.as_i32(), Some(123));
        drop(n);

        assert!(
            convert_from_string("Int32", "not-a-number").is_none(),
            "malformed int should not convert"
        );

        let c = convert_from_string("Color", "#FF0000")
            .expect("convert_from_string(Color) returned None");
        assert!(c.as_bool().is_none() && c.as_i32().is_none());
        drop(c);

        assert!(
            convert_from_string("NzTest.NoSuchType", "x").is_none(),
            "unknown type should resolve no converter"
        );
    }

    noesis_runtime::shutdown();
}
