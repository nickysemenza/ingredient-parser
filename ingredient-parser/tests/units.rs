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
