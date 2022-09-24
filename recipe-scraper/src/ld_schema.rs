use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootRecipe {
    #[serde(rename = "@context")]
    pub context: Option<String>,
    // #[serde(rename = "@type")]
    // pub type_field: String,
    pub name: String,
    // pub description: String,
    // pub author: Author,
    pub image: Option<ImageOrList>,
    // pub total_time: String,
    // pub recipe_yield: String,
    // pub recipe_cuisine: String,
    // pub recipe_category: String,
    // pub keywords: String,
    // pub aggregate_rating: AggregateRating,
    pub recipe_ingredient: Vec<String>,
    pub recipe_instructions: RecipeInstructionFOO,
    // pub is_accessible_for_free: String,
    // pub has_part: HasPart,
    // pub publisher: Publisher,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeInstructionA {
    #[serde(rename = "@context")]
    pub context: Option<String>,
    #[serde(rename = "@type")]
    pub type_field: String,
    pub text: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeInstructionB {
    #[serde(rename = "@type")]
    pub type_field: String,
    pub name: String,
    pub item_list_element: Vec<ItemListElement>,
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
pub struct Image {
    pub url: String,
    pub height: Option<i64>,
    pub width: Option<i64>,
    #[serde(rename = "@context")]
    pub context: Option<String>,
    #[serde(rename = "@type")]
    pub type_field: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RecipeInstruction {
    A(RecipeInstructionA),
    B(RecipeInstructionB),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RecipeInstructionFOO {
    A(Vec<RecipeInstructionA>),
    B(Vec<RecipeInstructionB>),
    C(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImageOrList {
    URL(String),
    List(Vec<Image>),
    URLList(Vec<String>),
    Image(Image),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Root {
    Graph(RootGraph),
    Recipe(RootRecipe),
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootGraph {
    #[serde(rename = "@context")]
    pub context: String,
    #[serde(rename = "@graph")]
    pub graph: Vec<Graph>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "@type")]
pub enum Graph {
    Article(Value),
    WebPage(Value),
    ImageObject(Image),
    BreadcrumbList(Value),
    WebSite(Value),
    Organization(Value),
    Person(Value),
    Recipe(RootRecipe),
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph2 {
    #[serde(rename = "@type")]
    pub type_field: String,
    #[serde(rename = "@id")]
    pub id: String,
    pub is_part_of: Option<IsPartOf>,
    pub author: Option<Author>,
    pub headline: Option<String>,
    pub date_published: Option<String>,
    pub date_modified: Option<String>,
    pub word_count: Option<i64>,
    pub comment_count: Option<i64>,
    // pub publisher: Option<Publisher>,
    pub image: serde_json::Value,
    pub thumbnail_url: Option<String>,
    pub article_section: Option<Vec<String>>,
    pub in_language: Option<String>,
    pub url: Option<String>,
    pub name: Option<String>,
    pub primary_image_of_page: Option<PrimaryImageOfPage>,
    pub description: Option<String>,
    pub breadcrumb: Option<Breadcrumb>,
    pub content_url: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub caption: Option<String>,
    pub item_list_element: Option<Vec<ItemListElement>>,
    #[serde(default)]
    pub same_as: Vec<String>,
    pub logo: Option<Logo>,
    #[serde(rename = "@context")]
    pub context: Option<String>,
    #[serde(default)]
    pub recipe_yield: Vec<String>,
    pub prep_time: Option<String>,
    pub cook_time: Option<String>,
    pub total_time: Option<String>,
    #[serde(default)]
    pub recipe_ingredient: Vec<String>,
    pub recipe_instructions: Option<Vec<RecipeInstructionC>>,
    pub aggregate_rating: Option<AggregateRating>,
    #[serde(default)]
    pub recipe_category: Vec<String>,
    #[serde(default)]
    pub recipe_cuisine: Vec<String>,
    pub keywords: Option<String>,
    pub nutrition: Option<Nutrition>,
    pub main_entity_of_page: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IsPartOf {
    #[serde(rename = "@id")]
    pub id: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    #[serde(rename = "@type")]
    pub type_field: Option<String>,
    pub name: String,
    #[serde(rename = "@id")]
    pub id: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryImageOfPage {
    #[serde(rename = "@id")]
    pub id: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Breadcrumb {
    #[serde(rename = "@id")]
    pub id: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemListElement {
    #[serde(rename = "@type")]
    pub type_field: Option<String>,
    pub position: Option<i64>,
    pub name: Option<String>,
    pub item: Option<String>,
    pub text: Option<String>,
    pub url: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logo {
    #[serde(rename = "@type")]
    pub type_field: String,
    pub in_language: String,
    #[serde(rename = "@id")]
    pub id: String,
    pub url: String,
    pub content_url: String,
    pub width: i64,
    pub height: i64,
    pub caption: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeInstructionC {
    #[serde(rename = "@type")]
    pub type_field: String,
    pub text: String,
    pub name: String,
    pub url: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateRating {
    #[serde(rename = "@type")]
    pub type_field: String,
    pub rating_value: String,
    pub rating_count: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nutrition {
    #[serde(rename = "@type")]
    pub type_field: String,
    pub calories: String,
    pub carbohydrate_content: String,
    pub protein_content: String,
    pub fat_content: String,
    pub saturated_fat_content: String,
    pub cholesterol_content: String,
    pub sodium_content: String,
    pub fiber_content: String,
    pub sugar_content: String,
    pub trans_fat_content: String,
    pub unsaturated_fat_content: String,
    pub serving_size: String,
}

#[cfg(test)]
mod tests {
    use super::RootGraph;

    #[test]
    fn it_works_file() {
        let _v: RootGraph = serde_json::from_str(include_str!(
            "../test_data/thewoksoflife_vietnamese-rice-noodle-salad-chicken.partial.json"
        ))
        .unwrap();

        let _v: RootGraph = serde_json::from_str(include_str!(
            "../test_data/kingarthurbaking_pretzel-focaccia-recipe.json"
        ))
        .unwrap();
    }
}
