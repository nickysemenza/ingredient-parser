use super::kind::MeasureKind;
use super::measure::is_nutrient_unit;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

pub fn is_valid(units: &HashSet<String>, s: &str) -> bool {
    // Unit::from_str always returns Ok - check if it's a known unit (not Other)
    if !matches!(Unit::from_str(&singular(s)), Ok(Unit::Other(_))) {
        // anything other than `other`
        return true;
    }
    is_addon_unit(units, s)
}

/// Check if a string matches an addon unit (from the custom units set)
///
/// This does NOT check built-in units - use `is_valid` for that.
pub(crate) fn is_addon_unit(units: &HashSet<String>, s: &str) -> bool {
    units.contains(&s.to_lowercase())
}

// NOTE: deliberately NOT `#[non_exhaustive]` (considered for todo 009). The
// integration tests (`tests/units.rs`, `tests/parsing.rs`) — which compile as
// separate crates — construct `Unit` variants by value extensively (e.g.
// `Unit::Gram`, `Unit::Other(...)`), and `#[non_exhaustive]` forbids external
// variant construction (E0639). Hardening this would require routing all those
// constructions through an in-crate constructor; deferred as out of scope.
#[derive(Clone, PartialEq, PartialOrd, Debug, Eq, Hash, Serialize, Deserialize)]
pub enum Unit {
    Gram,
    Kilogram,
    Liter,
    Milliliter,
    Teaspoon,
    Tablespoon,
    Cup,
    Quart,
    FluidOunce,
    Ounce,
    Pound,
    Cent,
    Dollar,
    KCal,
    // time
    Day,
    Hour,
    Minute,
    Second,
    // temperature
    Fahrenheit,
    Celsius,
    //distance
    Inch,
    Whole,
    // https://stackoverflow.com/a/77723851
    #[serde(untagged)]
    Other(String),
}

impl Unit {
    /// Canonicalize a unit.
    ///
    /// Built-in variants are already canonical and returned unchanged. An
    /// `Other` may hold a known alias when constructed directly (the variant is
    /// public), e.g. `Unit::Other("cup".into())`; such values are promoted to
    /// their built-in (`Unit::Cup`). A genuinely-unknown unit is normalized to
    /// its lowercase, singular form (e.g. `Other("Cloves")` -> `Other("clove")`).
    pub fn normalize(&self) -> Unit {
        match self {
            // `from_str` lower-cases/singularizes for lookup and promotes known
            // aliases; it only ever returns `Other` for truly-unknown units (and
            // never `Err`), in which case we canonicalize the stored text.
            Unit::Other(x) => match Unit::from_str(x) {
                Ok(Unit::Other(_)) | Err(()) => Unit::Other(singular(x).into_owned()),
                Ok(known) => known,
            },
            // Built-in variants hold no heap data, so this clone is a cheap copy;
            // taking `&self` lets callers normalize a borrow without cloning first.
            canonical => canonical.clone(),
        }
    }
    /// The unit's canonical string form. Built-in variants borrow a `&'static
    /// str`; only `Other(s)` allocates (to singularize), so `Display` and other
    /// output paths avoid a per-call `String` for the common case.
    pub fn to_str(&self) -> Cow<'static, str> {
        match self {
            Unit::Gram => Cow::Borrowed("g"),
            Unit::Kilogram => Cow::Borrowed("kg"),
            Unit::Liter => Cow::Borrowed("l"),
            Unit::Milliliter => Cow::Borrowed("ml"),
            Unit::Teaspoon => Cow::Borrowed("tsp"),
            Unit::Tablespoon => Cow::Borrowed("tbsp"),
            Unit::Cup => Cow::Borrowed("cup"),
            Unit::Quart => Cow::Borrowed("quart"),
            Unit::FluidOunce => Cow::Borrowed("fl oz"),
            Unit::Ounce => Cow::Borrowed("oz"),
            Unit::Pound => Cow::Borrowed("lb"),
            Unit::Cent => Cow::Borrowed("cent"),
            Unit::Dollar => Cow::Borrowed("$"),
            Unit::KCal => Cow::Borrowed("kcal"),
            Unit::Second => Cow::Borrowed("second"),
            Unit::Minute => Cow::Borrowed("minute"),
            Unit::Hour => Cow::Borrowed("hour"),
            Unit::Day => Cow::Borrowed("day"),
            Unit::Fahrenheit => Cow::Borrowed("fahrenheit"),
            Unit::Celsius => Cow::Borrowed("celsius"),
            Unit::Inch => Cow::Borrowed("\""),
            Unit::Whole => Cow::Borrowed("whole"),
            Unit::Other(s) => Cow::Owned(singular(s).into_owned()),
        }
    }

    /// The measurement category this unit belongs to. A pure function of the
    /// unit (no value needed), so the graph viz and other unit-only callers can
    /// classify a node without constructing a `Measure`. `Measure::kind`
    /// delegates here.
    pub fn kind(&self) -> MeasureKind {
        match self {
            // Weight units
            Unit::Gram | Unit::Kilogram | Unit::Ounce | Unit::Pound => MeasureKind::Weight,

            // Volume units
            Unit::Milliliter
            | Unit::Liter
            | Unit::Teaspoon
            | Unit::Tablespoon
            | Unit::Cup
            | Unit::Quart
            | Unit::FluidOunce => MeasureKind::Volume,

            // Money units
            Unit::Cent | Unit::Dollar => MeasureKind::Money,

            // Time units
            Unit::Second | Unit::Minute | Unit::Hour | Unit::Day => MeasureKind::Time,

            // Temperature units
            Unit::Fahrenheit | Unit::Celsius => MeasureKind::Temperature,

            // Energy units
            Unit::KCal => MeasureKind::Calories,

            // Length units
            Unit::Inch => MeasureKind::Length,

            // Other/custom units
            Unit::Whole => MeasureKind::Other("whole".to_string()),
            Unit::Other(s) => {
                // Nutrient unit pattern like "g protein", "mg sodium", "ug vitamin_b12"
                if is_nutrient_unit(s) {
                    MeasureKind::Nutrient(s.clone())
                } else {
                    MeasureKind::Other(s.clone())
                }
            }
        }
    }
}

static UNIT_MAPPINGS: &[(&str, Unit)] = &[
    ("g", Unit::Gram),
    ("gram", Unit::Gram),
    ("kg", Unit::Kilogram),
    ("kilogram", Unit::Kilogram),
    ("l", Unit::Liter),
    ("liter", Unit::Liter),
    ("ml", Unit::Milliliter),
    ("milliliter", Unit::Milliliter),
    ("tsp", Unit::Teaspoon),
    ("teaspoon", Unit::Teaspoon),
    ("tbsp", Unit::Tablespoon),
    ("tablespoon", Unit::Tablespoon),
    ("cup", Unit::Cup),
    ("c", Unit::Cup),
    ("quart", Unit::Quart),
    ("q", Unit::Quart),
    ("fl oz", Unit::FluidOunce),
    ("fluid oz", Unit::FluidOunce),
    ("oz", Unit::Ounce),
    ("ounce", Unit::Ounce),
    ("lb", Unit::Pound),
    ("pound", Unit::Pound),
    ("cent", Unit::Cent),
    ("$", Unit::Dollar),
    ("dollar", Unit::Dollar),
    ("kcal", Unit::KCal),
    ("calorie", Unit::KCal),
    ("cal", Unit::KCal),
    // time
    ("second", Unit::Second),
    ("sec", Unit::Second),
    ("s", Unit::Second),
    ("minute", Unit::Minute),
    ("min", Unit::Minute),
    ("hour", Unit::Hour),
    ("hr", Unit::Hour),
    ("day", Unit::Day),
    // temperature
    ("fahrenheit", Unit::Fahrenheit),
    ("f", Unit::Fahrenheit),
    ("°", Unit::Fahrenheit),
    ("°f", Unit::Fahrenheit),
    ("degrees", Unit::Fahrenheit),
    ("celsius", Unit::Celsius),
    ("celcius", Unit::Celsius),
    ("°c", Unit::Celsius),
    ("\"", Unit::Inch),
    //distance
    ("inch", Unit::Inch),
    ("whole", Unit::Whole),
    ("each", Unit::Whole),
];

/// O(1) lookup from string to Unit
static UNIT_MAP: LazyLock<HashMap<&'static str, Unit>> = LazyLock::new(|| {
    UNIT_MAPPINGS
        .iter()
        .map(|&(s, ref u)| (s, u.clone()))
        .collect()
});

impl FromStr for Unit {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // `singular` lowercases internally, borrowing without allocation when the
        // input is already lowercase ASCII (the common case off a recipe line),
        // so there's no need for an unconditional `to_lowercase()` here.
        let s_norm = singular(s);
        // O(1) lookup using HashMap
        if let Some(unit) = UNIT_MAP.get(&*s_norm) {
            return Ok(unit.clone());
        }
        Ok(Unit::Other(s.to_string()))
    }
}
impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

/// Strip a plural suffix from an (already-singular-or-plural) unit word.
/// "-es" after a sibilant ("bunches", "boxes", "dishes") strips to the base;
/// otherwise a bare trailing "s" is stripped ("cups", "slices", "recipes").
fn strip_plural(s: &str) -> &str {
    if let Some(base) = s.strip_suffix("es") {
        if base.ends_with("ch")
            || base.ends_with("sh")
            || base.ends_with("ss")
            || base.ends_with('x')
            || base.ends_with('z')
        {
            return base;
        }
    }
    s.strip_suffix('s').unwrap_or(s)
}

/// Lowercase + strip a plural suffix from a unit word ("Scoops" -> "scoop",
/// "pouches" -> "pouch"). Public because downstream boundary code (e.g.
/// recipebridge's bare-count serving guard) must relabel units with exactly
/// the form this parser would read off a recipe line — reimplementing the
/// rule there would drift.
pub fn singular(s: &str) -> Cow<'_, str> {
    // Fast path: if already lowercase ASCII, borrow directly
    if s.bytes().all(|b| !b.is_ascii_uppercase()) {
        Cow::Borrowed(strip_plural(s))
    } else {
        // Slow path: needs lowercasing
        let lowered = s.to_lowercase();
        Cow::Owned(strip_plural(&lowered).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singular_plural_suffixes() {
        // Bare trailing "s"
        assert_eq!(singular("cups"), "cup");
        assert_eq!(singular("slices"), "slice");
        assert_eq!(singular("recipes"), "recipe");
        // Sibilant "-es" plurals strip the whole suffix ("bunche" was wrong)
        assert_eq!(singular("bunches"), "bunch");
        assert_eq!(singular("pinches"), "pinch");
        assert_eq!(singular("boxes"), "box");
        // Already singular stays put
        assert_eq!(singular("bunch"), "bunch");
        assert_eq!(singular("box"), "box");
    }

    #[test]
    fn test_is_addon_unit() {
        let custom_units: HashSet<String> =
            HashSet::from(["packet".to_string(), "slice".to_string()]);

        assert!(is_addon_unit(&custom_units, "packet"));
        assert!(is_addon_unit(&custom_units, "slice"));
        assert!(is_addon_unit(&custom_units, "PACKET")); // Case insensitive
        assert!(!is_addon_unit(&custom_units, "cup"));
        assert!(!is_addon_unit(&custom_units, "unknown"));
    }
}
