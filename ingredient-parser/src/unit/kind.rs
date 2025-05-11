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
            MeasureKind::Temperature => Unit::Farhenheit,
            MeasureKind::Length => Unit::Inch,
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
        for (str_repr, kind) in MEASURE_KIND_MAPPINGS {
            if s_norm == *str_repr {
                return Ok(kind.clone());
            }
        }
        Ok(MeasureKind::Other(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::unit::{MeasureKind, Unit};

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
}
