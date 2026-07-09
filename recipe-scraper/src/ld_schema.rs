use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Wrapper for recipeYield which can be a string, number, or array
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RecipeYieldWrapper {
    /// Single string like "4 servings" or "12 cookies"
    String(String),
    /// Single number like 4
    Number(f64),
    /// Array of strings like ["4 servings", "12 pancakes"]
    StringArray(Vec<String>),
    /// Array of numbers
    NumberArray(Vec<f64>),
    /// Any other shape (e.g. a structured `QuantitativeValue` object). The
    /// catch-all keeps this untagged enum **total** so a surprising `recipeYield`
    /// shape never fails `RootRecipe` deserialization — which would silently
    /// regress an otherwise-good LD+JSON recipe into the weaker HTML fallback.
    /// Mirrors [`StringOrList::Other`].
    Other(Value),
}

/// A schema.org value that may be a single string, a list of strings, or some
/// other shape entirely. The `Other(Value)` catch-all keeps the untagged enum
/// **total** so a surprising shape (a number, an object) never fails `RootRecipe`
/// deserialization — which would silently regress a working site into the HTML
/// fallback. Used for the soft metadata fields (description, category, …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrList {
    String(String),
    List(Vec<String>),
    Other(Value),
}

impl StringOrList {
    /// The first usable string: the value itself, or the first list element.
    pub fn first_string(&self) -> Option<String> {
        self.first_str().map(str::to_owned)
    }

    /// Borrowing variant of [`first_string`](Self::first_string) for callers
    /// that only need to read the value (e.g. parse it) without owning it.
    pub fn first_str(&self) -> Option<&str> {
        match self {
            StringOrList::String(s) => Some(s.as_str()),
            StringOrList::List(l) => l.first().map(String::as_str),
            StringOrList::Other(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootRecipe {
    #[serde(rename = "@context")]
    pub context: Option<String>,
    // #[serde(rename = "@type")]
    // pub type_field: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<StringOrList>,
    // pub author: Author,
    pub image: Option<ImageOrList>,
    #[serde(default)]
    pub total_time: Option<StringOrList>,
    #[serde(default)]
    pub prep_time: Option<StringOrList>,
    #[serde(default)]
    pub cook_time: Option<StringOrList>,
    pub recipe_yield: Option<RecipeYieldWrapper>,
    // pub recipe_cuisine: String,
    #[serde(default)]
    pub recipe_category: Option<StringOrList>,
    // pub keywords: String,
    // pub aggregate_rating: AggregateRating,
    /// schema.org HowTo `tool`; items are bare strings or `{ "name": ... }`
    /// objects, so we keep it a raw `Value` and extract names in `ld_json`.
    #[serde(default)]
    pub tool: Option<Value>,
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
    /// Optional: schema.org marks `name` optional on `HowToSection`. A nameless
    /// section must still deserialize as `B` (keeping its `item_list_element`
    /// steps) rather than silently falling through the untagged enum to
    /// `Wrapper(ItemListElement)`, which would drop every step.
    #[serde(default)]
    pub name: Option<String>,
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
    // Boxed: RootRecipe is large; keeps the enum's variants similarly sized.
    Recipe(Box<RootRecipe>),
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
/// One `@graph` node: a recipe, or anything else. Untagged, so every non-recipe
/// node (Article, WebPage, ImageObject, …) lands in `Other` — listing them as
/// separate `Value` variants made everything after the first unreachable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Graph {
    // Boxed: RootRecipe is much larger than the catch-all variant.
    Recipe(Box<RootRecipe>),
    Other(Value),
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
