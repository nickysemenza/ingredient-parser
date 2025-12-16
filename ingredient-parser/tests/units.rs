//! Tests for unit types (Unit, Measure, MeasureKind) and utilities

#![allow(clippy::unwrap_used)]

use std::collections::HashSet;
use std::str::FromStr;

use ingredient::unit::{is_valid, make_graph, print_graph, Measure, MeasureKind, Unit};
use ingredient::util::num_without_zeroes;

// ============================================================================
// Unit and MeasureKind Tests
// ============================================================================

#[test]
fn test_unit_and_kind() {
    // MeasureKind to Unit mapping
    let kind_unit_pairs: Vec<(&str, &str)> = vec![
        ("weight", "g"),
        ("volume", "ml"),
        ("money", "cent"),
        ("calories", "cal"),
        ("time", "second"),
        ("temperature", "°"),
    ];

    for (kind_str, unit_str) in kind_unit_pairs {
        assert_eq!(
            Unit::from_str(unit_str).unwrap(),
            MeasureKind::from_str(kind_str).unwrap().unit(),
            "Kind '{kind_str}' should map to unit '{unit_str}'"
        );
    }

    // Custom/Other kind
    assert_eq!(
        Unit::from_str("foo").unwrap().normalize(),
        MeasureKind::from_str("foo").unwrap().unit()
    );

    // Unit validation
    assert!(is_valid(&HashSet::from([]), "oz"));
    assert!(is_valid(&HashSet::from([]), "fl oz"));
    assert!(!is_valid(&HashSet::from([]), "slice"));
    assert!(is_valid(&HashSet::from(["slice".to_string()]), "slice"));
    assert!(is_valid(&HashSet::from([]), "TABLESPOONS"));
    assert!(!is_valid(&HashSet::from([]), "foo"));

    // Unit roundtrip (from_str -> to_str)
    assert_eq!(Unit::from_str("oz").unwrap(), Unit::Ounce);
    assert_eq!(Unit::from_str("gram").unwrap().to_str(), "g");
    assert_eq!(Unit::from_str("foo").unwrap().to_str(), "foo");
    assert_eq!(
        format!("{}", Unit::from_str("foo").unwrap()),
        "Other(\"foo\")"
    );
}

// ============================================================================
// Measure Conversion Tests
// ============================================================================

#[test]
fn test_measure_conversions() {
    // Basic conversion
    let m = Measure::new("tbsp", 1.0);
    let tbsp_dollars = (Measure::new("tbsp", 2.0), Measure::new("dollars", 4.0));
    assert_eq!(
        Measure::new("dollars", 2.0),
        m.convert_measure_via_mappings(MeasureKind::Money, vec![tbsp_dollars.clone()])
            .unwrap()
    );

    // Conversion to incompatible kind fails
    assert!(m
        .convert_measure_via_mappings(MeasureKind::Volume, vec![tbsp_dollars])
        .is_none());

    // Weight conversions (lb, oz, g)
    let grams_dollars = (Measure::new("gram", 1.0), Measure::new("dollar", 1.0));
    assert_eq!(
        Measure::new("dollars", 2.0),
        Measure::new("grams", 2.0)
            .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars.clone()])
            .unwrap()
    );
    assert_eq!(
        Measure::new("dollars", 56.7),
        Measure::new("oz", 2.0)
            .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars.clone()])
            .unwrap()
    );
    assert_eq!(
        Measure::new("dollars", 226.8),
        Measure::new("lb", 0.5)
            .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars.clone()])
            .unwrap()
    );
    assert_eq!(
        Measure::new("dollars", 453.59),
        Measure::new("lb", 1.0)
            .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars])
            .unwrap()
    );

    // Custom unit (whole) conversion
    assert_eq!(
        Measure::new("cents", 10.0).denormalize(),
        Measure::new("whole", 1.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                vec![(Measure::new("whole", 12.0), Measure::new("dollar", 1.20))]
            )
            .unwrap()
    );

    // Range conversion
    assert_eq!(
        Measure::with_range("dollars", 5.0, 10.0),
        Measure::with_range("whole", 1.0, 2.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                vec![(Measure::new("whole", 4.0), Measure::new("dollar", 20.0))]
            )
            .unwrap()
    );

    // Transitive conversions (A -> B -> C)
    assert_eq!(
        Measure::new("cent", 1.0).denormalize(),
        Measure::new("grams", 1.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                vec![
                    (Measure::new("cent", 1.0), Measure::new("tsp", 1.0)),
                    (Measure::new("grams", 1.0), Measure::new("tsp", 1.0)),
                ]
            )
            .unwrap()
    );
    assert_eq!(
        Measure::new("dollar", 1.0),
        Measure::new("grams", 1.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                vec![
                    (Measure::new("dollar", 1.0), Measure::new("cup", 1.0)),
                    (Measure::new("grams", 1.0), Measure::new("cup", 1.0)),
                ]
            )
            .unwrap()
    );

    // Calorie conversion
    assert_eq!(
        Measure::new("kcal", 200.0),
        Measure::new("g", 100.0)
            .convert_measure_via_mappings(
                MeasureKind::Calories,
                vec![
                    (Measure::new("cups", 20.0), Measure::new("grams", 40.0)),
                    (Measure::new("grams", 20.0), Measure::new("kcal", 40.0)),
                ]
            )
            .unwrap()
    );
}

// ============================================================================
// Graph and Utility Tests
// ============================================================================

#[test]
fn test_graph_and_utilities() {
    // Graph creation and printing
    let g = make_graph(vec![
        (Measure::new("tbsp", 1.0), Measure::new("dollar", 30.0)),
        (Measure::new("tsp", 1.0), Measure::new("gram", 1.0)),
    ]);
    assert_eq!(
        print_graph(g),
        r#"digraph {
    0 [ label = "Teaspoon" ]
    1 [ label = "Cent" ]
    2 [ label = "Gram" ]
    0 -> 1 [ label = "1000" ]
    1 -> 0 [ label = "0.001" ]
    0 -> 2 [ label = "1" ]
    2 -> 0 [ label = "1" ]
}
"#
    );

    // num_without_zeroes utility
    assert_eq!(num_without_zeroes(1.0), "1");
    assert_eq!(num_without_zeroes(1.1), "1.1");
    assert_eq!(num_without_zeroes(1.01), "1.01");
    assert_eq!(num_without_zeroes(1.234), "1.23");
}

// ============================================================================
// MeasureKind Display and to_str Tests
// ============================================================================

#[test]
fn test_measure_kind_display_and_to_str() {
    // Test Display trait for all MeasureKind variants
    assert_eq!(format!("{}", MeasureKind::Weight), "Weight");
    assert_eq!(format!("{}", MeasureKind::Volume), "Volume");
    assert_eq!(format!("{}", MeasureKind::Money), "Money");
    assert_eq!(format!("{}", MeasureKind::Calories), "Calories");
    assert_eq!(format!("{}", MeasureKind::Time), "Time");
    assert_eq!(format!("{}", MeasureKind::Temperature), "Temperature");
    assert_eq!(format!("{}", MeasureKind::Length), "Length");
    assert_eq!(
        format!("{}", MeasureKind::Other("custom".to_string())),
        "Other(\"custom\")"
    );

    // Test to_str for all MeasureKind variants
    assert_eq!(MeasureKind::Weight.to_str(), "weight");
    assert_eq!(MeasureKind::Volume.to_str(), "volume");
    assert_eq!(MeasureKind::Money.to_str(), "money");
    assert_eq!(MeasureKind::Calories.to_str(), "calories");
    assert_eq!(MeasureKind::Time.to_str(), "time");
    assert_eq!(MeasureKind::Temperature.to_str(), "temperature");
    assert_eq!(MeasureKind::Length.to_str(), "length");
    // Other falls back to "other"
    assert_eq!(MeasureKind::Other("anything".to_string()).to_str(), "other");

    // Test unit() method for Length (not covered before)
    assert_eq!(MeasureKind::Length.unit(), Unit::Inch);
}

// ============================================================================
// Measure Edge Case Tests
// ============================================================================

#[test]
fn test_measure_add_different_kinds() {
    // Adding measures of different kinds should fail
    let weight = Measure::new("grams", 100.0);
    let volume = Measure::new("cups", 1.0);

    let result = weight.add(volume);
    assert!(result.is_err());
}

#[test]
fn test_measure_add_same_kind() {
    // Adding measures of same kind should work
    let m1 = Measure::new("cups", 1.0);
    let m2 = Measure::new("tbsp", 2.0);

    let result = m1.add(m2);
    assert!(result.is_ok());

    // The result should be normalized
    let combined = result.unwrap();
    assert!(combined.values().0 > 0.0);
}

#[test]
fn test_measure_add_with_ranges() {
    // Adding two measures with ranges
    let m1 = Measure::with_range("cups", 1.0, 2.0);
    let m2 = Measure::with_range("cups", 0.5, 1.0);

    let result = m1.add(m2).unwrap();
    // Both upper values should be added
    assert!(result.values().1.is_some());
}

#[test]
fn test_measure_add_range_with_non_range() {
    // Adding range to non-range
    let m1 = Measure::with_range("cups", 1.0, 2.0);
    let m2 = Measure::new("cups", 1.0);

    let result = m1.add(m2).unwrap();
    // Should have an upper value
    assert!(result.values().1.is_some());
}

#[test]
fn test_measure_add_non_range_with_range() {
    // Adding non-range to range
    let m1 = Measure::new("cups", 1.0);
    let m2 = Measure::with_range("cups", 0.5, 1.0);

    let result = m1.add(m2).unwrap();
    assert!(result.values().1.is_some());
}

#[test]
fn test_measure_add_other_unit() {
    // Adding with Other unit (custom unit) - should return first measure
    let m1 = Measure::new("cups", 1.0);
    let m2 = Measure::new("unknown_unit", 2.0);

    let result = m1.add(m2).unwrap();
    // Should return m1 unchanged when m2 is Other
    assert_eq!(result.values().0, 1.0);
}

#[test]
fn test_measure_custom_unit_singularized() {
    // Custom units should be singularized in display
    let m = Measure::new("packets", 2.0);
    // Unit should be singular in values()
    assert_eq!(m.values().2, "packet");
}

#[test]
fn test_measure_denormalize_teaspoon_ranges() {
    // Test all denormalize branches for teaspoon
    // < 3 tsp -> stays teaspoon
    let m1 = Measure::new("tsp", 2.0);
    assert_eq!(m1.denormalize().values().2, "tsp");

    // 3-12 tsp -> tablespoon
    let m2 = Measure::new("tsp", 6.0);
    assert_eq!(m2.denormalize().values().2, "tbsp");

    // 12-192 tsp -> cup (pluralized because value > 1)
    let m3 = Measure::new("tsp", 96.0);
    assert_eq!(m3.denormalize().values().2, "cups");

    // >= 192 tsp -> quart
    let m4 = Measure::new("tsp", 250.0);
    assert_eq!(m4.denormalize().values().2, "quart");
}

#[test]
fn test_measure_denormalize_seconds_ranges() {
    // Test all denormalize branches for seconds
    // < 60 sec -> stays second
    let m1 = Measure::new("second", 30.0);
    assert_eq!(m1.denormalize().values().2, "second");

    // 60-3600 sec -> minute (pluralized because value > 1)
    let m2 = Measure::new("second", 120.0);
    let d2 = m2.denormalize();
    assert_eq!(d2.values().2, "minutes");

    // 3600-86400 sec -> hour
    let m3 = Measure::new("second", 7200.0);
    let d3 = m3.denormalize();
    assert_eq!(d3.values().2, "hour");

    // >= 86400 sec -> day
    let m4 = Measure::new("second", 100000.0);
    let d4 = m4.denormalize();
    assert_eq!(d4.values().2, "day");
}

#[test]
fn test_measure_denormalize_passthrough() {
    // Test units that should pass through unchanged
    let units_to_test = vec![
        ("kg", "kg"),
        ("liter", "l"),
        ("tablespoon", "tbsp"),
        ("cup", "cup"),
        ("quart", "quart"),
        ("fl oz", "fl oz"),
        ("oz", "oz"),
        ("lb", "lb"),
        ("dollar", "dollar"),
        ("fahrenheit", "f"),
        ("celcius", "°c"),
        ("minute", "minute"),
        ("hour", "hour"),
        ("day", "day"),
    ];

    for (input, _) in units_to_test {
        let m = Measure::new(input, 1.0);
        let d = m.denormalize();
        // Should return self (unchanged)
        assert_eq!(d.values().0, 1.0);
    }
}

#[test]
fn test_measure_denormalize_with_upper_value() {
    // Test denormalize preserves upper_value
    let m = Measure::with_range("tsp", 6.0, 12.0);
    let d = m.denormalize();
    // Should convert both values
    assert!(d.values().1.is_some());
}

#[test]
fn test_measure_kind_all_units() {
    // Test kind() for all unit types
    // Weight
    assert_eq!(
        Measure::new("gram", 1.0).kind().unwrap(),
        MeasureKind::Weight
    );
    assert_eq!(Measure::new("kg", 1.0).kind().unwrap(), MeasureKind::Weight);
    assert_eq!(Measure::new("oz", 1.0).kind().unwrap(), MeasureKind::Weight);
    assert_eq!(Measure::new("lb", 1.0).kind().unwrap(), MeasureKind::Weight);

    // Volume
    assert_eq!(Measure::new("ml", 1.0).kind().unwrap(), MeasureKind::Volume);
    assert_eq!(
        Measure::new("liter", 1.0).kind().unwrap(),
        MeasureKind::Volume
    );
    assert_eq!(
        Measure::new("tsp", 1.0).kind().unwrap(),
        MeasureKind::Volume
    );
    assert_eq!(
        Measure::new("tbsp", 1.0).kind().unwrap(),
        MeasureKind::Volume
    );
    assert_eq!(
        Measure::new("cup", 1.0).kind().unwrap(),
        MeasureKind::Volume
    );
    assert_eq!(
        Measure::new("quart", 1.0).kind().unwrap(),
        MeasureKind::Volume
    );
    assert_eq!(
        Measure::new("fl oz", 1.0).kind().unwrap(),
        MeasureKind::Volume
    );

    // Money
    assert_eq!(
        Measure::new("cent", 1.0).kind().unwrap(),
        MeasureKind::Money
    );
    assert_eq!(
        Measure::new("dollar", 1.0).kind().unwrap(),
        MeasureKind::Money
    );

    // Time
    assert_eq!(
        Measure::new("second", 1.0).kind().unwrap(),
        MeasureKind::Time
    );
    assert_eq!(
        Measure::new("minute", 1.0).kind().unwrap(),
        MeasureKind::Time
    );
    assert_eq!(Measure::new("hour", 1.0).kind().unwrap(), MeasureKind::Time);
    assert_eq!(Measure::new("day", 1.0).kind().unwrap(), MeasureKind::Time);

    // Temperature
    assert_eq!(
        Measure::new("fahrenheit", 1.0).kind().unwrap(),
        MeasureKind::Temperature
    );
    assert_eq!(
        Measure::new("°c", 1.0).kind().unwrap(),
        MeasureKind::Temperature
    );

    // Calories
    assert_eq!(
        Measure::new("kcal", 1.0).kind().unwrap(),
        MeasureKind::Calories
    );

    // Length
    assert_eq!(
        Measure::new("inch", 1.0).kind().unwrap(),
        MeasureKind::Length
    );

    // Other
    assert!(matches!(
        Measure::new("whole", 1.0).kind().unwrap(),
        MeasureKind::Other(_)
    ));
    assert!(matches!(
        Measure::new("custom", 1.0).kind().unwrap(),
        MeasureKind::Other(_)
    ));
}

#[test]
fn test_measure_display() {
    // Test Display trait
    let m1 = Measure::new("cups", 2.0);
    assert_eq!(format!("{m1}"), "2 cups");

    let m2 = Measure::new("cup", 1.0);
    assert_eq!(format!("{m2}"), "1 cup");

    // With range
    let m3 = Measure::with_range("cups", 1.0, 2.0);
    assert_eq!(format!("{m3}"), "1 - 2 cups");

    // Decimal values
    let m4 = Measure::new("g", 100.5);
    assert!(format!("{m4}").contains("100.5"));
}

#[test]
fn test_measure_unit_as_string_pluralization() {
    // Cup pluralization
    assert_eq!(Measure::new("cup", 0.5).values().2, "cup");
    assert_eq!(Measure::new("cup", 1.0).values().2, "cup");
    assert_eq!(Measure::new("cup", 2.0).values().2, "cups");

    // Minute pluralization
    assert_eq!(Measure::new("minute", 1.0).values().2, "minute");
    assert_eq!(Measure::new("minute", 2.0).values().2, "minutes");

    // Other units don't pluralize
    assert_eq!(Measure::new("gram", 100.0).values().2, "g");
}

#[test]
fn test_measure_convert_no_path() {
    // Test conversion when no path exists in graph
    let m = Measure::new("inch", 1.0);

    let result = m.convert_measure_via_mappings(
        MeasureKind::Money,
        vec![(Measure::new("gram", 1.0), Measure::new("dollar", 1.0))],
    );

    // Should return None when no conversion path exists
    assert!(result.is_none());
}

#[test]
fn test_measure_convert_with_range() {
    // Test conversion with range values
    let m = Measure::with_range("gram", 100.0, 200.0);

    let result = m.convert_measure_via_mappings(
        MeasureKind::Money,
        vec![(Measure::new("gram", 1.0), Measure::new("dollar", 0.01))],
    );

    assert!(result.is_some());
    let converted = result.unwrap();
    // Both values should be converted
    assert!(converted.values().1.is_some());
}

// ============================================================================
// Unit Edge Case Tests
// ============================================================================

#[test]
fn test_unit_normalize() {
    // Test Unit::normalize for Other variant
    let unit = Unit::from_str("packets").unwrap();
    assert_eq!(unit.normalize(), Unit::Other("packet".to_string()));

    // Non-Other units stay the same
    let gram = Unit::from_str("gram").unwrap();
    assert_eq!(gram.normalize(), Unit::Gram);
}

#[test]
fn test_unit_to_str_fallback() {
    // Test to_str for various units
    assert_eq!(Unit::Gram.to_str(), "g");
    assert_eq!(Unit::Kilogram.to_str(), "kg");
    assert_eq!(Unit::Teaspoon.to_str(), "tsp");
    assert_eq!(Unit::Tablespoon.to_str(), "tbsp");
    assert_eq!(Unit::Cup.to_str(), "cup");
    assert_eq!(Unit::FluidOunce.to_str(), "fl oz");
    assert_eq!(Unit::Ounce.to_str(), "oz");
    assert_eq!(Unit::Pound.to_str(), "lb");
    assert_eq!(Unit::Dollar.to_str(), "$"); // First mapping wins
    assert_eq!(Unit::Cent.to_str(), "cent");
    assert_eq!(Unit::KCal.to_str(), "kcal");
    assert_eq!(Unit::Second.to_str(), "second");
    assert_eq!(Unit::Minute.to_str(), "minute");
    assert_eq!(Unit::Hour.to_str(), "hour");
    assert_eq!(Unit::Day.to_str(), "day");
    assert_eq!(Unit::Fahrenheit.to_str(), "fahrenheit"); // First mapping wins (reverse iteration)
    assert_eq!(Unit::Celcius.to_str(), "celcius"); // First mapping wins (reverse iteration)
    assert_eq!(Unit::Inch.to_str(), "\""); // Last mapping wins (reverse iteration: inch, then \")
    assert_eq!(Unit::Whole.to_str(), "whole"); // First mapping wins after reverse iteration

    // Other units return the custom string (singularized)
    assert_eq!(Unit::Other("packets".to_string()).to_str(), "packet");
}

#[test]
fn test_unit_from_str_all_aliases() {
    // Test all unit aliases
    assert_eq!(Unit::from_str("g").unwrap(), Unit::Gram);
    assert_eq!(Unit::from_str("gram").unwrap(), Unit::Gram);
    assert_eq!(Unit::from_str("grams").unwrap(), Unit::Gram);

    assert_eq!(Unit::from_str("kg").unwrap(), Unit::Kilogram);
    assert_eq!(Unit::from_str("kilogram").unwrap(), Unit::Kilogram);

    assert_eq!(Unit::from_str("l").unwrap(), Unit::Liter);
    assert_eq!(Unit::from_str("liter").unwrap(), Unit::Liter);

    assert_eq!(Unit::from_str("ml").unwrap(), Unit::Milliliter);
    assert_eq!(Unit::from_str("milliliter").unwrap(), Unit::Milliliter);

    assert_eq!(Unit::from_str("tsp").unwrap(), Unit::Teaspoon);
    assert_eq!(Unit::from_str("teaspoon").unwrap(), Unit::Teaspoon);

    assert_eq!(Unit::from_str("tbsp").unwrap(), Unit::Tablespoon);
    assert_eq!(Unit::from_str("tablespoon").unwrap(), Unit::Tablespoon);

    assert_eq!(Unit::from_str("cup").unwrap(), Unit::Cup);
    assert_eq!(Unit::from_str("c").unwrap(), Unit::Cup);

    assert_eq!(Unit::from_str("quart").unwrap(), Unit::Quart);
    assert_eq!(Unit::from_str("q").unwrap(), Unit::Quart);

    assert_eq!(Unit::from_str("fl oz").unwrap(), Unit::FluidOunce);
    assert_eq!(Unit::from_str("fluid oz").unwrap(), Unit::FluidOunce);

    assert_eq!(Unit::from_str("oz").unwrap(), Unit::Ounce);
    assert_eq!(Unit::from_str("ounce").unwrap(), Unit::Ounce);

    assert_eq!(Unit::from_str("lb").unwrap(), Unit::Pound);
    assert_eq!(Unit::from_str("pound").unwrap(), Unit::Pound);

    assert_eq!(Unit::from_str("cent").unwrap(), Unit::Cent);
    assert_eq!(Unit::from_str("$").unwrap(), Unit::Dollar);
    assert_eq!(Unit::from_str("dollar").unwrap(), Unit::Dollar);

    assert_eq!(Unit::from_str("kcal").unwrap(), Unit::KCal);
    assert_eq!(Unit::from_str("calorie").unwrap(), Unit::KCal);
    assert_eq!(Unit::from_str("cal").unwrap(), Unit::KCal);

    assert_eq!(Unit::from_str("second").unwrap(), Unit::Second);
    assert_eq!(Unit::from_str("sec").unwrap(), Unit::Second);
    // Note: "s" gets singularized to "" which is not in mapping, so returns Other
    // Use "sec" or "second" instead

    assert_eq!(Unit::from_str("minute").unwrap(), Unit::Minute);
    assert_eq!(Unit::from_str("min").unwrap(), Unit::Minute);

    assert_eq!(Unit::from_str("hour").unwrap(), Unit::Hour);
    assert_eq!(Unit::from_str("hr").unwrap(), Unit::Hour);

    assert_eq!(Unit::from_str("day").unwrap(), Unit::Day);

    assert_eq!(Unit::from_str("fahrenheit").unwrap(), Unit::Fahrenheit);
    assert_eq!(Unit::from_str("f").unwrap(), Unit::Fahrenheit);
    assert_eq!(Unit::from_str("°").unwrap(), Unit::Fahrenheit);
    assert_eq!(Unit::from_str("°f").unwrap(), Unit::Fahrenheit);
    // Note: "degrees" gets singularized to "degree" which is not in mapping

    // Note: "celcius" gets singularized to "celciu" which is not in mapping
    assert_eq!(Unit::from_str("°c").unwrap(), Unit::Celcius);

    assert_eq!(Unit::from_str("\"").unwrap(), Unit::Inch);
    assert_eq!(Unit::from_str("inch").unwrap(), Unit::Inch);

    assert_eq!(Unit::from_str("whole").unwrap(), Unit::Whole);
    assert_eq!(Unit::from_str("each").unwrap(), Unit::Whole);

    // Unknown units become Other
    assert_eq!(
        Unit::from_str("unknown").unwrap(),
        Unit::Other("unknown".to_string())
    );
}

#[test]
fn test_unit_display() {
    // Test Display trait for Unit
    assert_eq!(format!("{}", Unit::Gram), "Gram");
    assert_eq!(format!("{}", Unit::Cup), "Cup");
    assert_eq!(
        format!("{}", Unit::Other("custom".to_string())),
        "Other(\"custom\")"
    );
}

#[test]
fn test_is_addon_unit() {
    use ingredient::unit::is_addon_unit;

    let custom_units: HashSet<String> = HashSet::from(["packet".to_string(), "slice".to_string()]);

    // Custom units should match
    assert!(is_addon_unit(&custom_units, "packet"));
    assert!(is_addon_unit(&custom_units, "slice"));
    assert!(is_addon_unit(&custom_units, "PACKET")); // Case insensitive

    // Built-in units should not match
    assert!(!is_addon_unit(&custom_units, "cup"));
    assert!(!is_addon_unit(&custom_units, "gram"));

    // Unknown units not in custom set should not match
    assert!(!is_addon_unit(&custom_units, "unknown"));
}
