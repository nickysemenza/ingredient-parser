//! Tests for unit types (Unit, Measure, MeasureKind) and utilities

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashSet;
use std::str::FromStr;

use ingredient::unit::{is_valid, make_graph, print_graph, Measure, MeasureKind, Unit};
use ingredient::util::num_without_zeroes;

// ============================================================================
// MeasureKind Tests
// ============================================================================

#[test]
fn test_kind() {
    assert_eq!(
        Unit::from_str("g").unwrap(),
        MeasureKind::from_str("weight").unwrap().unit()
    );
    assert_eq!(
        Unit::from_str("ml").unwrap(),
        MeasureKind::from_str("volume").unwrap().unit()
    );
    assert_eq!(
        Unit::from_str("cent").unwrap(),
        MeasureKind::from_str("money").unwrap().unit()
    );
    assert_eq!(
        Unit::from_str("cal").unwrap(),
        MeasureKind::from_str("calories").unwrap().unit()
    );
    assert_eq!(
        Unit::from_str("second").unwrap(),
        MeasureKind::from_str("time").unwrap().unit()
    );
    assert_eq!(
        Unit::from_str("°").unwrap(),
        MeasureKind::from_str("temperature").unwrap().unit()
    );
    assert_eq!(
        Unit::from_str("foo").unwrap().normalize(),
        MeasureKind::from_str("foo").unwrap().unit()
    );
}

// ============================================================================
// Unit Validation Tests
// ============================================================================

#[test]
fn test_is_unit() {
    assert!(is_valid(HashSet::from([]), "oz"));
    assert!(is_valid(HashSet::from([]), "fl oz"));
    assert!(!is_valid(HashSet::from([]), "slice"));
    assert!(is_valid(HashSet::from(["slice".to_string()]), "slice"));
    assert!(is_valid(HashSet::from([]), "TABLESPOONS"));
    assert!(!is_valid(HashSet::from([]), "foo"));
}

#[test]
fn test_back_forth() {
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
fn test_convert() {
    let m = Measure::parse_new("tbsp", 1.0);
    let tbsp_dollars = (
        Measure::parse_new("tbsp", 2.0),
        Measure::parse_new("dollars", 4.0),
    );
    assert_eq!(
        Measure::parse_new("dollars", 2.0),
        m.convert_measure_via_mappings(MeasureKind::Money, vec![tbsp_dollars.clone()])
            .unwrap()
    );

    assert!(m
        .convert_measure_via_mappings(MeasureKind::Volume, vec![tbsp_dollars])
        .is_none());
}

#[test]
fn test_convert_lb() {
    let grams_dollars = (
        Measure::parse_new("gram", 1.0),
        Measure::parse_new("dollar", 1.0),
    );
    assert_eq!(
        Measure::parse_new("dollars", 2.0),
        Measure::parse_new("grams", 2.0)
            .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars.clone()])
            .unwrap()
    );
    assert_eq!(
        Measure::parse_new("dollars", 56.7),
        Measure::parse_new("oz", 2.0)
            .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars.clone()])
            .unwrap()
    );
    assert_eq!(
        Measure::parse_new("dollars", 226.8),
        Measure::parse_new("lb", 0.5)
            .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars.clone()])
            .unwrap()
    );
    assert_eq!(
        Measure::parse_new("dollars", 453.59),
        Measure::parse_new("lb", 1.0)
            .convert_measure_via_mappings(MeasureKind::Money, vec![grams_dollars])
            .unwrap()
    );
}

#[test]
fn test_convert_other() {
    assert_eq!(
        Measure::parse_new("cents", 10.0).denormalize(),
        Measure::parse_new("whole", 1.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                vec![(
                    Measure::parse_new("whole", 12.0),
                    Measure::parse_new("dollar", 1.20),
                )]
            )
            .unwrap()
    );
}

#[test]
fn test_convert_range() {
    assert_eq!(
        Measure::parse_new_with_upper("dollars", 5.0, 10.0),
        Measure::parse_new_with_upper("whole", 1.0, 2.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                vec![(
                    Measure::parse_new("whole", 4.0),
                    Measure::parse_new("dollar", 20.0)
                )]
            )
            .unwrap()
    );
}

#[test]
fn test_convert_transitive() {
    assert_eq!(
        Measure::parse_new("cent", 1.0).denormalize(),
        Measure::parse_new("grams", 1.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                vec![
                    (
                        Measure::parse_new("cent", 1.0),
                        Measure::parse_new("tsp", 1.0)
                    ),
                    (
                        Measure::parse_new("grams", 1.0),
                        Measure::parse_new("tsp", 1.0)
                    ),
                ]
            )
            .unwrap()
    );
    assert_eq!(
        Measure::parse_new("dollar", 1.0),
        Measure::parse_new("grams", 1.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                vec![
                    (
                        Measure::parse_new("dollar", 1.0),
                        Measure::parse_new("cup", 1.0)
                    ),
                    (
                        Measure::parse_new("grams", 1.0),
                        Measure::parse_new("cup", 1.0)
                    ),
                ]
            )
            .unwrap()
    );
}

#[test]
fn test_convert_kcal() {
    assert_eq!(
        Measure::parse_new("kcal", 200.0),
        Measure::parse_new("g", 100.0)
            .convert_measure_via_mappings(
                MeasureKind::Calories,
                vec![
                    (
                        Measure::parse_new("cups", 20.0),
                        Measure::parse_new("grams", 40.0),
                    ),
                    (
                        Measure::parse_new("grams", 20.0),
                        Measure::parse_new("kcal", 40.0),
                    )
                ]
            )
            .unwrap()
    );
}

// ============================================================================
// Graph Tests
// ============================================================================

#[test]
fn test_print_graph() {
    let g = make_graph(vec![
        (
            Measure::parse_new("tbsp", 1.0),
            Measure::parse_new("dollar", 30.0),
        ),
        (
            Measure::parse_new("tsp", 1.0),
            Measure::parse_new("gram", 1.0),
        ),
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
}

// ============================================================================
// Utility Tests
// ============================================================================

#[test]
fn test_num_without_zeroes() {
    assert_eq!(num_without_zeroes(1.0), "1");
    assert_eq!(num_without_zeroes(1.1), "1.1");
    assert_eq!(num_without_zeroes(1.01), "1.01");
    assert_eq!(num_without_zeroes(1.234), "1.23");
}
