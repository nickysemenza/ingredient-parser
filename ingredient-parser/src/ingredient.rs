use std::{convert::TryFrom, fmt};

use crate::{from_str, unit::Measure};

#[cfg_attr(feature = "serde-derive", derive(Serialize, Deserialize))]
#[derive(Clone, PartialEq, PartialOrd, Debug, Default)]
/// Holds a name, list of [Measure], and optional modifier string
pub struct Ingredient {
    pub name: String,
    pub amounts: Vec<Measure>,
    pub modifier: Option<String>,
}

impl TryFrom<&str> for Ingredient {
    type Error = String;
    fn try_from(value: &str) -> Result<Ingredient, Self::Error> {
        Ok(from_str(value))
    }
}

impl fmt::Display for Ingredient {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let amounts: Vec<String> = self.amounts.iter().map(|id| id.to_string()).collect();
        let modifier = self
            .modifier
            .as_ref()
            .map_or_else(String::new, |m| format!(", {m}"));

        let amount_list = if amounts.is_empty() {
            "n/a ".to_string()
        } else {
            format!("{} ", amounts.join(" / "))
        };

        write!(f, "{}{}{}", amount_list, self.name, modifier)
    }
}
