use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

pub fn is_valid(units: HashSet<String>, s: &str) -> bool {
    if !matches!(Unit::from_str(&singular(s)).unwrap(), Unit::Other(_)) {
        // anything other than `other`
        return true;
    }
    is_addon_unit(units, s)
}

pub fn is_addon_unit(units: HashSet<String>, s: &str) -> bool {
    units.contains(&s.to_lowercase())
}

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
    Celcius,
    //distance
    Inch,
    Whole,
    // https://stackoverflow.com/a/77723851
    #[serde(untagged)]
    Other(String),
}

impl Unit {
    pub fn normalize(self) -> Unit {
        //todo
        match self {
            Unit::Other(x) => Unit::Other(singular(&x)),
            _ => self,
        }
    }
    pub fn to_str(&self) -> String {
        for (s, unit) in UNIT_MAPPINGS {
            if self == unit {
                return s.to_string();
            }
        }
        match self {
            Unit::Other(s) => singular(s),
            _ => unreachable!("Unit not found in mapping"),
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
    ("celcius", Unit::Celcius),
    ("°c", Unit::Celcius),
    ("\"", Unit::Inch),
    //distance
    ("inch", Unit::Inch),
    ("whole", Unit::Whole),
    ("each", Unit::Whole),
];

impl FromStr for Unit {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s_norm = singular(&s.to_lowercase());
        for (str_repr, unit) in UNIT_MAPPINGS {
            if s_norm == *str_repr {
                return Ok(unit.clone());
            }
        }
        Ok(Unit::Other(s.to_string()))
    }
}
impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

pub fn singular(s: &str) -> String {
    let s2 = s.to_lowercase();
    s2.strip_suffix('s').unwrap_or(&s2).to_string()
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn test_is_unit() {
        assert!(is_valid(HashSet::from([]), "oz"));
        assert!(is_valid(HashSet::from([]), "fl oz"));
        assert!(!is_valid(HashSet::from([]), "slice"));
        assert!(is_valid(HashSet::from(["slice".to_string()]), "slice"),);
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
}
