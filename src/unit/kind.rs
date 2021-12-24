#[derive(Clone, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
pub enum MeasureKind {
    Weight,
    Volume,
    Money,
    Calories,
    Other,
    Time,
}
impl MeasureKind {
    pub fn from_str(s: &str) -> Self {
        match s {
            "weight" => Self::Weight,
            "volume" => Self::Volume,
            "money" => Self::Money,
            "calories" => Self::Calories,
            "time" => Self::Time,
            _ => Self::Other,
        }
    }
}
