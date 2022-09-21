use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    #[serde(rename = "@context")]
    pub context: String,
    // #[serde(rename = "@type")]
    // pub type_field: String,
    // pub name: String,
    // pub description: String,
    // pub author: Author,
    // pub image: String,
    // pub total_time: String,
    // pub recipe_yield: String,
    // pub recipe_cuisine: String,
    // pub recipe_category: String,
    // pub keywords: String,
    // pub aggregate_rating: AggregateRating,
    pub recipe_ingredient: Vec<String>,
    pub recipe_instructions: Vec<RecipeInstruction>,
    // pub is_accessible_for_free: String,
    // pub has_part: HasPart,
    // pub publisher: Publisher,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    #[serde(rename = "@type")]
    pub type_field: String,
    pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateRating {
    #[serde(rename = "@type")]
    pub type_field: String,
    pub rating_value: i64,
    pub rating_count: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeInstruction {
    #[serde(rename = "@context")]
    pub context: Option<String>,
    #[serde(rename = "@type")]
    pub type_field: String,
    pub text: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HasPart {
    #[serde(rename = "@type")]
    pub type_field: String,
    pub is_accessible_for_free: String,
    pub css_selector: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Publisher {
    #[serde(rename = "@context")]
    pub context: String,
    #[serde(rename = "@type")]
    pub type_field: String,
    pub name: String,
    pub url: String,
    pub alternate_name: Vec<String>,
    pub image: Vec<Image>,
    pub same_as: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub url: String,
    pub height: i64,
    pub width: i64,
    #[serde(rename = "@context")]
    pub context: Option<String>,
    #[serde(rename = "@type")]
    pub type_field: String,
}
