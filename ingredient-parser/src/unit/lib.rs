use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

pub fn is_valid(units: HashSet<String>, s: &str) -> bool {
    // Unit::from_str always returns Ok - check if it's a known unit (not Other)
    if !matches!(Unit::from_str(&singular(s)), Ok(Unit::Other(_))) {
        // anything other than `other`
        return true;
    }
    is_addon_unit(units, s)
}

pub(crate) fn is_addon_unit(units: HashSet<String>, s: &str) -> bool {
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

pub(crate) fn singular(s: &str) -> String {
    let s2 = s.to_lowercase();
    s2.strip_suffix('s').unwrap_or(&s2).to_string()
}
