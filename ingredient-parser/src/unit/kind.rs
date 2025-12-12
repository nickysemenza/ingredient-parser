use std::{fmt, str::FromStr};

use super::Unit;

#[derive(Clone, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
pub enum MeasureKind {
    Weight,
    Volume,
    Money,
    Calories,
    Other(String),
    Time,
    Temperature,
    Length,
    /// Target a specific nutrient unit like "g protein", "mg sodium"
    /// Used for direct conversions: "2 cups flour" → "X g protein"
    Nutrient(String),
}
impl MeasureKind {
    pub fn unit(&self) -> Unit {
        match self {
            MeasureKind::Weight => Unit::Gram,
            MeasureKind::Volume => Unit::Milliliter,
            MeasureKind::Money => Unit::Cent,
            MeasureKind::Calories => Unit::KCal,
            MeasureKind::Other(s) => Unit::Other(s.clone()),
            MeasureKind::Time => Unit::Second,
            MeasureKind::Temperature => Unit::Fahrenheit,
            MeasureKind::Length => Unit::Inch,
            MeasureKind::Nutrient(s) => Unit::Other(s.clone()),
        }
    }
    pub fn to_str(&self) -> &str {
        for (s, kind) in MEASURE_KIND_MAPPINGS {
            if self == kind {
                return s;
            }
        }
        "other"
    }
}

impl fmt::Display for MeasureKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

static MEASURE_KIND_MAPPINGS: &[(&str, MeasureKind)] = &[
    ("weight", MeasureKind::Weight),
    ("volume", MeasureKind::Volume),
    ("money", MeasureKind::Money),
    ("calories", MeasureKind::Calories),
    ("time", MeasureKind::Time),
    ("temperature", MeasureKind::Temperature),
    ("length", MeasureKind::Length),
];

impl FromStr for MeasureKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s_norm = s.to_lowercase();

        // Check for nutrient prefix pattern: "nutrient:g protein"
        if let Some(nutrient_unit) = s_norm.strip_prefix("nutrient:") {
            return Ok(MeasureKind::Nutrient(nutrient_unit.to_string()));
        }

        for (str_repr, kind) in MEASURE_KIND_MAPPINGS {
            if s_norm == *str_repr {
                return Ok(kind.clone());
            }
        }
        Ok(MeasureKind::Other(s.to_string()))
    }
}
