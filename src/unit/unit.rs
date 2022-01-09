use std::collections::HashSet;
use std::fmt;
use std::iter::FromIterator;

pub fn is_valid(units: Vec<String>, s: &str) -> bool {
    if !matches!(Unit::from_str(&singular(s)), Unit::Other(_)) {
        // anything other than `other`
        return true;
    }

    let m: HashSet<String> = HashSet::from_iter(units.iter().cloned());
    return m.contains(&s.to_lowercase());
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
    Farhenheit,
    Celcius,

    Other(String),
}

impl Unit {
    pub fn normalize(self) -> Unit {
        //todo
        match self {
            Unit::Other(x) => return Unit::Other(singular(&x)),
            _ => return self,
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "gram" | "g" => Self::Gram,
            "kilogram" | "kg" => Self::Kilogram,

            "oz" | "ounce" => Self::Ounce,
            "lb" | "pound" => Self::Pound,

            "ml" | "milliliter" => Self::Milliliter,
            "l" | "liter" => Self::Liter,

            "tsp" | "teaspoon" => Self::Teaspoon,
            "tbsp" | "tablespoon" => Self::Tablespoon,
            "c" | "cup" => Self::Cup,
            "q" | "quart" => Self::Quart,
            "fl oz" | "fluid oz" => Self::FluidOunce,

            "dollar" | "$" => Self::Dollar,
            "cent" => Self::Cent,

            "calorie" | "cal" | "kcal" => Self::KCal,
            "second" | "sec" | "s" => Self::Second,
            "minute" | "min" => Self::Minute,
            "hour" | "hr" => Self::Hour,
            "day" => Self::Day,

            "fahrenheit" | "f" | "°" | "°f" => Self::Farhenheit,
            "celcius" | "°c" => Self::Celcius,

            _ => Self::Other(s.to_string()),
        }
    }
    pub fn to_str(self) -> String {
        match self {
            Unit::Gram => "g",
            Unit::Kilogram => "kg",
            Unit::Liter => "l",
            Unit::Milliliter => "ml",
            Unit::Teaspoon => "tsp",
            Unit::Tablespoon => "tbsp",
            Unit::Cup => "cup",
            Unit::Quart => "quart",
            Unit::FluidOunce => "fl oz",
            Unit::Ounce => "oz",
            Unit::Pound => "lb",
            Unit::Cent => "cent",
            Unit::Dollar => "$",
            Unit::KCal => "kcal",
            Unit::Day => "day",
            Unit::Hour => "hour",
            Unit::Minute => "minute",
            Unit::Second => "second",
            Unit::Celcius => "°c",
            Unit::Farhenheit => "°F",
            Unit::Other(s) => return singular(&s),
        }
        .to_string()
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub fn singular(s: &str) -> String {
    let s2 = s.to_lowercase();
    s2.strip_suffix("s").unwrap_or(&s2).to_string()
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn test_is_unit() {
        assert_eq!(is_valid(vec![], "oz"), true);
        assert_eq!(is_valid(vec![], "fl oz"), true);
        assert_eq!(is_valid(vec![], "slice"), false);
        assert_eq!(is_valid(vec!["slice".to_string()], "slice"), true);
        assert_eq!(is_valid(vec![], "TABLESPOONS"), true);
        assert_eq!(is_valid(vec![], "foo"), false);
    }
    #[test]
    fn test_back_forth() {
        assert_eq!(Unit::from_str("oz"), Unit::Ounce);
        assert_eq!(Unit::from_str("gram").to_str(), "g");
        assert_eq!(Unit::from_str("foo").to_str(), "foo");
        assert_eq!(format!("{}", Unit::from_str("foo")), "Other(\"foo\")");
    }
}
