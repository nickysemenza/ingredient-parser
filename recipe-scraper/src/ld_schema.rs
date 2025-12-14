use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "recipe")]
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
    pub recipe_instructions: InstructionWrapper,
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
pub struct Image {
    pub url: String,
    // todo: dims are sometimes string, sometimes int
    // nytimes_1022674-chewy-gingerbread-cookies.json is string, others are int.
    // pub height: Option<i64>,
    // pub width: Option<i64>,
    #[serde(rename = "@context")]
    pub context: Option<String>,
    #[serde(rename = "@type")]
    pub type_field: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InstructionWrapper {
    A(Vec<RecipeInstructionA>),
    B(Vec<BOrWrapper>),
    C(String),
    D(Vec<Vec<RecipeInstructionA>>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BOrWrapper {
    B(RecipeInstructionB),
    Wrapper(ItemListElement),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ImageOrList {
    Url(String),
    List(Vec<Image>),
    UrlList(Vec<String>),
    Image(Image),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Root {
    Graph(RootGraph),
    Recipe(RootRecipe),
    List(Vec<RootRecipe>),
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
// #[serde(tag = "@type")]
#[serde(untagged)]
pub enum Graph {
    Recipe(RootRecipe),
    Article(Value),
    WebPage(Value),
    ImageObject(Image),
    BreadcrumbList(Value),
    WebSite(Value),
    Organization(Value),
    Person(Value),
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{RootGraph, RootRecipe};

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

        let _v: RootRecipe = serde_json::from_str(include_str!(
            "../test_data/food52_85952-nectarine-crumble-recipe.json"
        ))
        .unwrap();

        let _v: RootGraph = serde_json::from_str(include_str!(
            "../test_data/omnivorescookbook_sichuan-shrimp-stir-fry.json"
        ))
        .unwrap();
        let _v: RootRecipe = serde_json::from_str(include_str!(
            "../test_data/nytimes_1022674-chewy-gingerbread-cookies.json"
        ))
        .unwrap();
    }
}
