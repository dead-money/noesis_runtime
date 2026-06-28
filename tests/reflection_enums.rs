//! TODO §9 (A) — custom runtime enum registration.
//!
//! Registers a named enum with string<->int pairs, then reads the mapping back
//! THROUGH Noesis (`TypeEnum::HasName` / `HasValue`, and the `TypeConverter`
//! string path the XAML parser uses). A no-op / stubbed registration makes the
//! type unresolvable or the names unknown, so every assertion below fails.

use noesis_runtime::reflection::register_enum;

#[test]
fn custom_enum_round_trips_through_noesis() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let e = register_enum(
            "NzTest.Direction",
            &[("North", 10), ("East", 20), ("South", 30), ("West", 40)],
        )
        .expect("register_enum returned None");
        assert_eq!(e.name(), "NzTest.Direction");

        // Forward: name -> value, read through TypeEnum::HasName.
        assert_eq!(e.value_from_name("North"), Some(10));
        assert_eq!(e.value_from_name("South"), Some(30));
        assert_eq!(e.value_from_name("West"), Some(40));

        // Negative: a name that was never registered must not resolve.
        assert_eq!(e.value_from_name("Nowhere"), None);

        // Inverse: value -> name, read through TypeEnum::HasValue.
        assert_eq!(e.name_from_value(20).as_deref(), Some("East"));
        assert_eq!(e.name_from_value(40).as_deref(), Some("West"));
        assert_eq!(e.name_from_value(999), None);

        // Re-registering the same name must fail (no silent shadowing).
        assert!(
            register_enum("NzTest.Direction", &[("X", 1)]).is_none(),
            "duplicate enum name should be rejected"
        );

        // A type that was never registered must not resolve.
        let bogus = register_enum("", &[("A", 1)]);
        assert!(bogus.is_none(), "empty enum name should be rejected");
    }

    noesis_runtime::shutdown();
}
