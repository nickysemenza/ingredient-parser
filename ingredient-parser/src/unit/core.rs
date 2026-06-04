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
    pub fn normalize(self) -> Unit {
        match self {
            // `from_str` lower-cases/singularizes for lookup and promotes known
            // aliases; it only ever returns `Other` for truly-unknown units (and
            // never `Err`), in which case we canonicalize the stored text.
            Unit::Other(x) => match Unit::from_str(&x) {
                Ok(Unit::Other(_)) | Err(()) => Unit::Other(singular(&x).into_owned()),
                Ok(known) => known,
            },
            canonical => canonical,
        }
    }
    // TODO(perf, deferred from todo 013 item 6): built-in variants could return a
    // `&'static str` (e.g. `as_static_str()`), allocating only for `Other(s)`. This
    // is on the Display/output path, not the parse hot path — deferred to avoid any
    // risk to Display behavior until it shows up in a profile.
    pub fn to_str(&self) -> String {
        match self {
            Unit::Gram => "g".to_string(),
            Unit::Kilogram => "kg".to_string(),
            Unit::Liter => "l".to_string(),
            Unit::Milliliter => "ml".to_string(),
            Unit::Teaspoon => "tsp".to_string(),
            Unit::Tablespoon => "tbsp".to_string(),
            Unit::Cup => "cup".to_string(),
            Unit::Quart => "quart".to_string(),
            Unit::FluidOunce => "fl oz".to_string(),
            Unit::Ounce => "oz".to_string(),
            Unit::Pound => "lb".to_string(),
            Unit::Cent => "cent".to_string(),
            Unit::Dollar => "$".to_string(),
            Unit::KCal => "kcal".to_string(),
            Unit::Second => "second".to_string(),
            Unit::Minute => "minute".to_string(),
            Unit::Hour => "hour".to_string(),
            Unit::Day => "day".to_string(),
            Unit::Fahrenheit => "fahrenheit".to_string(),
            Unit::Celsius => "celsius".to_string(),
            Unit::Inch => "\"".to_string(),
            Unit::Whole => "whole".to_string(),
            Unit::Other(s) => singular(s).into_owned(),
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
        let lowered = s.to_lowercase();
        let s_norm = singular(&lowered);
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

pub(crate) fn singular(s: &str) -> Cow<'_, str> {
    // Fast path: if already lowercase ASCII with no trailing 's', borrow directly
    if s.bytes().all(|b| !b.is_ascii_uppercase()) {
        match s.strip_suffix('s') {
            Some(stripped) => Cow::Borrowed(stripped),
            None => Cow::Borrowed(s),
        }
    } else {
        // Slow path: needs lowercasing
        let lowered = s.to_lowercase();
        match lowered.strip_suffix('s') {
            Some(stripped) => Cow::Owned(stripped.to_string()),
            None => Cow::Owned(lowered),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
