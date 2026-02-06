//! Tests for unit types (Unit, Measure, MeasureKind) and utilities

#![allow(clippy::unwrap_used)]

use std::collections::HashSet;
use std::str::FromStr;

use ingredient::unit::{
    is_addon_unit, is_valid, make_graph, print_graph, Measure, MeasureKind, Unit,
};
use ingredient::util::num_without_zeroes;
use rstest::rstest;

// ============================================================================
// Unit FromStr and ToStr Tests (Parameterized)
// ============================================================================

#[rstest]
#[case::gram_g("g", Unit::Gram)]
#[case::gram_full("gram", Unit::Gram)]
#[case::grams("grams", Unit::Gram)]
#[case::kg("kg", Unit::Kilogram)]
#[case::kilogram("kilogram", Unit::Kilogram)]
#[case::liter_l("l", Unit::Liter)]
#[case::liter_full("liter", Unit::Liter)]
#[case::ml("ml", Unit::Milliliter)]
#[case::milliliter("milliliter", Unit::Milliliter)]
#[case::tsp("tsp", Unit::Teaspoon)]
#[case::teaspoon("teaspoon", Unit::Teaspoon)]
#[case::tbsp("tbsp", Unit::Tablespoon)]
#[case::tablespoon("tablespoon", Unit::Tablespoon)]
#[case::cup("cup", Unit::Cup)]
#[case::cup_c("c", Unit::Cup)]
#[case::quart("quart", Unit::Quart)]
#[case::quart_q("q", Unit::Quart)]
#[case::fl_oz("fl oz", Unit::FluidOunce)]
#[case::fluid_oz("fluid oz", Unit::FluidOunce)]
#[case::oz("oz", Unit::Ounce)]
#[case::ounce("ounce", Unit::Ounce)]
#[case::lb("lb", Unit::Pound)]
#[case::pound("pound", Unit::Pound)]
#[case::cent("cent", Unit::Cent)]
#[case::dollar_sym("$", Unit::Dollar)]
#[case::dollar("dollar", Unit::Dollar)]
#[case::kcal("kcal", Unit::KCal)]
#[case::calorie("calorie", Unit::KCal)]
#[case::cal("cal", Unit::KCal)]
#[case::second("second", Unit::Second)]
#[case::sec("sec", Unit::Second)]
#[case::minute("minute", Unit::Minute)]
#[case::min("min", Unit::Minute)]
#[case::hour("hour", Unit::Hour)]
#[case::hr("hr", Unit::Hour)]
#[case::day("day", Unit::Day)]
#[case::fahrenheit("fahrenheit", Unit::Fahrenheit)]
#[case::fahrenheit_f("f", Unit::Fahrenheit)]
#[case::fahrenheit_deg("°", Unit::Fahrenheit)]
#[case::fahrenheit_deg_f("°f", Unit::Fahrenheit)]
#[case::celcius("°c", Unit::Celcius)]
#[case::inch_quote("\"", Unit::Inch)]
#[case::inch("inch", Unit::Inch)]
#[case::whole("whole", Unit::Whole)]
#[case::each("each", Unit::Whole)]
fn test_unit_from_str(#[case] input: &str, #[case] expected: Unit) {
    assert_eq!(Unit::from_str(input).unwrap(), expected);
}

#[test]
fn test_unit_from_str_unknown() {
    assert_eq!(
        Unit::from_str("unknown").unwrap(),
        Unit::Other("unknown".to_string())
    );
}

#[rstest]
#[case::gram(Unit::Gram, "g")]
#[case::kilogram(Unit::Kilogram, "kg")]
#[case::teaspoon(Unit::Teaspoon, "tsp")]
#[case::tablespoon(Unit::Tablespoon, "tbsp")]
#[case::cup(Unit::Cup, "cup")]
#[case::fluid_ounce(Unit::FluidOunce, "fl oz")]
#[case::ounce(Unit::Ounce, "oz")]
#[case::pound(Unit::Pound, "lb")]
#[case::dollar(Unit::Dollar, "$")]
#[case::cent(Unit::Cent, "cent")]
#[case::kcal(Unit::KCal, "kcal")]
#[case::second(Unit::Second, "second")]
#[case::minute(Unit::Minute, "minute")]
#[case::hour(Unit::Hour, "hour")]
#[case::day(Unit::Day, "day")]
#[case::fahrenheit(Unit::Fahrenheit, "fahrenheit")]
#[case::celcius(Unit::Celcius, "celcius")]
#[case::inch(Unit::Inch, "\"")]
#[case::whole(Unit::Whole, "whole")]
fn test_unit_to_str(#[case] unit: Unit, #[case] expected: &str) {
    assert_eq!(unit.to_str(), expected);
}

#[test]
fn test_unit_other_to_str() {
    // Other units return singularized custom string
    assert_eq!(Unit::Other("packets".to_string()).to_str(), "packet");
}

// ============================================================================
// MeasureKind Tests (Parameterized)
// ============================================================================

#[rstest]
#[case::weight(MeasureKind::Weight, "Weight", "weight")]
#[case::volume(MeasureKind::Volume, "Volume", "volume")]
#[case::money(MeasureKind::Money, "Money", "money")]
#[case::calories(MeasureKind::Calories, "Calories", "calories")]
#[case::time(MeasureKind::Time, "Time", "time")]
#[case::temperature(MeasureKind::Temperature, "Temperature", "temperature")]
#[case::length(MeasureKind::Length, "Length", "length")]
fn test_measure_kind_display_and_to_str(
    #[case] kind: MeasureKind,
    #[case] display: &str,
    #[case] to_str: &str,
) {
    assert_eq!(format!("{kind}"), display);
    assert_eq!(kind.to_str(), to_str);
}

#[test]
fn test_measure_kind_other() {
    let other = MeasureKind::Other("custom".to_string());
    assert_eq!(format!("{other}"), "Other(\"custom\")");
    assert_eq!(other.to_str(), "other");
}

#[rstest]
#[case::weight("weight", MeasureKind::Weight)]
#[case::volume("volume", MeasureKind::Volume)]
#[case::money("money", MeasureKind::Money)]
#[case::calories("calories", MeasureKind::Calories)]
#[case::time("time", MeasureKind::Time)]
#[case::temperature("temperature", MeasureKind::Temperature)]
#[case::length("length", MeasureKind::Length)]
fn test_measure_kind_from_str(#[case] input: &str, #[case] expected: MeasureKind) {
    assert_eq!(MeasureKind::from_str(input).unwrap(), expected);
}

#[rstest]
#[case::gram("gram", MeasureKind::Weight)]
#[case::kg("kg", MeasureKind::Weight)]
#[case::oz("oz", MeasureKind::Weight)]
#[case::lb("lb", MeasureKind::Weight)]
#[case::ml("ml", MeasureKind::Volume)]
#[case::liter("liter", MeasureKind::Volume)]
#[case::tsp("tsp", MeasureKind::Volume)]
#[case::tbsp("tbsp", MeasureKind::Volume)]
#[case::cup("cup", MeasureKind::Volume)]
#[case::quart("quart", MeasureKind::Volume)]
#[case::fl_oz("fl oz", MeasureKind::Volume)]
#[case::cent("cent", MeasureKind::Money)]
#[case::dollar("dollar", MeasureKind::Money)]
#[case::second("second", MeasureKind::Time)]
#[case::minute("minute", MeasureKind::Time)]
#[case::hour("hour", MeasureKind::Time)]
#[case::day("day", MeasureKind::Time)]
#[case::fahrenheit("fahrenheit", MeasureKind::Temperature)]
#[case::celcius("°c", MeasureKind::Temperature)]
#[case::kcal("kcal", MeasureKind::Calories)]
#[case::inch("inch", MeasureKind::Length)]
fn test_measure_kind_from_unit(#[case] unit_str: &str, #[case] expected: MeasureKind) {
    assert_eq!(Measure::new(unit_str, 1.0).kind().unwrap(), expected);
}

#[test]
fn test_measure_kind_other_units() {
    assert!(matches!(
        Measure::new("whole", 1.0).kind().unwrap(),
        MeasureKind::Other(_)
    ));
    assert!(matches!(
        Measure::new("custom", 1.0).kind().unwrap(),
        MeasureKind::Other(_)
    ));
}

#[rstest]
#[case::length(MeasureKind::Length, Unit::Inch)]
#[case::weight(MeasureKind::Weight, Unit::Gram)]
#[case::volume(MeasureKind::Volume, Unit::Milliliter)]
#[case::money(MeasureKind::Money, Unit::Cent)]
#[case::calories(MeasureKind::Calories, Unit::KCal)]
#[case::time(MeasureKind::Time, Unit::Second)]
#[case::temperature(MeasureKind::Temperature, Unit::Fahrenheit)]
fn test_measure_kind_unit(#[case] kind: MeasureKind, #[case] expected: Unit) {
    assert_eq!(kind.unit(), expected);
}

// ============================================================================
// Measure Denormalization Tests (Parameterized)
// ============================================================================

#[rstest]
#[case::small_tsp(2.0, "tsp")]
#[case::tbsp(6.0, "tbsp")]
#[case::cups(96.0, "cups")]
#[case::quart(250.0, "quart")]
fn test_teaspoon_denormalize(#[case] tsp_value: f64, #[case] expected_unit: &str) {
    let m = Measure::new("tsp", tsp_value);
    assert_eq!(m.denormalize().unit_as_string(), expected_unit);
}

#[rstest]
#[case::small_sec(30.0, "second")]
#[case::minutes(120.0, "minutes")]
#[case::hour(7200.0, "hour")]
#[case::day(100000.0, "day")]
fn test_second_denormalize(#[case] sec_value: f64, #[case] expected_unit: &str) {
    let m = Measure::new("second", sec_value);
    assert_eq!(m.denormalize().unit_as_string(), expected_unit);
}

#[rstest]
#[case("kg")]
#[case("liter")]
#[case("tablespoon")]
#[case("cup")]
#[case("quart")]
#[case("fl oz")]
#[case("oz")]
#[case("lb")]
#[case("dollar")]
#[case("fahrenheit")]
#[case("celcius")]
#[case("minute")]
#[case("hour")]
#[case("day")]
fn test_denormalize_passthrough(#[case] unit: &str) {
    let m = Measure::new(unit, 1.0);
    assert_eq!(m.denormalize().value(), 1.0);
}

#[test]
fn test_denormalize_with_upper_value() {
    let m = Measure::with_range("tsp", 6.0, 12.0);
    let d = m.denormalize();
    assert!(d.upper_value().is_some());
}

// ============================================================================
// Measure Pluralization Tests
// ============================================================================

#[rstest]
#[case::cup_half("cup", 0.5, "cup")]
#[case::cup_one("cup", 1.0, "cup")]
#[case::cup_two("cup", 2.0, "cups")]
#[case::minute_one("minute", 1.0, "minute")]
#[case::minute_two("minute", 2.0, "minutes")]
#[case::gram("gram", 100.0, "g")]
fn test_measure_pluralization(#[case] unit: &str, #[case] value: f64, #[case] expected: &str) {
    assert_eq!(Measure::new(unit, value).unit_as_string(), expected);
}

// ============================================================================
// Measure Display Tests
// ============================================================================

#[rstest]
#[case::simple("cups", 2.0, None, "2 cups")]
#[case::singular("cup", 1.0, None, "1 cup")]
#[case::range("cups", 1.0, Some(2.0), "1 - 2 cups")]
#[case::zero_range("days", 0.0, Some(3.0), "3 day")]
fn test_measure_display(
    #[case] unit: &str,
    #[case] value: f64,
    #[case] upper: Option<f64>,
    #[case] expected: &str,
) {
    let m = match upper {
        Some(u) => Measure::with_range(unit, value, u),
        None => Measure::new(unit, value),
    };
    assert_eq!(m.to_string(), expected);
}

// ============================================================================
// Measure Add Tests
// ============================================================================

#[test]
fn test_measure_add_different_kinds() {
    let weight = Measure::new("grams", 100.0);
    let volume = Measure::new("cups", 1.0);
    assert!(weight.add(volume).is_err());
}

#[test]
fn test_measure_add_same_kind() {
    let m1 = Measure::new("cups", 1.0);
    let m2 = Measure::new("tbsp", 2.0);
    let result = m1.add(m2);
    assert!(result.is_ok());
    assert!(result.unwrap().value() > 0.0);
}

#[rstest]
#[case::both_ranges(
    Measure::with_range("cups", 1.0, 2.0),
    Measure::with_range("cups", 0.5, 1.0),
    true
)]
#[case::first_range(Measure::with_range("cups", 1.0, 2.0), Measure::new("cups", 1.0), true)]
#[case::second_range(Measure::new("cups", 1.0), Measure::with_range("cups", 0.5, 1.0), true)]
fn test_measure_add_ranges(#[case] m1: Measure, #[case] m2: Measure, #[case] has_upper: bool) {
    let result = m1.add(m2).unwrap();
    assert_eq!(result.upper_value().is_some(), has_upper);
}

#[test]
fn test_measure_add_other_unit() {
    let m1 = Measure::new("cups", 1.0);
    let m2 = Measure::new("unknown_unit", 2.0);
    let result = m1.add(m2).unwrap();
    assert_eq!(result.value(), 1.0);
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
        m.convert_measure_via_mappings(MeasureKind::Money, &[tbsp_dollars.clone()])
            .unwrap()
    );

    // Conversion to incompatible kind fails
    assert!(m
        .convert_measure_via_mappings(MeasureKind::Volume, &[tbsp_dollars])
        .is_none());
}

#[rstest]
#[case::gram_to_dollar("grams", 2.0, 2.0)]
#[case::oz_to_dollar("oz", 2.0, 56.7)]
#[case::half_lb_to_dollar("lb", 0.5, 226.8)]
#[case::lb_to_dollar("lb", 1.0, 453.59)]
fn test_weight_to_money_conversion(
    #[case] unit: &str,
    #[case] amount: f64,
    #[case] expected_dollars: f64,
) {
    let grams_dollars = (Measure::new("gram", 1.0), Measure::new("dollar", 1.0));
    let result = Measure::new(unit, amount)
        .convert_measure_via_mappings(MeasureKind::Money, &[grams_dollars])
        .unwrap();
    assert_eq!(result, Measure::new("dollars", expected_dollars));
}

#[test]
fn test_measure_convert_custom_unit() {
    assert_eq!(
        Measure::new("cents", 10.0).denormalize(),
        Measure::new("whole", 1.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                &[(Measure::new("whole", 12.0), Measure::new("dollar", 1.20))]
            )
            .unwrap()
    );
}

#[test]
fn test_measure_convert_range() {
    assert_eq!(
        Measure::with_range("dollars", 5.0, 10.0),
        Measure::with_range("whole", 1.0, 2.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                &[(Measure::new("whole", 4.0), Measure::new("dollar", 20.0))]
            )
            .unwrap()
    );
}

#[test]
fn test_measure_convert_transitive() {
    // A -> B -> C conversion
    assert_eq!(
        Measure::new("cent", 1.0).denormalize(),
        Measure::new("grams", 1.0)
            .convert_measure_via_mappings(
                MeasureKind::Money,
                &[
                    (Measure::new("cent", 1.0), Measure::new("tsp", 1.0)),
                    (Measure::new("grams", 1.0), Measure::new("tsp", 1.0)),
                ]
            )
            .unwrap()
    );
}

#[test]
fn test_measure_convert_no_path() {
    let m = Measure::new("inch", 1.0);
    let result = m.convert_measure_via_mappings(
        MeasureKind::Money,
        &[(Measure::new("gram", 1.0), Measure::new("dollar", 1.0))],
    );
    assert!(result.is_none());
}

// ============================================================================
// Unit Validation and Utilities
// ============================================================================

#[rstest]
#[case::oz("oz", true)]
#[case::fl_oz("fl oz", true)]
#[case::tablespoons("TABLESPOONS", true)]
#[case::slice("slice", false)]
#[case::foo("foo", false)]
fn test_unit_validation(#[case] unit: &str, #[case] expected: bool) {
    assert_eq!(is_valid(&HashSet::new(), unit), expected);
}

#[test]
fn test_unit_validation_custom() {
    let custom = HashSet::from(["slice".to_string()]);
    assert!(is_valid(&custom, "slice"));
}

#[test]
fn test_unit_normalize() {
    let unit = Unit::from_str("packets").unwrap();
    assert_eq!(unit.normalize(), Unit::Other("packet".to_string()));
    let gram = Unit::from_str("gram").unwrap();
    assert_eq!(gram.normalize(), Unit::Gram);
}

#[test]
fn test_unit_display() {
    assert_eq!(format!("{}", Unit::Gram), "g");
    assert_eq!(format!("{}", Unit::Cup), "cup");
    assert_eq!(format!("{}", Unit::Other("custom".to_string())), "custom");
}

#[test]
fn test_is_addon_unit() {
    let custom_units: HashSet<String> = HashSet::from(["packet".to_string(), "slice".to_string()]);

    assert!(is_addon_unit(&custom_units, "packet"));
    assert!(is_addon_unit(&custom_units, "slice"));
    assert!(is_addon_unit(&custom_units, "PACKET")); // Case insensitive
    assert!(!is_addon_unit(&custom_units, "cup"));
    assert!(!is_addon_unit(&custom_units, "unknown"));
}

#[rstest]
#[case::one(1.0, "1")]
#[case::one_one(1.1, "1.1")]
#[case::one_point_oh_one(1.01, "1.01")]
#[case::truncated(1.234, "1.23")]
fn test_num_without_zeroes(#[case] input: f64, #[case] expected: &str) {
    assert_eq!(num_without_zeroes(input), expected);
}

// ============================================================================
// Graph Tests
// ============================================================================

#[test]
fn test_unit_and_kind_mapping() {
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
}

#[test]
fn test_graph_creation_and_printing() {
    let g = make_graph(&[
        (Measure::new("tbsp", 1.0), Measure::new("dollar", 30.0)),
        (Measure::new("tsp", 1.0), Measure::new("gram", 1.0)),
    ]);
    assert_eq!(
        print_graph(g),
        r#"digraph {
    0 [ label = "tsp" ]
    1 [ label = "cent" ]
    2 [ label = "g" ]
    0 -> 1 [ label = "1000" ]
    1 -> 0 [ label = "0.001" ]
    0 -> 2 [ label = "1" ]
    2 -> 0 [ label = "1" ]
}
"#
    );
}

#[test]
fn test_measure_custom_unit_singularized() {
    let m = Measure::new("packets", 2.0);
    assert_eq!(m.unit_as_string(), "packet");
}
