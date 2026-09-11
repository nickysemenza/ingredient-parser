//! Deterministic checks on a lowered chunk answer.
//!
//! Hard faults mean the answer is structurally wrong and the model is asked
//! again with the fault as feedback (then the next model in the ladder). Soft
//! flags keep the answer but mark the chunk for a second opinion. Neither uses
//! a model.

use std::fmt;

use crate::chunk::Chunk;
use crate::contract::{Kind, Lowered};
use crate::lines::{BookLines, looks_like_quantity_text};
use crate::report::Flag;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardFault {
    /// A title line sits in a figure or caption.
    TitleIsCaption { local: usize, title: String },
    /// `kind: recipe` with no ingredient lines.
    RecipeWithoutIngredients { title: String },
    /// No items, but the chunk prints a yield and several quantities.
    EmptyWithYield { quantities: usize },
    /// A title line that points at another recipe ("… (this page)", "… page 42").
    TitleIsReference { local: usize, title: String },
    /// A title that is a printed label ("Do Ahead", "Special Equipment: …").
    TitleIsLabel { local: usize, title: String },
    /// A title that is a paragraph, not a name.
    TitleIsProse { local: usize, title: String },
    /// Every "ingredient line" is a sentence: a procedure, not a list.
    IngredientsAreProse { title: String },
    /// A title that names an ingredient group ("For the sauce").
    TitleIsSectionHeading { local: usize, title: String },
}

/// "For the sauce", "For serving", "To finish:" — group headings, not items.
fn is_section_heading(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    (lower.starts_with("for the ")
        || lower.starts_with("for ")
        || lower.starts_with("to serve")
        || lower.starts_with("to finish"))
        && lower.split_whitespace().count() <= 8
}

/// An ingredient line this long is a paragraph, and a shorter one that
/// ends with a period is a sentence.
fn ingredient_line_is_prose(text: &str) -> bool {
    let t = text.trim();
    t.len() > 160 || (t.len() > 100 && t.ends_with('.'))
}

/// Titles longer than this are paragraphs.
const MAX_TITLE_CHARS: usize = 120;

fn is_prose(text: &str) -> bool {
    let t = text.trim();
    t.len() > MAX_TITLE_CHARS || (t.len() > 60 && t.ends_with('.'))
}

/// Words a cross-reference line carries.
const REFERENCE_MARKERS: &[&str] = &[
    "this page",
    "see page",
    "(page ",
    "opposite page",
    "see recipe",
    "recipe follows",
];
/// Labels that head notes and metadata, never items.
pub const LABELS: &[&str] = &[
    "do ahead",
    "do-ahead",
    "make ahead",
    "note",
    "notes",
    "tip",
    "tips",
    "variation",
    "variations",
    "special equipment",
    "equipment",
    "ingredients",
    "method",
    "directions",
    "instructions",
    "serves",
    "makes",
    "yield",
    "yields",
];

fn is_reference_line(text: &str) -> bool {
    let lower = text.to_lowercase();
    REFERENCE_MARKERS.iter().any(|m| lower.contains(m)) || PAGE_NUMBER.is_match(&lower)
}

static PAGE_NUMBER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"\bpage\s*\d{1,4}\b").unwrap_or_else(|e| unreachable!("{e}"))
});

/// `Do Ahead`, `NOTE:`, `Special Equipment: 9-inch pan` — a label, possibly
/// with its payload on the same line.
pub fn is_label(text: &str) -> bool {
    let lower = text.trim().trim_end_matches(':').to_lowercase();
    LABELS.contains(&lower.as_str()) || LABELS.iter().any(|l| lower.starts_with(&format!("{l}:")))
}

impl fmt::Display for HardFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HardFault::TitleIsCaption { local, title } => write!(
                f,
                "line {local} ({title:?}) is a photo caption, not a title; put it in photos or captions and select the item's real title line"
            ),
            HardFault::RecipeWithoutIngredients { title } => write!(
                f,
                "item {title:?} is kind recipe but has no ingredient lines; select its ingredient lines, or mark it technique or essay if the book prints none"
            ),
            HardFault::EmptyWithYield { quantities } => write!(
                f,
                "no items were returned, yet the source prints a serves/makes line and {quantities} quantity lines; extract that recipe"
            ),
            HardFault::TitleIsReference { local, title } => write!(
                f,
                "line {local} ({title:?}) refers to another recipe; it is an ingredient, step, or note of the item it sits in, not a title"
            ),
            HardFault::TitleIsLabel { local, title } => write!(
                f,
                "line {local} ({title:?}) is a printed label, not an item title; put it in notes or equipment with the lines it introduces"
            ),
            HardFault::TitleIsSectionHeading { local, title } => write!(
                f,
                "line {local} ({title:?}) is an ingredient-group heading of the recipe it sits in; make it that recipe's section name, not an item"
            ),
            HardFault::IngredientsAreProse { title } => write!(
                f,
                "item {title:?} lists only paragraphs as ingredient lines; a recipe needs a printed ingredient list, so mark this item technique or essay and put the paragraphs in steps or description"
            ),
            HardFault::TitleIsProse { local, title } => write!(
                f,
                "line {local} ({}…) is a paragraph, not a title; chapter introductions and untitled prose belong in ignored",
                title.chars().take(60).collect::<String>()
            ),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Validation {
    pub hard: Vec<HardFault>,
    pub soft: Vec<Flag>,
}

impl Validation {
    pub fn is_ok(&self) -> bool {
        self.hard.is_empty()
    }

    /// Feedback for the retry request.
    pub fn feedback(&self) -> String {
        self.hard
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Below this share of ingredient lines carrying an amount, a chunk with a
/// real ingredient list is suspect.
const AMOUNT_RATE_FLOOR: f32 = 0.55;
const AMOUNT_RATE_MIN_LINES: usize = 6;
const IGNORED_QUANTITY_LINES: usize = 4;

pub fn validate(chunk: &Chunk, book: &BookLines, lowered: &Lowered) -> Validation {
    let mut v = Validation::default();

    for item in &lowered.items {
        for &line in &item.title_lines {
            if book.lines.get(line).is_some_and(|l| l.clean.in_figure) {
                v.hard.push(HardFault::TitleIsCaption {
                    local: line - chunk.start,
                    title: item.title.clone(),
                });
                v.soft.push(Flag::CaptionAsTitle { line });
            }
        }
        if !item.continues && is_reference_line(&item.title) {
            v.hard.push(HardFault::TitleIsReference {
                local: item
                    .title_lines
                    .first()
                    .map(|l| l - chunk.start)
                    .unwrap_or(0),
                title: item.title.clone(),
            });
        }
        if !item.continues && is_prose(&item.title) {
            v.hard.push(HardFault::TitleIsProse {
                local: item
                    .title_lines
                    .first()
                    .map(|l| l - chunk.start)
                    .unwrap_or(0),
                title: item.title.clone(),
            });
        }
        if !item.continues && is_section_heading(&item.title) {
            v.hard.push(HardFault::TitleIsSectionHeading {
                local: item
                    .title_lines
                    .first()
                    .map(|l| l - chunk.start)
                    .unwrap_or(0),
                title: item.title.clone(),
            });
        }
        if !item.continues && is_label(&item.title) {
            v.hard.push(HardFault::TitleIsLabel {
                local: item
                    .title_lines
                    .first()
                    .map(|l| l - chunk.start)
                    .unwrap_or(0),
                title: item.title.clone(),
            });
        }
        if matches!(item.kind, Kind::Recipe | Kind::Variation)
            && item.ingredient_count() > 0
            && item
                .sections
                .iter()
                .flat_map(|s| s.ingredients.iter())
                .all(|l| ingredient_line_is_prose(&l.text))
        {
            v.hard.push(HardFault::IngredientsAreProse {
                title: item.title.clone(),
            });
        }
        if item.kind == Kind::Recipe && item.ingredient_count() == 0 && !item.continues {
            v.hard.push(HardFault::RecipeWithoutIngredients {
                title: item.title.clone(),
            });
        }
    }

    if lowered.items.is_empty() {
        let has_yield = (chunk.start..chunk.end).any(|i| {
            let upper = book.text(i).trim().to_ascii_uppercase();
            upper.starts_with("SERVES ")
                || upper.starts_with("MAKES ")
                || upper.starts_with("YIELD")
        });
        let quantities = (chunk.start..chunk.end)
            .filter(|&i| looks_like_quantity_text(book.text(i)))
            .count();
        if has_yield && quantities >= 3 {
            v.hard.push(HardFault::EmptyWithYield { quantities });
        }
    }

    // Soft: ingredient lines that mostly fail to parse an amount.
    let ingredient_lines: Vec<&str> = lowered
        .items
        .iter()
        .filter(|i| matches!(i.kind, Kind::Recipe | Kind::Variation))
        .flat_map(|i| i.sections.iter().flat_map(|s| s.ingredients.iter()))
        .map(|t| t.text.as_str())
        .collect();
    if ingredient_lines.len() >= AMOUNT_RATE_MIN_LINES {
        let with_amount = ingredient_lines
            .iter()
            .filter(|t| !ingredient::from_str(t).amounts.is_empty())
            .count();
        let rate = with_amount as f32 / ingredient_lines.len() as f32;
        if rate < AMOUNT_RATE_FLOOR {
            v.soft.push(Flag::LowAmountParseRate {
                rate,
                lines: ingredient_lines.len(),
            });
        }
    }

    if !lowered.auto_ignored.is_empty() {
        v.soft.push(Flag::UnassignedLines {
            count: lowered.auto_ignored.len(),
        });
    }

    // Soft: quantity-like lines the model threw away.
    let ignored_quantities = lowered
        .ignored
        .iter()
        .filter(|&&i| looks_like_quantity_text(book.text(i)))
        .count();
    if ignored_quantities >= IGNORED_QUANTITY_LINES {
        v.soft.push(Flag::IngredientLikeIgnored {
            count: ignored_quantities,
        });
    }

    // Soft: a recipe with ingredients but no method while long prose was
    // ignored. The last item in a chunk is exempt: its method may follow in
    // the next chunk.
    let long_ignored = lowered
        .ignored
        .iter()
        .filter(|&&i| book.text(i).len() > 100)
        .count();
    let last = lowered.items.len().saturating_sub(1);
    for (index, item) in lowered.items.iter().enumerate() {
        if item.kind == Kind::Recipe
            && item.ingredient_count() > 0
            && item.step_count() == 0
            && index != last
            && long_ignored >= 2
        {
            v.soft.push(Flag::RecipeWithoutSteps {
                title: item.title.clone(),
            });
        }
    }
    v
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::chunk::Boundary;
    use crate::contract::lower;
    use crate::epub::nav::Nav;
    use crate::epub::open::SpineDoc;
    use serde_json::{Value, json};

    fn book_and_chunk(html: &str) -> (BookLines, Chunk) {
        let doc = SpineDoc {
            index: 0,
            path: "c.xhtml".into(),
            xhtml: format!("<html><body>{html}</body></html>"),
        };
        let book = BookLines::build(&[doc], &Nav::default());
        let chunk = Chunk {
            id: "k000".into(),
            index: 0,
            start: 0,
            end: book.len(),
            chars: 0,
            title_hint: None,
            boundary: Boundary::Start,
        };
        (book, chunk)
    }

    fn run(html: &str, payload: Value) -> Validation {
        let (book, chunk) = book_and_chunk(html);
        let lowered = lower(&chunk, &book, payload).unwrap();
        validate(&chunk, &book, &lowered)
    }

    #[test]
    fn caption_titles_are_hard_faults() {
        let v = run(
            "<div class=\"cap\"><p>Sour Cherry Pie, this page</p></div><p class=\"rt\">Mousse Pie</p><p>2 cups cream</p><p>Whisk.</p>",
            json!({"items":[{"title":[0],"sections":[{"ingredients":[2],"steps":[3]}]}],"ignored":[1]}),
        );
        assert_eq!(
            v.hard[0],
            HardFault::TitleIsCaption {
                local: 0,
                title: "Sour Cherry Pie, this page".into()
            }
        );
        assert!(
            v.hard
                .iter()
                .any(|f| matches!(f, HardFault::TitleIsReference { .. })),
            "a caption naming a page is also a reference line"
        );
        assert!(v.feedback().contains("photo caption"));
        assert_eq!(v.soft, [Flag::CaptionAsTitle { line: 0 }]);
    }

    #[test]
    fn reference_lines_and_labels_are_not_titles() {
        let html = "<p>Tomato Tart</p><p>2 cups flour</p><p>Flaky All-Butter Pie Dough (this page) ①</p><p>8 ounces feta</p><p>Bake it.</p><p>Do Ahead</p><p>Keeps two days.</p>";
        let v = run(
            html,
            json!({"items":[
            {"title":[0],"sections":[{"ingredients":[1]}]},
            {"title":[2],"sections":[{"ingredients":[3],"steps":[4]}]},
            {"kind":"essay","title":[5],"description":[6]}]}),
        );
        assert!(
            matches!(v.hard[0], HardFault::TitleIsReference { local: 2, .. }),
            "{:?}",
            v.hard
        );
        assert!(
            matches!(v.hard[1], HardFault::TitleIsLabel { local: 5, .. }),
            "{:?}",
            v.hard
        );
        assert!(is_section_heading("FOR PEPPERONI PIE"));
        assert!(is_section_heading("For the green goddess butter"));
        assert!(!is_section_heading(
            "For All the Tea in China: a long essay title about trade and empire"
        ));
        assert!(is_label("Special Equipment: Food processor"));
        assert!(!is_label("Special Butter Cake"));
        let intro = "An unfussy, single-layer or loaf cake is my favorite category of dessert. The cakes in this chapter are like a drapey jumpsuit, breezy and elegant.";
        let html = format!("<p>{intro}</p><p>Rye Cake</p><p>2 cups rye</p><p>Bake.</p>");
        let v = run(
            &html,
            json!({"items":[{"kind":"essay","title":[0]},{"title":[1],"sections":[{"ingredients":[2],"steps":[3]}]}]}),
        );
        assert!(
            matches!(v.hard[0], HardFault::TitleIsProse { local: 0, .. }),
            "{:?}",
            v.hard
        );
    }

    #[test]
    fn prose_ingredient_lines_are_not_an_ingredient_list() {
        let long = "For a batch of classic martinis, combine 2½ cups gin and ½ cup dry vermouth in a pitcher and stir with plenty of ice until very cold.";
        let html = format!(
            "<p>diy martini bar</p><p>{long}</p><p>Serve in chilled glasses with olives and a twist.</p>"
        );
        let v = run(
            &html,
            json!({"items":[{"title":[0],"sections":[{"ingredients":[1],"steps":[2]}]}]}),
        );
        assert!(
            matches!(v.hard[0], HardFault::IngredientsAreProse { .. }),
            "{:?}",
            v.hard
        );
        let ok = run(
            "<p>Gin Martini</p><p>2½ cups gin</p><p>Stir with ice until very cold and strain into glasses, garnishing each with an olive.</p>",
            json!({"items":[{"title":[0],"sections":[{"ingredients":[1],"steps":[2]}]}]}),
        );
        assert!(ok.is_ok());
    }

    #[test]
    fn a_continuation_may_carry_only_steps() {
        let doc = SpineDoc {
            index: 0,
            path: "c.xhtml".into(),
            xhtml: "<html><body><p>Knead the dough well.</p><p>Bake until brown.</p></body></html>"
                .into(),
        };
        let book = BookLines::build(&[doc], &Nav::default());
        let chunk = Chunk {
            id: "k001".into(),
            index: 1,
            start: 0,
            end: 2,
            chars: 0,
            title_hint: Some("Bagels".into()),
            boundary: Boundary::Hard,
        };
        let lowered = lower(
            &chunk,
            &book,
            json!({"items":[{"title":[],"sections":[{"steps":[0,1]}]}]}),
        )
        .unwrap();
        assert!(validate(&chunk, &book, &lowered).is_ok());
    }

    #[test]
    fn recipes_need_ingredients_but_techniques_do_not() {
        let html = "<p>Tempering Chocolate</p><p>Melt two thirds of the chocolate gently over a water bath until smooth.</p>";
        let recipe = run(
            html,
            json!({"items":[{"kind":"recipe","title":[0],"sections":[{"steps":[1]}]}]}),
        );
        assert!(matches!(
            recipe.hard[0],
            HardFault::RecipeWithoutIngredients { .. }
        ));
        let technique = run(
            html,
            json!({"items":[{"kind":"technique","title":[0],"sections":[{"steps":[1]}]}]}),
        );
        assert!(technique.is_ok());
    }

    #[test]
    fn empty_answer_with_a_printed_recipe_is_a_hard_fault() {
        let html =
            "<p>Serves 4</p><p>2 cups flour</p><p>3 eggs</p><p>1 cup milk</p><p>Mix and bake.</p>";
        let v = run(html, json!({"ignored":[0,1,2,3,4]}));
        assert_eq!(v.hard, [HardFault::EmptyWithYield { quantities: 3 }]);
        let prose = run(
            "<p>A chapter about eggs.</p><p>They are good.</p>",
            json!({"ignored":[0,1]}),
        );
        assert!(prose.is_ok());
    }

    #[test]
    fn low_amount_rate_and_ignored_quantities_are_flags() {
        let html = "<p>Salad</p><p>lettuce</p><p>tomato</p><p>onion</p><p>oil</p><p>vinegar</p><p>salt</p><p>Toss.</p>\
                    <p>2 cups rice</p><p>3 cups water</p><p>1 tsp salt</p><p>4 eggs</p>";
        let v = run(
            html,
            json!({"items":[{"title":[0],"sections":[{"ingredients":[1,2,3,4,5,6],"steps":[7]}]}],"ignored":[8,9,10,11]}),
        );
        assert!(v.is_ok());
        assert!(matches!(
            v.soft[0],
            Flag::LowAmountParseRate { lines: 6, .. }
        ));
        assert_eq!(v.soft[1], Flag::IngredientLikeIgnored { count: 4 });
    }

    #[test]
    fn stepless_recipe_with_ignored_prose_is_flagged_unless_last() {
        let long = "Stir the pot slowly over low heat until everything comes together and thickens nicely for the family dinner.";
        let html = format!(
            "<p>Stew</p><p>2 cups beans</p><p>{long}</p><p>{long}</p><p>Bread</p><p>3 cups flour</p><p>Knead.</p>"
        );
        let v = run(
            &html,
            json!({"items":[
            {"title":[0],"sections":[{"ingredients":[1]}]},
            {"title":[4],"sections":[{"ingredients":[5],"steps":[6]}]}],"ignored":[2,3]}),
        );
        assert_eq!(
            v.soft,
            [Flag::RecipeWithoutSteps {
                title: "Stew".into()
            }]
        );
        let last = run(
            &html,
            json!({"items":[
            {"title":[0],"sections":[{"ingredients":[1],"steps":[2,3]}]},
            {"title":[4],"sections":[{"ingredients":[5]}]}],"ignored":[6]}),
        );
        assert!(
            last.soft.is_empty(),
            "the last item may continue into the next chunk"
        );
    }
}
