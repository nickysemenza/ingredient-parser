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
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "weight" => Self::Weight,
            "volume" => Self::Volume,
            "money" => Self::Money,
            "calories" => Self::Calories,
            "time" => Self::Time,
            "temperature" => Self::Temperature,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::unit::{MeasureKind, Unit};

    #[test]
    fn test_kind() {
        assert_eq!(Unit::from_str("g"), MeasureKind::from_str("weight").unit());
        assert_eq!(Unit::from_str("ml"), MeasureKind::from_str("volume").unit());
        assert_eq!(
            Unit::from_str("cent"),
            MeasureKind::from_str("money").unit()
        );
        assert_eq!(
            Unit::from_str("cal"),
            MeasureKind::from_str("calories").unit()
        );
        assert_eq!(
            Unit::from_str("second"),
            MeasureKind::from_str("time").unit()
        );
        assert_eq!(
            Unit::from_str("°"),
            MeasureKind::from_str("temperature").unit()
        );
        assert_eq!(
            Unit::from_str("").normalize(),
            MeasureKind::from_str("foo").unit()
        );
    }
}
