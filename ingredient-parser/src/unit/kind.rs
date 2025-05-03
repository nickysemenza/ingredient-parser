use std::{fmt, str::FromStr};

use super::Unit;

#[derive(Clone, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
pub enum MeasureKind {
    Weight,
    Volume,
    Money,
    Calories,
    Other, //todo: make this hold a string
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
            MeasureKind::Other => Unit::Other("".to_string()),
            MeasureKind::Time => Unit::Second,
            MeasureKind::Temperature => Unit::Farhenheit,
            MeasureKind::Length => Unit::Inch,
        }
    }
    pub fn to_str(&self) -> &str {
        match self {
            MeasureKind::Weight => "weight",
            MeasureKind::Volume => "volume",
            MeasureKind::Money => "money",
            MeasureKind::Calories => "calories",
            MeasureKind::Other => "other",
            MeasureKind::Time => "time",
            MeasureKind::Temperature => "temperature",
            MeasureKind::Length => "length",
        }
    }
}

impl fmt::Display for MeasureKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl FromStr for MeasureKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "weight" => Self::Weight,
            "volume" => Self::Volume,
            "money" => Self::Money,
            "calories" => Self::Calories,
            "time" => Self::Time,
            "temperature" => Self::Temperature,
            "length" => Self::Length,
            _ => Self::Other,
        })
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
            Unit::from_str("").unwrap().normalize(),
            MeasureKind::from_str("foo").unwrap().unit()
        );
    }
}
