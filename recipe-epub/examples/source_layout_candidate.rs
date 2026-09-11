//! Offline, profile-pinned source-layout candidate planning.
//!
//! This is deliberately an experiment, not an extraction or acceptance path.
//! It consumes indexed EPUB rows plus an external layout profile and emits
//! unverified source-indexed candidates or explicit deferrals. It never calls a
//! model, does not write a cache, and must not be wired into recovery until the
//! profile semantics have independent source evidence.

use recipe_epub::{
    Chunk, ImageRef, IndexedChunk, Link, SourceElement, SourceElementCoordinate, SourceLine,
    chunk_epub_indexed,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema_version: u32,
    epub_sha256: String,
    recipe_container: Selector,
    roles: RoleSelectors,
    required: Vec<Requirement>,
    defer_if: Vec<Selector>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Selector {
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    class: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleSelectors {
    title: Vec<Selector>,
    description: Vec<Selector>,
    ingredient: Vec<Selector>,
    method: Vec<Selector>,
    section_name: Vec<Selector>,
    /// A translation or alternate name immediately following a section name.
    /// It extends that section's displayed name and never starts a boundary.
    #[serde(default)]
    section_subtitle: Vec<Selector>,
    /// A heading that groups only the ingredient list for a component.
    ///
    /// Unlike `section_name`, this does not establish ownership of following
    /// method rows. Those rows can be shared recipe-level instructions.
    #[serde(default)]
    ingredient_section_name: Vec<Selector>,
    recipe_yield: Vec<Selector>,
    notes: Vec<Selector>,
    #[serde(default)]
    ignored: Vec<Selector>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Requirement {
    selector: Selector,
    min: usize,
    #[serde(default)]
    max: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum Role {
    Title,
    Description,
    Ingredient,
    Method,
    SectionName,
    SectionSubtitle,
    IngredientSectionName,
    RecipeYield,
    Notes,
    Ignored,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Description => "description",
            Self::Ingredient => "ingredient",
            Self::Method => "method",
            Self::SectionName => "section_name",
            Self::SectionSubtitle => "section_subtitle",
            Self::IngredientSectionName => "ingredient_section_name",
            Self::RecipeYield => "recipe_yield",
            Self::Notes => "notes",
            Self::Ignored => "ignored",
        }
    }

    fn all() -> [Self; 10] {
        [
            Self::Title,
            Self::Description,
            Self::Ingredient,
            Self::Method,
            Self::SectionName,
            Self::SectionSubtitle,
            Self::IngredientSectionName,
            Self::RecipeYield,
            Self::Notes,
            Self::Ignored,
        ]
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct RegionKey {
    doc_path: String,
    element_index: usize,
}

#[derive(Debug, Clone, Serialize)]
struct RegionElement {
    element_index: usize,
    tag: String,
    classes: String,
    anchor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct OriginalLine {
    original_chunk_index: usize,
    line_index: usize,
    document_line: usize,
    contributors: Vec<SourceElement>,
    links: Vec<Link>,
    images: Vec<ImageRef>,
    transformed: bool,
}

/// Coordinate-only reference to a source row canonically recorded elsewhere.
///
/// This preserves one global owner for each physical source row while allowing
/// every deferred region affected by an ambiguous row to name that row.
#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct OriginalLineCoordinate {
    original_chunk_index: usize,
    line_index: usize,
    document_line: usize,
}

fn coordinate(line: &OriginalLine) -> OriginalLineCoordinate {
    OriginalLineCoordinate {
        original_chunk_index: line.original_chunk_index,
        line_index: line.line_index,
        document_line: line.document_line,
    }
}

#[derive(Debug, Clone)]
struct PlannedLine {
    original: OriginalLine,
    text: String,
    role: Result<Role, String>,
}

#[derive(Debug)]
struct Region {
    element: RegionElement,
    lines: Vec<PlannedLine>,
    excluded_original_lines: BTreeSet<OriginalLineCoordinate>,
    structural_reasons: BTreeSet<String>,
    unresolved_role_reasons: BTreeSet<String>,
}

#[derive(Debug, Serialize)]
struct Output {
    schema_version: u32,
    epub_sha256: String,
    profile_sha256: String,
    candidates: Vec<Candidate>,
    unassessed: Vec<Unassessed>,
    role_assignments: Option<AssignmentEvidence>,
    diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AssignmentEvidence {
    document_sha256: String,
    epub_sha256: String,
    profile_sha256: String,
    provenance: Value,
    assignments: Vec<RoleAssignment>,
    applied_coordinates: Vec<OriginalLineCoordinate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleAssignmentDocument {
    schema_version: u32,
    epub_sha256: String,
    profile_sha256: String,
    provenance: Value,
    assignments: Vec<RoleAssignment>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RoleAssignment {
    original_chunk_index: usize,
    line_index: usize,
    document_line: usize,
    #[serde(deserialize_with = "required_nullable_role")]
    role: Option<Role>,
}

fn required_nullable_role<'de, D>(deserializer: D) -> Result<Option<Role>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Role>::deserialize(deserializer)
}

type AssignmentMap = BTreeMap<OriginalLineCoordinate, Option<Role>>;

#[derive(Debug, Serialize)]
struct Candidate {
    doc_path: String,
    region_element_index: usize,
    region_element: RegionElement,
    region_chunk: Chunk,
    original_lines: Vec<OriginalLine>,
    indexed_payload: Value,
    lowered_payload: Option<Value>,
    parsed_recipe_count: Option<usize>,
    validation: CandidateValidation,
    diagnostics: Vec<String>,
    verified: bool,
}

#[derive(Debug, Serialize)]
struct CandidateValidation {
    indexed_lowering: bool,
    parsing: bool,
    content_validation: &'static str,
}

#[derive(Debug, Serialize)]
struct Unassessed {
    scope: &'static str,
    doc_path: String,
    region_element_index: Option<usize>,
    original_lines: Vec<OriginalLine>,
    excluded_original_lines: Vec<OriginalLineCoordinate>,
    indexed_payload: Option<Value>,
    lowered_payload: Option<Value>,
    reasons: Vec<String>,
    verified: bool,
}

#[derive(Debug, Serialize)]
struct ResidualOutput {
    schema_version: u32,
    epub_sha256: String,
    profile_sha256: String,
    authority: &'static str,
    regions: Vec<ResidualRegion>,
    diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ResidualRegion {
    doc_path: String,
    region_element_index: usize,
    authority: &'static str,
    region_source_context: Vec<ResidualContextLine>,
    unknown_role_lines: Vec<ResidualUnknownLine>,
}

#[derive(Debug, Serialize)]
struct ResidualContextLine {
    coordinate: OriginalLineCoordinate,
    text: String,
    inferred_role: Option<Role>,
}

#[derive(Debug, Serialize)]
struct ResidualUnknownLine {
    coordinate: OriginalLineCoordinate,
    text: String,
    proposed_roles: Vec<Role>,
    authority: &'static str,
}

#[derive(Debug)]
struct Plan {
    output: Output,
    residual_regions: Vec<ResidualRegion>,
    applied_assignments: BTreeSet<OriginalLineCoordinate>,
}

#[derive(Debug)]
struct Args {
    epub: PathBuf,
    profile: PathBuf,
    out: PathBuf,
    role_assignments: Option<PathBuf>,
    residual_out: Option<PathBuf>,
    recovery_state_out: Option<PathBuf>,
    recovery_model: Option<String>,
}

fn next_value(values: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    values.next().ok_or_else(|| format!("missing {name}"))
}

fn args() -> Result<Args, String> {
    let mut values = env::args().skip(1);
    let mut epub = None;
    let mut profile = None;
    let mut out = None;
    let mut role_assignments = None;
    let mut residual_out = None;
    let mut recovery_state_out = None;
    let mut recovery_model = None;
    while let Some(flag) = values.next() {
        match flag.as_str() {
            "--epub" => epub = Some(next_value(&mut values, "--epub value")?.into()),
            "--profile" => profile = Some(next_value(&mut values, "--profile value")?.into()),
            "--out" => out = Some(next_value(&mut values, "--out value")?.into()),
            "--role-assignments" => {
                role_assignments = Some(next_value(&mut values, "--role-assignments value")?.into())
            }
            "--residual-out" => {
                residual_out = Some(next_value(&mut values, "--residual-out value")?.into())
            }
            "--recovery-state-out" => recovery_state_out = Some(next_value(&mut values, "--recovery-state-out value")?.into()),
            "--recovery-model" => recovery_model = Some(next_value(&mut values, "--recovery-model value")?),
            "--help" => return Err(
                "usage: source_layout_candidate --epub BOOK.epub --profile layout-profile.json --out private-output.json [--role-assignments private-roles.json] [--residual-out private-residual.json] [--recovery-state-out private-state.json --recovery-model MODEL]"
                    .into(),
            ),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if recovery_state_out.is_some() != recovery_model.is_some() {
        return Err("--recovery-state-out and --recovery-model must be supplied together".into());
    }
    Ok(Args {
        epub: epub.ok_or("--epub is required")?,
        profile: profile.ok_or("--profile is required")?,
        out: out.ok_or("--out is required")?,
        role_assignments,
        residual_out,
        recovery_state_out,
        recovery_model,
    })
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_selector(selector: &Selector, path: &str) -> Result<(), String> {
    if selector.tag.as_deref().is_none_or(str::is_empty)
        && selector.class.as_deref().is_none_or(str::is_empty)
    {
        return Err(format!("{path} needs a tag or class"));
    }
    Ok(())
}

fn validate_profile(profile: &Profile) -> Result<(), String> {
    if profile.schema_version != 1 {
        return Err("profile schema_version must be 1".into());
    }
    if profile.epub_sha256.len() != 64
        || !profile
            .epub_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("profile epub_sha256 must be a SHA-256 hex digest".into());
    }
    validate_selector(&profile.recipe_container, "recipe_container")?;
    for (role, selectors) in role_selector_sets(&profile.roles) {
        for (index, selector) in selectors.iter().enumerate() {
            validate_selector(selector, &format!("roles.{role}[{index}]"))?;
        }
    }
    for (index, requirement) in profile.required.iter().enumerate() {
        validate_selector(
            &requirement.selector,
            &format!("required[{index}].selector"),
        )?;
        if requirement.max.is_some_and(|max| max < requirement.min) {
            return Err(format!("required[{index}] has max below min"));
        }
    }
    for (index, selector) in profile.defer_if.iter().enumerate() {
        validate_selector(selector, &format!("defer_if[{index}]"))?;
    }
    Ok(())
}

fn role_selector_sets(roles: &RoleSelectors) -> [(&'static str, &Vec<Selector>); 10] {
    [
        ("title", &roles.title),
        ("description", &roles.description),
        ("ingredient", &roles.ingredient),
        ("method", &roles.method),
        ("section_name", &roles.section_name),
        ("section_subtitle", &roles.section_subtitle),
        ("ingredient_section_name", &roles.ingredient_section_name),
        ("recipe_yield", &roles.recipe_yield),
        ("notes", &roles.notes),
        ("ignored", &roles.ignored),
    ]
}

fn selector_matches(selector: &Selector, tag: &str, classes: &str) -> bool {
    selector.tag.as_deref().is_none_or(|wanted| wanted == tag)
        && selector.class.as_deref().is_none_or(|wanted| {
            classes
                .split_ascii_whitespace()
                .any(|class| class == wanted)
        })
}

fn matching_containers(contributor: &SourceElement, selector: &Selector) -> Vec<RegionElement> {
    let mut matches = vec![];
    if selector_matches(selector, &contributor.tag, &contributor.classes) {
        matches.push(RegionElement {
            element_index: contributor.element_index,
            tag: contributor.tag.clone(),
            classes: contributor.classes.clone(),
            anchor: contributor.anchor.clone(),
        });
    }
    matches.extend(
        contributor
            .ancestors
            .iter()
            .rev()
            .filter(|ancestor| selector_matches(selector, &ancestor.tag, &ancestor.classes))
            .map(region_element),
    );
    matches
}

fn region_element(element: &SourceElementCoordinate) -> RegionElement {
    RegionElement {
        element_index: element.element_index,
        tag: element.tag.clone(),
        classes: element.classes.clone(),
        anchor: element.anchor.clone(),
    }
}

fn region_element_matches(selector: &Selector, element: &RegionElement) -> bool {
    selector_matches(selector, &element.tag, &element.classes)
}

/// All authored elements at or below a region container for one contributor.
/// Ancestors above the region are intentionally excluded so an outer wrapper
/// cannot satisfy an inner recipe's profile requirement or deferral guard.
fn elements_below_region(
    contributor: &SourceElement,
    region: &RegionElement,
) -> Vec<RegionElement> {
    if contributor.element_index == region.element_index {
        return vec![RegionElement {
            element_index: contributor.element_index,
            tag: contributor.tag.clone(),
            classes: contributor.classes.clone(),
            anchor: contributor.anchor.clone(),
        }];
    }
    let Some(position) = contributor
        .ancestors
        .iter()
        .position(|ancestor| ancestor.element_index == region.element_index)
    else {
        return vec![];
    };
    let mut elements: Vec<_> = contributor.ancestors[position + 1..]
        .iter()
        .map(region_element)
        .collect();
    elements.push(RegionElement {
        element_index: contributor.element_index,
        tag: contributor.tag.clone(),
        classes: contributor.classes.clone(),
        anchor: contributor.anchor.clone(),
    });
    elements
}

fn matched_element_indices(region: &Region, selector: &Selector) -> BTreeSet<usize> {
    region
        .lines
        .iter()
        .flat_map(|line| line.original.contributors.iter())
        .flat_map(|contributor| elements_below_region(contributor, &region.element))
        .filter(|element| region_element_matches(selector, element))
        .map(|element| element.element_index)
        .collect()
}

fn apply_profile_guards(region: &mut Region, profile: &Profile) {
    for (index, requirement) in profile.required.iter().enumerate() {
        let count = matched_element_indices(region, &requirement.selector).len();
        let above_maximum = requirement.max.is_some_and(|maximum| count > maximum);
        if count < requirement.min || above_maximum {
            region.structural_reasons.insert(format!(
                "required selector {index} matched {count} elements; expected min {}{}",
                requirement.min,
                requirement
                    .max
                    .map(|maximum| format!(", max {maximum}"))
                    .unwrap_or_default()
            ));
        }
    }
    for (index, selector) in profile.defer_if.iter().enumerate() {
        let count = matched_element_indices(region, selector).len();
        if count > 0 {
            region.structural_reasons.insert(format!(
                "defer_if selector {index} matched {count} elements"
            ));
        }
    }
}

fn nearest_role(contributor: &SourceElement, roles: &RoleSelectors) -> Result<Role, String> {
    let mut levels: Vec<(&str, &str)> = vec![(&contributor.tag, &contributor.classes)];
    levels.extend(
        contributor
            .ancestors
            .iter()
            .rev()
            .map(|ancestor| (ancestor.tag.as_str(), ancestor.classes.as_str())),
    );
    for (tag, classes) in levels {
        let matched: BTreeSet<_> = [
            (Role::Title, &roles.title),
            (Role::Description, &roles.description),
            (Role::Ingredient, &roles.ingredient),
            (Role::Method, &roles.method),
            (Role::SectionName, &roles.section_name),
            (Role::SectionSubtitle, &roles.section_subtitle),
            (Role::IngredientSectionName, &roles.ingredient_section_name),
            (Role::RecipeYield, &roles.recipe_yield),
            (Role::Notes, &roles.notes),
            (Role::Ignored, &roles.ignored),
        ]
        .into_iter()
        .filter_map(|(role, selectors)| {
            selectors
                .iter()
                .any(|selector| selector_matches(selector, tag, classes))
                .then_some(role)
        })
        .collect();
        match matched.len() {
            0 => continue,
            1 => {
                if let Some(role) = matched.first() {
                    return Ok(*role);
                }
                return Err("role selection lost its single matched role".into());
            }
            _ => {
                return Err(format!(
                    "nearest matching element has conflicting roles: {}",
                    matched
                        .iter()
                        .map(|role| role.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }
    Err("unknown recipe-owned line: no contributor matches a configured role selector".into())
}

fn line_role(line: &SourceLine, roles: &RoleSelectors) -> Result<Role, String> {
    if line.contributors.is_empty() {
        return Err("unknown recipe-owned line: no source contributors".into());
    }
    let mut resolved_roles = BTreeSet::new();
    let mut has_unknown_contributor = false;
    for contributor in &line.contributors {
        match nearest_role(contributor, roles) {
            Ok(role) => {
                resolved_roles.insert(role);
            }
            Err(reason) if is_unknown_role_reason(&reason) => {
                has_unknown_contributor = true;
            }
            Err(reason) => return Err(reason),
        }
    }
    if resolved_roles.len() > 1 {
        return Err(format!(
            "line has multiple contributor roles: {}",
            resolved_roles
                .iter()
                .map(|role| role.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if has_unknown_contributor || resolved_roles.is_empty() {
        return Err(
            "unknown recipe-owned line: at least one contributor has no configured role selector"
                .into(),
        );
    }
    resolved_roles
        .first()
        .copied()
        .ok_or_else(|| "line role selection lost its single matched role".into())
}

fn is_unknown_role_reason(reason: &str) -> bool {
    reason.starts_with("unknown recipe-owned line:")
}

fn key(doc_path: &str, element: &RegionElement) -> RegionKey {
    RegionKey {
        doc_path: doc_path.into(),
        element_index: element.element_index,
    }
}

fn candidate_payload(lines: &[PlannedLine]) -> Result<Value, Vec<String>> {
    let title_positions: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.role == Ok(Role::Title)).then_some(index))
        .collect();
    let title_groups = contiguous_groups(&title_positions);
    if title_groups.len() != 1 {
        return Err(vec![format!(
            "multiple incompatible title groups: {}",
            title_groups.len()
        )]);
    }
    let Some(title_group) = title_groups.first() else {
        return Err(vec!["recipe region has no title group".into()]);
    };
    let section_positions: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.role == Ok(Role::SectionName)).then_some(index))
        .collect();
    let section_subtitle_positions: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.role == Ok(Role::SectionSubtitle)).then_some(index))
        .collect();
    let ingredient_section_positions: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.role == Ok(Role::IngredientSectionName)).then_some(index))
        .collect();
    let ingredients_or_methods = lines
        .iter()
        .any(|line| matches!(line.role, Ok(Role::Ingredient | Role::Method)));
    if !ingredients_or_methods {
        return Err(vec![
            "recipe region has no ingredient or method lines".into(),
        ]);
    }
    let sections = if ingredient_section_positions.is_empty() {
        general_sections(lines, &section_positions, &section_subtitle_positions)?
    } else if section_positions.is_empty() {
        if !section_subtitle_positions.is_empty() {
            return Err(vec![
                "ambiguous component ownership: section subtitle has no general section heading"
                    .into(),
            ]);
        }
        ingredient_component_sections(lines, &ingredient_section_positions)?
    } else {
        return Err(vec![
            "ambiguous component ownership: recipe mixes general and ingredient-only section headings"
                .into(),
        ]);
    };
    Ok(json!({
        "recipes": [{
            "title": title_group,
            "description": role_indices(lines, 0, lines.len(), Role::Description),
            "recipe_yield": role_indices(lines, 0, lines.len(), Role::RecipeYield),
            "notes": role_indices(lines, 0, lines.len(), Role::Notes),
            "equipment": [],
            "category": [],
            "page": [],
            "times": {"prep": [], "cook": [], "active": [], "total": []},
            "sections": sections,
        }],
        "ignored": role_indices(lines, 0, lines.len(), Role::Ignored),
    }))
}

/// The established `section_name` meaning: each heading owns both ingredient
/// and method rows until the next heading. Profiles without an
/// `ingredient_section_name` continue through this path unchanged.
fn general_sections(
    lines: &[PlannedLine],
    section_positions: &[usize],
    section_subtitle_positions: &[usize],
) -> Result<Vec<Value>, Vec<String>> {
    if section_positions
        .windows(2)
        .any(|pair| pair[1] == pair[0] + 1)
    {
        return Err(vec![
            "ambiguous component ownership: consecutive section-name lines".into(),
        ]);
    }
    let mut sections = vec![];
    let section_names = paired_section_names(section_positions, section_subtitle_positions)?;
    let first_section = section_positions.first().copied().unwrap_or(lines.len());
    let leading_ingredients = role_indices(lines, 0, first_section, Role::Ingredient);
    let leading_instructions = role_indices(lines, 0, first_section, Role::Method);
    if !leading_ingredients.is_empty() || !leading_instructions.is_empty() {
        sections.push(json!({
            "name": Vec::<usize>::new(),
            "ingredients": leading_ingredients,
            "instructions": leading_instructions,
        }));
    }
    for (section_index, name) in section_names.iter().enumerate() {
        let end = section_names
            .get(section_index + 1)
            .and_then(|next| next.first())
            .copied()
            .unwrap_or(lines.len());
        let body_start = name.last().copied().unwrap_or(0).saturating_add(1);
        sections.push(json!({
            "name": name,
            "ingredients": role_indices(lines, body_start, end, Role::Ingredient),
            "instructions": role_indices(lines, body_start, end, Role::Method),
        }));
    }
    Ok(sections)
}

/// A subtitle is source-owned only when it immediately extends one primary
/// section heading. It is then emitted as the second name line rather than a
/// new boundary. Any other subtitle would require semantic ownership evidence.
fn paired_section_names(
    section_positions: &[usize],
    section_subtitle_positions: &[usize],
) -> Result<Vec<Vec<usize>>, Vec<String>> {
    for subtitle in section_subtitle_positions {
        if !section_positions
            .iter()
            .any(|section| section.saturating_add(1) == *subtitle)
        {
            return Err(vec![
                "ambiguous component ownership: orphan or repeated section subtitle".into(),
            ]);
        }
    }
    Ok(section_positions
        .iter()
        .map(|section| {
            let mut name = vec![*section];
            if section_subtitle_positions.contains(&section.saturating_add(1)) {
                name.push(section.saturating_add(1));
            }
            name
        })
        .collect())
}

/// Ingredient-only headings divide ingredient components, but never make a
/// claim about method rows. Recipe-level methods may occur before every
/// component or after all components, and stay together in one unnamed section.
///
/// A method between the first and last structural component/ingredient row has
/// no source-derived ownership rule here. Keep the whole region unassessed
/// instead of attaching that method by proximity.
fn ingredient_component_sections(
    lines: &[PlannedLine],
    ingredient_section_positions: &[usize],
) -> Result<Vec<Value>, Vec<String>> {
    if ingredient_section_positions
        .windows(2)
        .any(|pair| pair[1] == pair[0] + 1)
    {
        return Err(vec![
            "ambiguous component ownership: consecutive ingredient-only section-name lines".into(),
        ]);
    }

    let ingredient_positions = role_indices(lines, 0, lines.len(), Role::Ingredient);
    let method_positions = role_indices(lines, 0, lines.len(), Role::Method);
    let first_component_or_ingredient = ingredient_section_positions
        .iter()
        .chain(ingredient_positions.iter())
        .min()
        .copied();
    let last_component_or_ingredient = ingredient_section_positions
        .iter()
        .chain(ingredient_positions.iter())
        .max()
        .copied();
    if let (Some(first), Some(last)) = (first_component_or_ingredient, last_component_or_ingredient)
        && method_positions
            .iter()
            .any(|method| *method >= first && *method <= last)
    {
        return Err(vec![
            "ambiguous component ownership: method rows are interleaved with ingredient-only components"
                .into(),
        ]);
    }

    let mut sections = vec![];
    let first_heading = ingredient_section_positions.first().copied();
    let unnamed_ingredients = role_indices(
        lines,
        0,
        first_heading.unwrap_or(lines.len()),
        Role::Ingredient,
    );
    if !unnamed_ingredients.is_empty() {
        sections.push(json!({
            "name": Vec::<usize>::new(),
            "ingredients": unnamed_ingredients,
            "instructions": Vec::<usize>::new(),
        }));
    }

    for (component_index, start) in ingredient_section_positions.iter().enumerate() {
        let end = ingredient_section_positions
            .get(component_index + 1)
            .copied()
            .unwrap_or(lines.len());
        sections.push(json!({
            "name": [start],
            "ingredients": role_indices(lines, start + 1, end, Role::Ingredient),
            "instructions": Vec::<usize>::new(),
        }));
    }

    if !method_positions.is_empty() {
        // Keep this section after all component ingredients. Reusing an
        // earlier unnamed ingredient section would serialize the shared method
        // before later named components and falsely imply that ownership.
        sections.push(json!({
            "name": Vec::<usize>::new(),
            "ingredients": Vec::<usize>::new(),
            "instructions": method_positions,
        }));
    }
    Ok(sections)
}

fn role_indices(lines: &[PlannedLine], start: usize, end: usize, role: Role) -> Vec<usize> {
    lines[start..end]
        .iter()
        .enumerate()
        .filter_map(|(offset, line)| (line.role == Ok(role)).then_some(start + offset))
        .collect()
}

fn contiguous_groups(indices: &[usize]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = vec![];
    for &index in indices {
        if let Some(last) = groups.last_mut()
            && last.last().is_some_and(|previous| *previous + 1 == index)
        {
            last.push(index);
            continue;
        }
        groups.push(vec![index]);
    }
    groups
}

fn synthetic_chunk(key: &RegionKey, lines: &[PlannedLine]) -> Chunk {
    Chunk {
        title_hint: None,
        text: lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        doc_path: key.doc_path.clone(),
        links: lines
            .iter()
            .flat_map(|line| line.original.links.iter().cloned())
            .collect(),
        images: lines
            .iter()
            .enumerate()
            .flat_map(|(line_index, line)| {
                line.original
                    .images
                    .iter()
                    .cloned()
                    .map(move |image| (line_index, image))
            })
            .collect(),
    }
}

fn residual_region(key: &RegionKey, region: &Region) -> ResidualRegion {
    let region_source_context = region
        .lines
        .iter()
        .map(|line| ResidualContextLine {
            coordinate: coordinate(&line.original),
            text: line.text.clone(),
            inferred_role: line.role.as_ref().ok().copied(),
        })
        .collect();
    let unknown_role_lines = region
        .lines
        .iter()
        .filter(|line| line.role.is_err())
        .map(|line| ResidualUnknownLine {
            coordinate: coordinate(&line.original),
            text: line.text.clone(),
            proposed_roles: Role::all().to_vec(),
            authority: "NONAUTHORITATIVE",
        })
        .collect();
    ResidualRegion {
        doc_path: key.doc_path.clone(),
        region_element_index: key.element_index,
        authority: "NONAUTHORITATIVE",
        region_source_context,
        unknown_role_lines,
    }
}

fn plan_with_assignments(
    indexed: Vec<IndexedChunk>,
    profile: &Profile,
    assignments: &AssignmentMap,
) -> Plan {
    let mut regions: BTreeMap<RegionKey, Region> = BTreeMap::new();
    let mut unassessed = vec![];
    let mut diagnostics = vec![];
    let mut applied_assignments = BTreeSet::new();
    let mut residual_regions = vec![];
    for (original_chunk_index, indexed_chunk) in indexed.into_iter().enumerate() {
        let text_lines: Vec<_> = indexed_chunk.chunk.text.lines().collect();
        if text_lines.len() != indexed_chunk.lines.len() {
            diagnostics.push(format!(
                "chunk {original_chunk_index} coordinate mismatch: {} text lines, {} indexed lines",
                text_lines.len(),
                indexed_chunk.lines.len()
            ));
            continue;
        }
        for (line_index, (text, source)) in
            text_lines.into_iter().zip(indexed_chunk.lines).enumerate()
        {
            let original = OriginalLine {
                original_chunk_index,
                line_index,
                document_line: source.document_line,
                contributors: source.contributors.clone(),
                links: source.links.clone(),
                images: source.images.clone(),
                transformed: source.transformed,
            };
            let contributor_containers: Vec<Vec<_>> = source
                .contributors
                .iter()
                .map(|contributor| {
                    matching_containers(contributor, &profile.recipe_container)
                        .into_iter()
                        .map(|element| {
                            let key = key(&indexed_chunk.chunk.doc_path, &element);
                            (key, element)
                        })
                        .collect()
                })
                .collect();
            let has_outside_contributor = contributor_containers.iter().any(Vec::is_empty);
            let containers: BTreeMap<_, _> = contributor_containers.into_iter().flatten().collect();
            match (containers.len(), has_outside_contributor) {
                (0, _) => unassessed.push(Unassessed {
                    scope: "outside_container",
                    doc_path: indexed_chunk.chunk.doc_path.clone(),
                    region_element_index: None,
                    original_lines: vec![original],
                    excluded_original_lines: vec![],
                    indexed_payload: None,
                    lowered_payload: None,
                    reasons: vec![
                        "outside configured recipe container; not assessed as non-recipe".into(),
                    ],
                    verified: false,
                }),
                (1, false) => {
                    let Some((region_key, element)) = containers.into_iter().next() else {
                        unassessed.push(Unassessed {
                            scope: "ambiguous_container",
                            doc_path: indexed_chunk.chunk.doc_path.clone(),
                            region_element_index: None,
                            original_lines: vec![original],
                            excluded_original_lines: vec![],
                            indexed_payload: None,
                            lowered_payload: None,
                            reasons: vec![
                                "container planning lost its one selected recipe container".into(),
                            ],
                            verified: false,
                        });
                        continue;
                    };
                    let region = regions.entry(region_key).or_insert_with(|| Region {
                        element,
                        lines: vec![],
                        excluded_original_lines: BTreeSet::new(),
                        structural_reasons: BTreeSet::new(),
                        unresolved_role_reasons: BTreeSet::new(),
                    });
                    if source.transformed {
                        region
                            .structural_reasons
                            .insert("transformed source row in recipe region".into());
                    }
                    let inferred_role = line_role(&source, &profile.roles);
                    let role = match inferred_role {
                        Ok(role) => Ok(role),
                        Err(reason) if is_unknown_role_reason(&reason) => {
                            match assignments.get(&coordinate(&original)) {
                                Some(Some(role)) => {
                                    applied_assignments.insert(coordinate(&original));
                                    Ok(*role)
                                }
                                Some(None) | None => Err(reason),
                            }
                        }
                        Err(reason) => Err(reason),
                    };
                    if let Err(reason) = &role {
                        if is_unknown_role_reason(reason) {
                            region.unresolved_role_reasons.insert(reason.clone());
                        } else {
                            region.structural_reasons.insert(reason.clone());
                        }
                    }
                    region.lines.push(PlannedLine {
                        original,
                        text: text.into(),
                        role,
                    });
                }
                _ => {
                    let coordinate_label =
                        format!("original chunk {original_chunk_index}, line {line_index}");
                    let reasons = vec![if has_outside_contributor {
                        format!(
                            "line at {coordinate_label} mixes recipe-container and outside-container contributors"
                        )
                    } else {
                        format!(
                            "line at {coordinate_label} has contributors from multiple recipe containers"
                        )
                    }];
                    for (region_key, element) in containers {
                        let region = regions.entry(region_key).or_insert_with(|| Region {
                            element,
                            lines: vec![],
                            excluded_original_lines: BTreeSet::new(),
                            structural_reasons: BTreeSet::new(),
                            unresolved_role_reasons: BTreeSet::new(),
                        });
                        region.structural_reasons.extend(reasons.iter().cloned());
                        region.excluded_original_lines.insert(coordinate(&original));
                    }
                    unassessed.push(Unassessed {
                        scope: "ambiguous_container",
                        doc_path: indexed_chunk.chunk.doc_path.clone(),
                        region_element_index: None,
                        original_lines: vec![original],
                        excluded_original_lines: vec![],
                        indexed_payload: None,
                        lowered_payload: None,
                        reasons,
                        verified: false,
                    });
                }
            }
        }
    }

    let mut candidates = vec![];
    for (region_key, mut region) in regions {
        region.lines.sort_by_key(|line| {
            (
                line.original.original_chunk_index,
                line.original.line_index,
                line.original.document_line,
            )
        });
        apply_profile_guards(&mut region, profile);
        if region.structural_reasons.is_empty() && !region.unresolved_role_reasons.is_empty() {
            residual_regions.push(residual_region(&region_key, &region));
        }
        let mut reasons = region.structural_reasons.clone();
        reasons.extend(region.unresolved_role_reasons.iter().cloned());
        let payload_result = reasons.is_empty().then(|| candidate_payload(&region.lines));
        if let Some(Err(payload_reasons)) = &payload_result {
            reasons.extend(payload_reasons.iter().cloned());
        }
        if !reasons.is_empty() {
            unassessed.push(Unassessed {
                scope: "recipe_region",
                doc_path: region_key.doc_path,
                region_element_index: Some(region_key.element_index),
                original_lines: region.lines.into_iter().map(|line| line.original).collect(),
                excluded_original_lines: region.excluded_original_lines.into_iter().collect(),
                indexed_payload: None,
                lowered_payload: None,
                reasons: reasons.into_iter().collect(),
                verified: false,
            });
            continue;
        }
        let Some(Ok(indexed_payload)) = payload_result else {
            unassessed.push(Unassessed {
                scope: "recipe_region",
                doc_path: region_key.doc_path,
                region_element_index: Some(region_key.element_index),
                original_lines: region.lines.into_iter().map(|line| line.original).collect(),
                excluded_original_lines: region.excluded_original_lines.into_iter().collect(),
                indexed_payload: None,
                lowered_payload: None,
                reasons: vec!["candidate payload was unavailable after planning".into()],
                verified: false,
            });
            continue;
        };
        let chunk = synthetic_chunk(&region_key, &region.lines);
        let lowered_payload =
            match recipe_epub::indexed::lower_indexed_payload(&chunk, indexed_payload.clone()) {
                Ok(lowered) => lowered,
                Err(error) => {
                    unassessed.push(Unassessed {
                        scope: "recipe_region",
                        doc_path: region_key.doc_path,
                        region_element_index: Some(region_key.element_index),
                        original_lines: region
                            .lines
                            .into_iter()
                            .map(|line| line.original)
                            .collect(),
                        excluded_original_lines: region
                            .excluded_original_lines
                            .into_iter()
                            .collect(),
                        indexed_payload: Some(indexed_payload),
                        lowered_payload: None,
                        reasons: vec![format!("indexed payload lowering failed: {error}")],
                        verified: false,
                    });
                    continue;
                }
            };
        let recipes =
            match recipe_epub::indexed::parse_indexed_recipes(&chunk, indexed_payload.clone()) {
                Ok(recipes) => recipes,
                Err(error) => {
                    unassessed.push(Unassessed {
                        scope: "recipe_region",
                        doc_path: region_key.doc_path,
                        region_element_index: Some(region_key.element_index),
                        original_lines: region
                            .lines
                            .into_iter()
                            .map(|line| line.original)
                            .collect(),
                        excluded_original_lines: region
                            .excluded_original_lines
                            .into_iter()
                            .collect(),
                        indexed_payload: Some(indexed_payload),
                        lowered_payload: Some(lowered_payload),
                        reasons: vec![format!("indexed payload validation failed: {error}")],
                        verified: false,
                    });
                    continue;
                }
            };
        candidates.push(Candidate {
            doc_path: region_key.doc_path,
            region_element_index: region_key.element_index,
            region_element: region.element,
            region_chunk: chunk,
            original_lines: region.lines.into_iter().map(|line| line.original).collect(),
            indexed_payload,
            lowered_payload: Some(lowered_payload),
            parsed_recipe_count: Some(recipes.len()),
            validation: CandidateValidation {
                indexed_lowering: true,
                parsing: true,
                content_validation: "passed public indexed literal-content validation; semantic and whole-book verification remain unassessed",
            },
            diagnostics: vec![],
            verified: false,
        });
    }
    candidates.sort_by_key(|candidate| {
        (
            candidate
                .original_lines
                .first()
                .map(|line| (line.original_chunk_index, line.line_index))
                .unwrap_or((usize::MAX, usize::MAX)),
            candidate.doc_path.clone(),
            candidate.region_element_index,
        )
    });
    unassessed.sort_by_key(|entry| {
        (
            entry
                .original_lines
                .first()
                .map(|line| (line.original_chunk_index, line.line_index))
                .unwrap_or((usize::MAX, usize::MAX)),
            entry.doc_path.clone(),
            entry.region_element_index.unwrap_or(usize::MAX),
        )
    });
    diagnostics.push("All output is experimental and unverified; no candidate is accepted or treated as non-recipe proof.".into());
    residual_regions.sort_by_key(|region| {
        (
            region
                .region_source_context
                .first()
                .map(|line| {
                    (
                        line.coordinate.original_chunk_index,
                        line.coordinate.line_index,
                    )
                })
                .unwrap_or((usize::MAX, usize::MAX)),
            region.doc_path.clone(),
            region.region_element_index,
        )
    });
    Plan {
        output: Output {
            schema_version: 1,
            epub_sha256: profile.epub_sha256.clone(),
            profile_sha256: String::new(),
            candidates,
            unassessed,
            role_assignments: None,
            diagnostics,
        },
        residual_regions,
        applied_assignments,
    }
}

#[cfg(test)]
fn plan(indexed: Vec<IndexedChunk>, profile: &Profile) -> Output {
    plan_with_assignments(indexed, profile, &AssignmentMap::new()).output
}

fn write_new<T: Serialize>(path: &Path, value: &T) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err(format!("refusing to overwrite existing output: {}", path.display()).into());
    }
    let parent = path.parent().ok_or("output path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn assignment_coordinate(assignment: &RoleAssignment) -> OriginalLineCoordinate {
    OriginalLineCoordinate {
        original_chunk_index: assignment.original_chunk_index,
        line_index: assignment.line_index,
        document_line: assignment.document_line,
    }
}

fn load_role_assignments(
    bytes: &[u8],
    epub_sha256: &str,
    profile_sha256: &str,
    indexed: &[IndexedChunk],
    profile: &Profile,
) -> Result<(RoleAssignmentDocument, AssignmentMap), String> {
    let document: RoleAssignmentDocument = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid role assignments: {error}"))?;
    if document.schema_version != 1 {
        return Err("role assignments schema_version must be 1".into());
    }
    if document.epub_sha256 != epub_sha256 {
        return Err("role assignments EPUB SHA-256 does not match input".into());
    }
    if document.profile_sha256 != profile_sha256 {
        return Err("role assignments profile SHA-256 does not match input".into());
    }
    let mut assignments = AssignmentMap::new();
    for assignment in &document.assignments {
        let coordinate = assignment_coordinate(assignment);
        let chunk = indexed
            .get(coordinate.original_chunk_index)
            .ok_or_else(|| {
                format!(
                    "role assignment references missing chunk {}",
                    coordinate.original_chunk_index
                )
            })?;
        let line = chunk.lines.get(coordinate.line_index).ok_or_else(|| {
            format!(
                "role assignment references missing line {} in chunk {}",
                coordinate.line_index, coordinate.original_chunk_index
            )
        })?;
        if line.document_line != coordinate.document_line {
            return Err(format!(
                "role assignment has stale document_line at chunk {}, line {}",
                coordinate.original_chunk_index, coordinate.line_index
            ));
        }
        let contributor_containers: BTreeSet<_> = line
            .contributors
            .iter()
            .flat_map(|contributor| matching_containers(contributor, &profile.recipe_container))
            .map(|element| element.element_index)
            .collect();
        let has_outside_contributor = line.contributors.iter().any(|contributor| {
            matching_containers(contributor, &profile.recipe_container).is_empty()
        });
        if contributor_containers.len() != 1 || has_outside_contributor {
            return Err(format!(
                "role assignment does not target one unambiguous recipe region at chunk {}, line {}",
                coordinate.original_chunk_index, coordinate.line_index
            ));
        }
        match line_role(line, &profile.roles) {
            Ok(_) => {
                return Err(format!(
                    "role assignment targets an already unambiguous line at chunk {}, line {}",
                    coordinate.original_chunk_index, coordinate.line_index
                ));
            }
            Err(reason) if !is_unknown_role_reason(&reason) => {
                return Err(format!(
                    "role assignment targets a conflicting role line at chunk {}, line {}",
                    coordinate.original_chunk_index, coordinate.line_index
                ));
            }
            Err(_) => {}
        }
        if assignments.insert(coordinate, assignment.role).is_some() {
            return Err(format!(
                "duplicate role assignment at chunk {}, line {}",
                coordinate.original_chunk_index, coordinate.line_index
            ));
        }
    }
    Ok((document, assignments))
}

/// Translate only complete original chunks. Unassessed rows are never inferred
/// to be ignored, and a region crossing a chunk boundary remains on extraction.
fn recovery_state(
    indexed: &[IndexedChunk],
    output: &Output,
    model: &str,
    documents: &[recipe_epub::source::SourceDocument],
) -> Result<recipe_epub::recovery::State, String> {
    use recipe_epub::recovery::{
        SeedChunk, SeedModel, SeedProvenance, State, UnverifiedCandidateSeed,
        canonical_source_sha256,
    };
    let source: Vec<_> = indexed.iter().map(|item| item.chunk.clone()).collect();
    let source_sha256 = canonical_source_sha256(&source)?;
    let catalog = recipe_epub::models::catalog()
        .into_iter()
        .find(|entry| entry.id == model && entry.enabled)
        .ok_or("recovery seed needs an enabled catalog model")?;
    let mut payloads = vec![json!({"recipes": [], "ignored": []}); source.len()];
    let mut covered = vec![BTreeSet::new(); source.len()];
    for candidate in &output.candidates {
        if candidate.verified {
            return Err("layout candidates must remain unverified".into());
        }
        let chunks: BTreeSet<_> = candidate
            .original_lines
            .iter()
            .map(|line| line.original_chunk_index)
            .collect();
        if chunks.len() != 1 {
            continue;
        }
        let chunk = *chunks
            .first()
            .ok_or("candidate has no source coordinates")?;
        let original = indexed
            .get(chunk)
            .ok_or("candidate references an absent chunk")?;
        let text: Vec<_> = original.chunk.text.lines().collect();
        let region_text: Vec<_> = candidate.region_chunk.text.lines().collect();
        if region_text.len() != candidate.original_lines.len()
            || original.chunk.doc_path != candidate.doc_path
        {
            return Err("candidate region does not match original source".into());
        }
        let mut map = Vec::new();
        for (line, region_text) in candidate.original_lines.iter().zip(region_text) {
            let indexed_line = original
                .lines
                .get(line.line_index)
                .ok_or("candidate references an absent source line")?;
            if indexed_line.document_line != line.document_line
                || text.get(line.line_index).copied() != Some(region_text)
                || indexed_line.transformed
                || !covered[chunk].insert(line.line_index)
            {
                return Err("candidate source binding is stale, transformed, or repeated".into());
            }
            map.push(line.line_index);
        }
        fn remap(value: &Value, map: &[usize]) -> Result<Value, String> {
            match value {
                Value::Number(index) => {
                    let index = index
                        .as_u64()
                        .and_then(|i| usize::try_from(i).ok())
                        .ok_or("invalid candidate source index")?;
                    Ok(json!(
                        map.get(index).ok_or("candidate index exceeds region")?
                    ))
                }
                Value::Array(values) => values
                    .iter()
                    .map(|v| remap(v, map))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Value::Array),
                Value::Object(fields) => fields
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), remap(v, map)?)))
                    .collect::<Result<serde_json::Map<_, _>, String>>()
                    .map(Value::Object),
                _ => Err("candidate indexed payload contains non-index content".into()),
            }
        }
        let remapped = remap(&candidate.indexed_payload, &map)?;
        for field in ["recipes", "ignored"] {
            let values = remapped[field]
                .as_array()
                .ok_or("candidate payload is malformed")?;
            payloads[chunk][field]
                .as_array_mut()
                .ok_or("invalid seed accumulator")?
                .extend(values.iter().cloned());
        }
    }
    let complete: BTreeSet<_> = covered
        .iter()
        .enumerate()
        .filter(|(i, lines)| lines.len() == indexed[*i].lines.len() && !lines.is_empty())
        .map(|(i, _)| i)
        .collect();
    let mut state = State::new(source, model, 10.0)?;
    state.bind_documents(documents)?;
    state.bind_source_line_provenance(
        &indexed
            .iter()
            .map(|chunk| chunk.lines.clone())
            .collect::<Vec<_>>(),
    )?;
    let groups: Vec<_> = state
        .groups
        .iter()
        .enumerate()
        .filter(|(_, group)| group.chunks.iter().all(|chunk| complete.contains(chunk)))
        .map(|(group, value)| (group, value.chunks.clone()))
        .collect();
    let mut hashes = BTreeMap::from([
        ("epub".into(), output.epub_sha256.clone()),
        ("profile".into(), output.profile_sha256.clone()),
        (
            "layout_artifact".into(),
            sha256(&serde_json::to_vec_pretty(output).map_err(|e| e.to_string())?),
        ),
    ]);
    if let Some(assignments) = &output.role_assignments {
        hashes.insert(
            "role_assignments".into(),
            assignments.document_sha256.clone(),
        );
    }
    for (group, chunks) in groups {
        state.install_unverified_seed(UnverifiedCandidateSeed {
            group,
            model: SeedModel {
                id: model.into(),
                provider: catalog.provider.into(),
                transport: catalog.transport.into(),
            },
            chunks: chunks
                .into_iter()
                .map(|chunk| SeedChunk {
                    chunk,
                    indexed_response: payloads[chunk].clone(),
                })
                .collect(),
            provenance: SeedProvenance {
                source_sha256: source_sha256.clone(),
                hashes: hashes.clone(),
            },
        })?;
    }
    Ok(state)
}

fn verification_context_metrics(
    actions: &[recipe_epub::recovery::Action],
) -> Result<Value, String> {
    let mut unique = BTreeMap::<usize, Vec<u8>>::new();
    let mut total = 0usize;
    let mut requests = Vec::new();
    for action in actions
        .iter()
        .filter(|action| action.verification_chunk.is_some())
    {
        let user: Value = serde_json::from_str(&action.request.user).map_err(|e| e.to_string())?;
        let source = user["source"]
            .as_array()
            .ok_or("verifier request has no source context")?;
        let mut chunks = Vec::new();
        let mut bytes = 0usize;
        for chunk in source {
            let id = chunk["chunk"]
                .as_u64()
                .and_then(|id| usize::try_from(id).ok())
                .ok_or("verifier source context has invalid chunk identity")?;
            let serialized = serde_json::to_vec(chunk).map_err(|e| e.to_string())?;
            bytes += serialized.len();
            if let Some(previous) = unique.get(&id) {
                if previous != &serialized {
                    return Err("verifier contexts disagree on source chunk content".into());
                }
            } else {
                unique.insert(id, serialized);
            }
            chunks.push(id);
        }
        total += bytes;
        requests.push(json!({
            "target_chunk": action.verification_chunk,
            "model": action.model,
            "source_chunks": chunks,
            "serialized_source_chunk_bytes": bytes,
            "candidate_bytes": serde_json::to_vec(&user["candidate"]).map_err(|e| e.to_string())?.len(),
            "user_bytes": action.request.user.len(),
        }));
    }
    let unique_bytes = unique.values().map(Vec::len).sum::<usize>();
    Ok(json!({
        "requests": requests,
        "total_serialized_source_chunk_bytes": total,
        "unique_serialized_source_chunk_bytes": unique_bytes,
        "repeated_serialized_source_chunk_bytes": total - unique_bytes,
        "scope": "Source chunk objects only; no token, cost, or latency saving is established",
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = args().map_err(std::io::Error::other)?;
    let epub = fs::read(&args.epub)?;
    let profile_bytes = fs::read(&args.profile)?;
    let profile: Profile = serde_json::from_slice(&profile_bytes)?;
    validate_profile(&profile).map_err(std::io::Error::other)?;
    let epub_sha256 = sha256(&epub);
    let profile_sha256 = sha256(&profile_bytes);
    if profile.epub_sha256 != epub_sha256 {
        return Err(format!(
            "profile EPUB SHA-256 does not match input: profile={}, input={epub_sha256}",
            profile.epub_sha256
        )
        .into());
    }
    let indexed = chunk_epub_indexed(&epub)?;
    for (index, chunk) in indexed.iter().enumerate() {
        if chunk.chunk.text.lines().count() != chunk.lines.len() {
            return Err(format!(
                "indexed input coordinate mismatch in chunk {index}: {} text lines, {} indexed lines",
                chunk.chunk.text.lines().count(),
                chunk.lines.len()
            )
            .into());
        }
    }
    if args.out.exists() {
        return Err(format!(
            "refusing to overwrite existing output: {}",
            args.out.display()
        )
        .into());
    }
    if args
        .recovery_state_out
        .as_ref()
        .is_some_and(|path| path.exists())
    {
        return Err("refusing to overwrite recovery state output".into());
    }
    if let Some(path) = &args.residual_out
        && path.exists()
    {
        return Err(format!(
            "refusing to overwrite existing residual output: {}",
            path.display()
        )
        .into());
    }
    let assignment_input = if let Some(path) = &args.role_assignments {
        let bytes = fs::read(path)?;
        let document =
            load_role_assignments(&bytes, &epub_sha256, &profile_sha256, &indexed, &profile)
                .map_err(std::io::Error::other)?;
        Some((document, sha256(&bytes)))
    } else {
        None
    };
    let empty_assignments = AssignmentMap::new();
    let assignments = assignment_input
        .as_ref()
        .map(|(document, _)| &document.1)
        .unwrap_or(&empty_assignments);
    let recovery_source = args.recovery_state_out.as_ref().map(|_| indexed.clone());
    let recovery_documents = if recovery_source.is_some() {
        recipe_epub::source::inspect_source(&epub)?
    } else {
        Vec::new()
    };
    let mut plan = plan_with_assignments(indexed, &profile, assignments);
    plan.output.epub_sha256 = epub_sha256.clone();
    plan.output.profile_sha256 = profile_sha256.clone();
    if let Some(((document, _), document_sha256)) = assignment_input {
        plan.output.role_assignments = Some(AssignmentEvidence {
            document_sha256,
            epub_sha256: document.epub_sha256,
            profile_sha256: document.profile_sha256,
            provenance: document.provenance,
            assignments: document.assignments,
            applied_coordinates: plan.applied_assignments.into_iter().collect(),
        });
    }
    if let Some(path) = &args.residual_out {
        let residual = ResidualOutput {
            schema_version: 1,
            epub_sha256,
            profile_sha256,
            authority: "NONAUTHORITATIVE",
            regions: plan.residual_regions,
            diagnostics: vec![
                "Residual role proposals are nonauthoritative future-model input; they are not source expectations or acceptance evidence.".into(),
            ],
        };
        write_new(path, &residual)?;
    }
    let seed_state = if let (Some(source), Some(model)) = (&recovery_source, &args.recovery_model) {
        Some(
            recovery_state(source, &plan.output, model, &recovery_documents)
                .map_err(std::io::Error::other)?,
        )
    } else {
        None
    };
    write_new(&args.out, &plan.output)?;
    if let (Some(path), Some(state)) = (&args.recovery_state_out, seed_state) {
        let actions = state.planned_actions().map_err(std::io::Error::other)?;
        let extraction = actions
            .iter()
            .filter(|action| action.chunk.is_some())
            .count();
        let verification = actions.len() - extraction;
        write_new(path, &state)?;
        println!(
            "{}",
            json!({
                "recovery_state": path,
                "verified": false,
                "seeded_groups": state.groups.iter().filter(|g| !g.candidates.is_empty()).count(),
                "currently_planned_extraction_actions": extraction,
                "currently_planned_verification_actions": verification,
                "currently_planned_reservations_usd": actions.iter().map(|a| a.reservation_usd).sum::<f64>(),
                "reservation_scope": "current actions only; excludes later verification, recovery, and retries",
                "verification_context": verification_context_metrics(&actions).map_err(std::io::Error::other)?,
                "provider_calls": 0,
            })
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_fixture() -> Vec<IndexedChunk> {
        let owner = ancestor(10, "article", "recipe");
        let recipe = indexed(vec![
            (
                "Example soup",
                source_line(0, vec![element(11, "h2", "title", vec![owner.clone()])]),
            ),
            (
                "1 cup water",
                source_line(
                    1,
                    vec![element(12, "li", "ingredient", vec![owner.clone()])],
                ),
            ),
            (
                "Boil the water.",
                source_line(2, vec![element(13, "p", "method", vec![owner])]),
            ),
        ]);
        let mut outside = indexed(vec![("Unassessed introduction", source_line(0, vec![]))]);
        outside.chunk.doc_path = "intro.xhtml".into();
        vec![recipe, outside]
    }

    #[test]
    fn complete_layout_chunk_seeds_verification_and_unassessed_source_still_extracts() {
        let source = seed_fixture();
        let mut output = plan(source.clone(), &profile());
        output.profile_sha256 = "1".repeat(64);
        let state = recovery_state(&source, &output, "gemini-2.5-flash", &[]).unwrap();
        assert!(!state.complete());
        assert!(state.attempts.is_empty());
        assert_eq!(state.groups[0].candidates.len(), 1);
        assert!(!state.groups[0].candidates[0].verified);
        assert!(state.groups[1].candidates.is_empty());
        let actions = state.planned_actions().unwrap();
        assert!(actions.iter().any(|a| a.verification_chunk == Some(0)));
        assert!(!actions.iter().any(|a| a.chunk == Some(0)));
        assert!(actions.iter().any(|a| a.chunk == Some(1)));
    }

    #[test]
    fn unassessed_line_in_recipe_chunk_prevents_seeding() {
        let mut source = seed_fixture();
        source[0]
            .chunk
            .text
            .push_str("\nUnassessed trailing guidance");
        source[0].lines.push(source_line(3, vec![]));
        let mut output = plan(source.clone(), &profile());
        output.profile_sha256 = "1".repeat(64);
        let state = recovery_state(&source, &output, "gemini-2.5-flash", &[]).unwrap();
        assert!(state.groups.iter().all(|g| g.candidates.is_empty()));
    }

    #[test]
    fn recovery_seed_rejects_changed_source_and_duplicate_ownership() {
        let source = seed_fixture();
        let mut output = plan(source.clone(), &profile());
        output.profile_sha256 = "1".repeat(64);
        let mut stale = source.clone();
        stale[0].chunk.text = stale[0].chunk.text.replace("water", "milk");
        assert!(recovery_state(&stale, &output, "gemini-2.5-flash", &[]).is_err());
        output.candidates[0].original_lines[1].line_index = 0;
        assert!(recovery_state(&source, &output, "gemini-2.5-flash", &[]).is_err());
    }

    fn selector(tag: &str, class: &str) -> Selector {
        Selector {
            tag: Some(tag.into()),
            class: Some(class.into()),
        }
    }

    fn profile() -> Profile {
        Profile {
            schema_version: 1,
            epub_sha256: "0".repeat(64),
            recipe_container: selector("article", "recipe"),
            required: vec![],
            defer_if: vec![],
            roles: RoleSelectors {
                title: vec![selector("h2", "title")],
                description: vec![selector("p", "headnote")],
                ingredient: vec![selector("li", "ingredient")],
                method: vec![selector("p", "method")],
                section_name: vec![selector("h3", "section")],
                section_subtitle: vec![],
                ingredient_section_name: vec![],
                recipe_yield: vec![selector("span", "yield")],
                notes: vec![selector("p", "notes")],
                ignored: vec![],
            },
        }
    }

    fn element(
        index: usize,
        tag: &str,
        class: &str,
        ancestors: Vec<SourceElementCoordinate>,
    ) -> SourceElement {
        SourceElement {
            element_index: index,
            tag: tag.into(),
            classes: class.into(),
            anchor: None,
            ancestors,
        }
    }

    fn ancestor(index: usize, tag: &str, class: &str) -> SourceElementCoordinate {
        SourceElementCoordinate {
            element_index: index,
            tag: tag.into(),
            classes: class.into(),
            anchor: None,
        }
    }

    fn indexed(lines: Vec<(&str, SourceLine)>) -> IndexedChunk {
        IndexedChunk {
            chunk: Chunk {
                title_hint: None,
                text: lines
                    .iter()
                    .map(|(text, _)| *text)
                    .collect::<Vec<_>>()
                    .join("\n"),
                doc_path: "chapter.xhtml".into(),
                links: vec![],
                images: vec![],
            },
            lines: lines.into_iter().map(|(_, line)| line).collect(),
        }
    }

    fn source_line(document_line: usize, contributors: Vec<SourceElement>) -> SourceLine {
        SourceLine {
            anchors: Vec::new(),
            document_line,
            contributors,
            links: vec![],
            images: vec![],
            transformed: false,
        }
    }

    fn assignment_bytes(epub_sha256: &str, profile_sha256: &str, assignments: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "epub_sha256": epub_sha256,
            "profile_sha256": profile_sha256,
            "provenance": {"kind": "synthetic-test"},
            "assignments": assignments,
        }))
        .unwrap()
    }

    fn unknown_role_input() -> Vec<IndexedChunk> {
        let recipe = ancestor(10, "article", "recipe");
        vec![indexed(vec![
            (
                "Example soup",
                source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
            ),
            (
                "Unmapped prose",
                source_line(1, vec![element(12, "p", "unmapped", vec![recipe])]),
            ),
        ])]
    }

    fn all_original_coordinates(output: &Output) -> Vec<(usize, usize)> {
        let mut coordinates = output
            .candidates
            .iter()
            .flat_map(|candidate| candidate.original_lines.iter())
            .chain(
                output
                    .unassessed
                    .iter()
                    .flat_map(|entry| entry.original_lines.iter()),
            )
            .map(|line| (line.original_chunk_index, line.line_index))
            .collect::<Vec<_>>();
        coordinates.sort_unstable();
        coordinates
    }

    #[test]
    fn component_headings_make_distinct_indexed_sections() {
        let recipe = ancestor(10, "article", "recipe");
        let output = plan(
            vec![indexed(vec![
                (
                    "Example soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Soup",
                    source_line(1, vec![element(12, "h3", "section", vec![recipe.clone()])]),
                ),
                (
                    "1 cup water",
                    source_line(
                        2,
                        vec![element(13, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Stir.",
                    source_line(3, vec![element(14, "p", "method", vec![recipe.clone()])]),
                ),
                (
                    "To serve",
                    source_line(4, vec![element(15, "h3", "section", vec![recipe.clone()])]),
                ),
                (
                    "Salt",
                    source_line(5, vec![element(16, "li", "ingredient", vec![recipe])]),
                ),
            ])],
            &profile(),
        );
        assert_eq!(output.candidates.len(), 1);
        assert_eq!(output.unassessed.len(), 0);
        assert_eq!(
            output.candidates[0].indexed_payload["recipes"][0]["sections"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(!output.candidates[0].verified);
    }

    #[test]
    fn ingredient_component_headings_leave_common_methods_in_an_unnamed_section() {
        let recipe = ancestor(10, "article", "recipe");
        let mut component_profile = profile();
        component_profile.roles.section_name = vec![];
        component_profile.roles.ingredient_section_name = vec![selector("h3", "component")];
        let output = plan(
            vec![indexed(vec![
                (
                    "Layered soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Batter",
                    source_line(
                        1,
                        vec![element(12, "h3", "component", vec![recipe.clone()])],
                    ),
                ),
                (
                    "1 cup flour",
                    source_line(
                        2,
                        vec![element(13, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Sauce",
                    source_line(
                        3,
                        vec![element(14, "h3", "component", vec![recipe.clone()])],
                    ),
                ),
                (
                    "1 cup stock",
                    source_line(
                        4,
                        vec![element(15, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Combine the batter and sauce.",
                    source_line(5, vec![element(16, "p", "method", vec![recipe.clone()])]),
                ),
                (
                    "Simmer until thick.",
                    source_line(6, vec![element(17, "p", "method", vec![recipe])]),
                ),
            ])],
            &component_profile,
        );
        assert_eq!(output.unassessed.len(), 0);
        let sections = &output.candidates[0].indexed_payload["recipes"][0]["sections"];
        assert_eq!(
            sections,
            &json!([
                {"name": [1], "ingredients": [2], "instructions": []},
                {"name": [3], "ingredients": [4], "instructions": []},
                {"name": [], "ingredients": [], "instructions": [5, 6]},
            ])
        );
    }

    #[test]
    fn leading_and_trailing_recipe_methods_share_one_unnamed_component_section() {
        let recipe = ancestor(10, "article", "recipe");
        let mut component_profile = profile();
        component_profile.roles.section_name = vec![];
        component_profile.roles.ingredient_section_name = vec![selector("h3", "component")];
        let output = plan(
            vec![indexed(vec![
                (
                    "Prepared soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Toast the spices before preparing either component.",
                    source_line(1, vec![element(12, "p", "method", vec![recipe.clone()])]),
                ),
                (
                    "Paste",
                    source_line(
                        2,
                        vec![element(13, "h3", "component", vec![recipe.clone()])],
                    ),
                ),
                (
                    "1 tablespoon spices",
                    source_line(
                        3,
                        vec![element(14, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Soup",
                    source_line(
                        4,
                        vec![element(15, "h3", "component", vec![recipe.clone()])],
                    ),
                ),
                (
                    "2 cups stock",
                    source_line(
                        5,
                        vec![element(16, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Combine the prepared components and simmer.",
                    source_line(6, vec![element(17, "p", "method", vec![recipe])]),
                ),
            ])],
            &component_profile,
        );
        assert_eq!(output.unassessed.len(), 0);
        assert_eq!(
            output.candidates[0].indexed_payload["recipes"][0]["sections"],
            json!([
                {"name": [2], "ingredients": [3], "instructions": []},
                {"name": [4], "ingredients": [5], "instructions": []},
                {"name": [], "ingredients": [], "instructions": [1, 6]},
            ])
        );
    }

    #[test]
    fn method_interleaved_with_ingredient_only_components_defers_the_region() {
        let recipe = ancestor(10, "article", "recipe");
        let mut component_profile = profile();
        component_profile.roles.section_name = vec![];
        component_profile.roles.ingredient_section_name = vec![selector("h3", "component")];
        let output = plan(
            vec![indexed(vec![
                (
                    "Two components",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Paste",
                    source_line(
                        1,
                        vec![element(12, "h3", "component", vec![recipe.clone()])],
                    ),
                ),
                (
                    "1 tablespoon spices",
                    source_line(
                        2,
                        vec![element(13, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Grind the spices now.",
                    source_line(3, vec![element(14, "p", "method", vec![recipe.clone()])]),
                ),
                (
                    "Soup",
                    source_line(
                        4,
                        vec![element(15, "h3", "component", vec![recipe.clone()])],
                    ),
                ),
                (
                    "2 cups stock",
                    source_line(5, vec![element(16, "li", "ingredient", vec![recipe])]),
                ),
            ])],
            &component_profile,
        );
        assert!(output.candidates.is_empty());
        assert!(
            output.unassessed[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("interleaved with ingredient-only components"))
        );
    }

    #[test]
    fn ingredient_component_methods_follow_named_components_after_unnamed_ingredients() {
        let recipe = ancestor(10, "article", "recipe");
        let mut component_profile = profile();
        component_profile.roles.section_name = vec![];
        component_profile.roles.ingredient_section_name = vec![selector("h3", "component")];
        let output = plan(
            vec![indexed(vec![
                (
                    "Layered soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "1 onion",
                    source_line(
                        1,
                        vec![element(12, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Sauce",
                    source_line(
                        2,
                        vec![element(13, "h3", "component", vec![recipe.clone()])],
                    ),
                ),
                (
                    "1 cup stock",
                    source_line(
                        3,
                        vec![element(14, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Cook everything together.",
                    source_line(4, vec![element(15, "p", "method", vec![recipe])]),
                ),
            ])],
            &component_profile,
        );
        assert_eq!(output.unassessed.len(), 0);
        assert_eq!(
            output.candidates[0].indexed_payload["recipes"][0]["sections"],
            json!([
                {"name": [], "ingredients": [1], "instructions": []},
                {"name": [2], "ingredients": [3], "instructions": []},
                {"name": [], "ingredients": [], "instructions": [4]},
            ])
        );
    }

    #[test]
    fn ignored_caption_keeps_source_coverage_without_becoming_recipe_notes() {
        let recipe = ancestor(10, "article", "recipe");
        let input = vec![indexed(vec![
            (
                "Example soup",
                source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
            ),
            (
                "1 cup water",
                source_line(
                    1,
                    vec![element(12, "li", "ingredient", vec![recipe.clone()])],
                ),
            ),
            (
                "Stir and serve.",
                source_line(2, vec![element(13, "p", "method", vec![recipe.clone()])]),
            ),
            (
                "Photo shown above",
                source_line(3, vec![element(14, "p", "caption", vec![recipe])]),
            ),
        ])];
        let mut layout = profile();
        assert!(plan(input.clone(), &layout).candidates.is_empty());
        layout.roles.ignored.push(selector("p", "caption"));
        let output = plan(input.clone(), &layout);
        assert_eq!(output.candidates.len(), 1);
        let candidate = &output.candidates[0];
        assert_eq!(candidate.original_lines.len(), 4);
        assert_eq!(candidate.indexed_payload["ignored"], json!([3]));
        assert_eq!(candidate.indexed_payload["recipes"][0]["notes"], json!([]));
        assert_eq!(
            candidate.indexed_payload["recipes"][0]["sections"][0]["instructions"],
            json!([2])
        );
        assert_eq!(candidate.parsed_recipe_count, Some(1));
        assert!(!candidate.verified);

        // An ignore selector cannot override a conflicting method claim.
        layout.roles.method.push(selector("p", "caption"));
        assert!(plan(input, &layout).candidates.is_empty());
    }

    #[test]
    fn profiles_without_ingredient_component_headings_keep_method_before_ingredients() {
        let recipe = ancestor(10, "article", "recipe");
        let output = plan(
            vec![indexed(vec![
                (
                    "Headnote-first soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Heat a pan before preparing the ingredients.",
                    source_line(1, vec![element(12, "p", "method", vec![recipe.clone()])]),
                ),
                (
                    "1 cup stock",
                    source_line(2, vec![element(13, "li", "ingredient", vec![recipe])]),
                ),
            ])],
            &profile(),
        );
        assert_eq!(output.unassessed.len(), 0);
        assert_eq!(
            output.candidates[0].indexed_payload["recipes"][0]["sections"],
            json!([{"name": [], "ingredients": [2], "instructions": [1]}])
        );
    }

    #[test]
    fn general_section_headings_keep_interleaved_methods_with_their_sections() {
        let recipe = ancestor(10, "article", "recipe");
        let output = plan(
            vec![indexed(vec![
                (
                    "Two preparations",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Filling",
                    source_line(1, vec![element(12, "h3", "section", vec![recipe.clone()])]),
                ),
                (
                    "1 cup greens",
                    source_line(
                        2,
                        vec![element(13, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Wilt the greens.",
                    source_line(3, vec![element(14, "p", "method", vec![recipe.clone()])]),
                ),
                (
                    "Dressing",
                    source_line(4, vec![element(15, "h3", "section", vec![recipe.clone()])]),
                ),
                (
                    "1 tablespoon oil",
                    source_line(
                        5,
                        vec![element(16, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Whisk the dressing.",
                    source_line(6, vec![element(17, "p", "method", vec![recipe])]),
                ),
            ])],
            &profile(),
        );
        assert_eq!(output.unassessed.len(), 0);
        assert_eq!(
            output.candidates[0].indexed_payload["recipes"][0]["sections"],
            json!([
                {"name": [1], "ingredients": [2], "instructions": [3]},
                {"name": [4], "ingredients": [5], "instructions": [6]},
            ])
        );
    }

    #[test]
    fn paired_section_subtitles_preserve_parent_and_variation_method_ownership() {
        let recipe = ancestor(10, "article", "recipe");
        let mut variation_profile = profile();
        variation_profile.roles.section_name = vec![selector("h3", "variation-title")];
        variation_profile.roles.section_subtitle = vec![selector("h4", "variation-subtitle")];
        let output = plan(
            vec![indexed(vec![
                (
                    "Two sauces",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Toast the spices before preparing either sauce.",
                    source_line(1, vec![element(12, "p", "method", vec![recipe.clone()])]),
                ),
                (
                    "Red Sauce",
                    source_line(
                        2,
                        vec![element(13, "h3", "variation-title", vec![recipe.clone()])],
                    ),
                ),
                (
                    "nam phrik daeng",
                    source_line(
                        3,
                        vec![element(
                            14,
                            "h4",
                            "variation-subtitle",
                            vec![recipe.clone()],
                        )],
                    ),
                ),
                (
                    "1 red chile",
                    source_line(
                        4,
                        vec![element(15, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Blend the red sauce.",
                    source_line(5, vec![element(16, "p", "method", vec![recipe.clone()])]),
                ),
                (
                    "Green Sauce",
                    source_line(
                        6,
                        vec![element(17, "h3", "variation-title", vec![recipe.clone()])],
                    ),
                ),
                (
                    "nam phrik khiao",
                    source_line(
                        7,
                        vec![element(
                            18,
                            "h4",
                            "variation-subtitle",
                            vec![recipe.clone()],
                        )],
                    ),
                ),
                (
                    "1 green chile",
                    source_line(
                        8,
                        vec![element(19, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Blend the green sauce.",
                    source_line(9, vec![element(20, "p", "method", vec![recipe])]),
                ),
            ])],
            &variation_profile,
        );
        assert_eq!(output.unassessed.len(), 0);
        assert_eq!(output.candidates[0].parsed_recipe_count, Some(1));
        assert_eq!(
            output.candidates[0].indexed_payload["recipes"][0]["sections"],
            json!([
                {"name": [], "ingredients": [], "instructions": [1]},
                {"name": [2, 3], "ingredients": [4], "instructions": [5]},
                {"name": [6, 7], "ingredients": [8], "instructions": [9]},
            ])
        );
    }

    #[test]
    fn malformed_section_subtitles_and_consecutive_primary_headings_defer() {
        let recipe = ancestor(10, "article", "recipe");
        let mut variation_profile = profile();
        variation_profile.roles.section_name = vec![selector("h3", "variation-title")];
        variation_profile.roles.section_subtitle = vec![selector("h4", "variation-subtitle")];
        for (middle, reason) in [
            (
                vec![
                    ("nam phrik", "h4", "variation-subtitle"),
                    ("1 chile", "li", "ingredient"),
                ],
                "orphan or repeated section subtitle",
            ),
            (
                vec![
                    ("Red", "h3", "variation-title"),
                    ("nam phrik", "h4", "variation-subtitle"),
                    ("daeng", "h4", "variation-subtitle"),
                    ("1 chile", "li", "ingredient"),
                ],
                "orphan or repeated section subtitle",
            ),
            (
                vec![
                    ("Red", "h3", "variation-title"),
                    ("Green", "h3", "variation-title"),
                    ("1 chile", "li", "ingredient"),
                ],
                "consecutive section-name lines",
            ),
        ] {
            let mut lines = vec![(
                "Sauce",
                source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
            )];
            for (index, (text, tag, class)) in middle.into_iter().enumerate() {
                lines.push((
                    text,
                    source_line(
                        index + 1,
                        vec![element(index + 12, tag, class, vec![recipe.clone()])],
                    ),
                ));
            }
            let output = plan(vec![indexed(lines)], &variation_profile);
            assert!(output.candidates.is_empty());
            assert!(
                output.unassessed[0]
                    .reasons
                    .iter()
                    .any(|entry| entry.contains(reason))
            );
        }
    }

    #[test]
    fn mixed_general_and_ingredient_only_headings_defer_the_whole_region() {
        let recipe = ancestor(10, "article", "recipe");
        let mut mixed_profile = profile();
        mixed_profile.roles.section_name = vec![selector("h3", "variant")];
        mixed_profile.roles.ingredient_section_name = vec![selector("h3", "component")];
        let output = plan(
            vec![indexed(vec![
                (
                    "Mixed soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Paste",
                    source_line(
                        1,
                        vec![element(12, "h3", "component", vec![recipe.clone()])],
                    ),
                ),
                (
                    "1 chile",
                    source_line(
                        2,
                        vec![element(13, "li", "ingredient", vec![recipe.clone()])],
                    ),
                ),
                (
                    "Variation",
                    source_line(3, vec![element(14, "h3", "variant", vec![recipe.clone()])]),
                ),
                (
                    "Blend the chile.",
                    source_line(4, vec![element(15, "p", "method", vec![recipe])]),
                ),
            ])],
            &mixed_profile,
        );
        assert!(output.candidates.is_empty());
        assert_eq!(output.unassessed.len(), 1);
        assert!(output.unassessed[0].reasons.iter().any(|reason| {
            reason.contains("mixes general and ingredient-only section headings")
        }));
    }

    #[test]
    fn repeated_text_with_distinct_recipe_owners_stays_distinct() {
        let first = ancestor(10, "article", "recipe");
        let second = ancestor(20, "article", "recipe");
        let output = plan(
            vec![indexed(vec![
                (
                    "Example soup",
                    source_line(0, vec![element(11, "h2", "title", vec![first.clone()])]),
                ),
                (
                    "1 cup water",
                    source_line(1, vec![element(12, "li", "ingredient", vec![first])]),
                ),
                (
                    "Example soup",
                    source_line(2, vec![element(21, "h2", "title", vec![second.clone()])]),
                ),
                (
                    "1 cup water",
                    source_line(3, vec![element(22, "li", "ingredient", vec![second])]),
                ),
            ])],
            &profile(),
        );
        assert_eq!(output.candidates.len(), 2);
        assert_eq!(
            output
                .candidates
                .iter()
                .map(|candidate| candidate.region_element_index)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([10, 20])
        );
    }

    #[test]
    fn region_chunk_preserves_indexed_line_links_and_images() {
        let recipe = ancestor(10, "article", "recipe");
        let mut title = source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]);
        title.images = vec![ImageRef {
            path: "images/soup.jpg".into(),
            mime: "image/jpeg".into(),
            alt: Some("Soup".into()),
        }];
        let mut ingredient = source_line(1, vec![element(12, "li", "ingredient", vec![recipe])]);
        ingredient.links = vec![Link {
            text: "stock".into(),
            href: "stock.xhtml#base".into(),
        }];
        let output = plan(
            vec![indexed(vec![("Soup", title), ("1 cup stock", ingredient)])],
            &profile(),
        );
        let chunk = &output.candidates[0].region_chunk;
        assert_eq!(chunk.links[0].href, "stock.xhtml#base");
        assert_eq!(
            chunk.images,
            vec![(
                0,
                ImageRef {
                    path: "images/soup.jpg".into(),
                    mime: "image/jpeg".into(),
                    alt: Some("Soup".into()),
                }
            )]
        );
    }

    #[test]
    fn unknown_recipe_owned_line_defers_its_whole_region() {
        let recipe = ancestor(10, "article", "recipe");
        let output = plan(
            vec![indexed(vec![
                (
                    "Example soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Unmapped content",
                    source_line(1, vec![element(12, "p", "unmapped", vec![recipe.clone()])]),
                ),
                (
                    "1 cup water",
                    source_line(2, vec![element(13, "li", "ingredient", vec![recipe])]),
                ),
            ])],
            &profile(),
        );
        assert!(output.candidates.is_empty());
        assert_eq!(output.unassessed.len(), 1);
        assert_eq!(output.unassessed[0].scope, "recipe_region");
        assert_eq!(output.unassessed[0].original_lines.len(), 3);
        assert!(output.unassessed[0].reasons[0].contains("unknown recipe-owned line"));
    }

    #[test]
    fn multi_container_line_defers_each_region_without_duplicate_source_coordinates() {
        let first = ancestor(10, "article", "recipe");
        let second = ancestor(20, "article", "recipe");
        let output = plan(
            vec![indexed(vec![
                (
                    "First soup",
                    source_line(0, vec![element(11, "h2", "title", vec![first.clone()])]),
                ),
                (
                    "Shared headnote",
                    source_line(
                        1,
                        vec![
                            element(12, "p", "headnote", vec![first.clone()]),
                            element(22, "p", "headnote", vec![second.clone()]),
                        ],
                    ),
                ),
                (
                    "1 cup water",
                    source_line(2, vec![element(13, "li", "ingredient", vec![first])]),
                ),
                (
                    "Second soup",
                    source_line(3, vec![element(21, "h2", "title", vec![second.clone()])]),
                ),
                (
                    "1 cup water",
                    source_line(4, vec![element(23, "li", "ingredient", vec![second])]),
                ),
            ])],
            &profile(),
        );

        assert!(output.candidates.is_empty());
        let coordinates = all_original_coordinates(&output);
        assert_eq!(coordinates, vec![(0, 0), (0, 1), (0, 2), (0, 3), (0, 4)]);
        assert_eq!(
            coordinates.len(),
            coordinates.iter().collect::<BTreeSet<_>>().len()
        );
        assert!(output.unassessed.iter().any(|entry| {
            entry.scope == "ambiguous_container"
                && entry.original_lines.len() == 1
                && entry.original_lines[0].line_index == 1
        }));
        for region in [10, 20] {
            assert!(output.unassessed.iter().any(|entry| {
                entry.scope == "recipe_region"
                    && entry.region_element_index == Some(region)
                    && entry.excluded_original_lines
                        == vec![OriginalLineCoordinate {
                            original_chunk_index: 0,
                            line_index: 1,
                            document_line: 1,
                        }]
                    && entry
                        .reasons
                        .iter()
                        .any(|reason| reason.contains("multiple recipe containers"))
            }));
        }
    }

    #[test]
    fn nested_recipe_containers_defer_parent_and_child_regions() {
        let parent = ancestor(10, "article", "recipe");
        let child = ancestor(20, "article", "recipe");
        let output = plan(
            vec![indexed(vec![
                (
                    "Example soup",
                    source_line(0, vec![element(11, "h2", "title", vec![parent.clone()])]),
                ),
                (
                    "1 cup water",
                    source_line(
                        1,
                        vec![element(12, "li", "ingredient", vec![parent.clone()])],
                    ),
                ),
                (
                    "Stir the soup.",
                    source_line(2, vec![element(13, "p", "method", vec![parent.clone()])]),
                ),
                (
                    "Cover the pot.",
                    source_line(3, vec![element(21, "p", "method", vec![parent, child])]),
                ),
            ])],
            &profile(),
        );

        assert!(output.candidates.is_empty());
        assert_eq!(
            all_original_coordinates(&output),
            vec![(0, 0), (0, 1), (0, 2), (0, 3)]
        );
        assert!(output.unassessed.iter().any(|entry| {
            entry.scope == "ambiguous_container"
                && entry
                    .original_lines
                    .first()
                    .is_some_and(|line| line.line_index == 3)
        }));
        for region in [10, 20] {
            assert!(output.unassessed.iter().any(|entry| {
                entry.scope == "recipe_region"
                    && entry.region_element_index == Some(region)
                    && entry.excluded_original_lines
                        == vec![OriginalLineCoordinate {
                            original_chunk_index: 0,
                            line_index: 3,
                            document_line: 3,
                        }]
                    && entry
                        .reasons
                        .iter()
                        .any(|reason| reason.contains("multiple recipe containers"))
            }));
        }
    }

    #[test]
    fn profile_rejects_unknown_fields_and_required_guard_defers_region() {
        let invalid = json!({
            "schema_version": 1,
            "epub_sha256": "0".repeat(64),
            "recipe_container": {"tag": "article", "class": "recipe"},
            "roles": {
                "title": [], "description": [], "ingredient": [], "method": [],
                "section_name": [], "recipe_yield": [], "notes": []
            },
            "required": [],
            "defer_if": [],
            "unexpected": true
        });
        assert!(serde_json::from_value::<Profile>(invalid).is_err());

        let recipe = ancestor(10, "article", "recipe");
        let mut guarded = profile();
        guarded.required = vec![Requirement {
            selector: selector("span", "yield"),
            min: 1,
            max: None,
        }];
        let output = plan(
            vec![indexed(vec![
                (
                    "Example soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "1 cup water",
                    source_line(1, vec![element(12, "li", "ingredient", vec![recipe])]),
                ),
            ])],
            &guarded,
        );
        assert!(output.candidates.is_empty());
        assert!(output.unassessed.iter().any(|entry| {
            entry.scope == "recipe_region"
                && entry
                    .reasons
                    .iter()
                    .any(|reason| reason.contains("required selector 0 matched 0 elements"))
        }));
    }

    #[test]
    fn residual_assignment_only_completes_unknown_line_and_preserves_evidence() {
        let recipe = ancestor(10, "article", "recipe");
        let input = vec![indexed(vec![
            (
                "Example soup",
                source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
            ),
            (
                "Cover the pot.",
                source_line(1, vec![element(12, "p", "unmapped", vec![recipe])]),
            ),
        ])];
        let baseline = plan(input.clone(), &profile());
        assert!(baseline.candidates.is_empty());

        let mut assignments = AssignmentMap::new();
        assignments.insert(
            OriginalLineCoordinate {
                original_chunk_index: 0,
                line_index: 1,
                document_line: 1,
            },
            Some(Role::Method),
        );
        let completed = plan_with_assignments(input, &profile(), &assignments);
        assert_eq!(completed.output.candidates.len(), 1);
        assert_eq!(
            completed.applied_assignments,
            BTreeSet::from([OriginalLineCoordinate {
                original_chunk_index: 0,
                line_index: 1,
                document_line: 1,
            }])
        );
        assert!(!completed.output.candidates[0].verified);
    }

    #[test]
    fn null_role_assignment_remains_deferred_without_application() {
        let test_profile = profile();
        let bytes = assignment_bytes(
            "epub",
            "profile",
            json!([{
                "original_chunk_index": 0,
                "line_index": 1,
                "document_line": 1,
                "role": null,
            }]),
        );
        let (_document, assignments) = load_role_assignments(
            &bytes,
            "epub",
            "profile",
            &unknown_role_input(),
            &test_profile,
        )
        .unwrap();
        let coordinate = OriginalLineCoordinate {
            original_chunk_index: 0,
            line_index: 1,
            document_line: 1,
        };
        assert_eq!(assignments.get(&coordinate), Some(&None));
        let result = plan_with_assignments(unknown_role_input(), &test_profile, &assignments);
        assert!(result.output.candidates.is_empty());
        assert!(result.applied_assignments.is_empty());
        assert_eq!(result.residual_regions.len(), 1);
    }

    #[test]
    fn role_assignment_loader_rejects_pinning_coordinate_and_role_conflicts() {
        let test_profile = profile();
        let assignment = json!({
            "original_chunk_index": 0,
            "line_index": 1,
            "document_line": 1,
            "role": "method",
        });
        assert!(
            load_role_assignments(
                &assignment_bytes("wrong", "profile", json!([assignment.clone()])),
                "epub",
                "profile",
                &unknown_role_input(),
                &test_profile,
            )
            .is_err()
        );
        assert!(
            load_role_assignments(
                &assignment_bytes("epub", "wrong-profile", json!([assignment.clone()])),
                "epub",
                "profile",
                &unknown_role_input(),
                &test_profile,
            )
            .is_err()
        );
        assert!(
            load_role_assignments(
                &assignment_bytes(
                    "epub",
                    "profile",
                    json!([{
                        "original_chunk_index": 0,
                        "line_index": 1,
                        "document_line": 99,
                        "role": "method",
                    }]),
                ),
                "epub",
                "profile",
                &unknown_role_input(),
                &test_profile,
            )
            .is_err()
        );
        assert!(
            load_role_assignments(
                &assignment_bytes(
                    "epub",
                    "profile",
                    json!([{
                        "original_chunk_index": 9,
                        "line_index": 0,
                        "document_line": 0,
                        "role": "method",
                    }]),
                ),
                "epub",
                "profile",
                &unknown_role_input(),
                &test_profile,
            )
            .is_err()
        );
        assert!(
            load_role_assignments(
                &assignment_bytes("epub", "profile", json!([assignment.clone(), assignment])),
                "epub",
                "profile",
                &unknown_role_input(),
                &test_profile,
            )
            .is_err()
        );
        assert!(
            load_role_assignments(
                &assignment_bytes(
                    "epub",
                    "profile",
                    json!([{
                        "original_chunk_index": 0,
                        "line_index": 0,
                        "document_line": 0,
                        "role": "method",
                    }]),
                ),
                "epub",
                "profile",
                &unknown_role_input(),
                &test_profile,
            )
            .is_err()
        );

        let recipe = ancestor(10, "article", "recipe");
        let conflicting_input = vec![indexed(vec![
            (
                "Example soup",
                source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
            ),
            (
                "Conflicting prose",
                source_line(1, vec![element(12, "p", "ambiguous", vec![recipe])]),
            ),
        ])];
        let mut conflicting_profile = profile();
        conflicting_profile.roles.description = vec![selector("p", "ambiguous")];
        conflicting_profile.roles.method = vec![selector("p", "ambiguous")];
        assert!(
            load_role_assignments(
                &assignment_bytes(
                    "epub",
                    "profile",
                    json!([{
                        "original_chunk_index": 0,
                        "line_index": 1,
                        "document_line": 1,
                        "role": "method",
                    }]),
                ),
                "epub",
                "profile",
                &conflicting_input,
                &conflicting_profile,
            )
            .is_err()
        );
    }

    #[test]
    fn residual_output_excludes_structurally_deferred_regions() {
        let recipe = ancestor(10, "article", "recipe");
        let output = plan_with_assignments(
            vec![indexed(vec![
                (
                    "Example soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Unmapped prose",
                    source_line(1, vec![element(12, "p", "unmapped", vec![recipe.clone()])]),
                ),
                (
                    "Rendered row",
                    SourceLine {
                        anchors: Vec::new(),
                        transformed: true,
                        ..source_line(2, vec![element(13, "p", "unmapped", vec![recipe])])
                    },
                ),
            ])],
            &profile(),
            &AssignmentMap::new(),
        );
        assert!(output.residual_regions.is_empty());
        assert!(output.output.candidates.is_empty());
    }

    #[test]
    fn conflicting_role_line_cannot_be_completed_by_residual_assignment() {
        let recipe = ancestor(10, "article", "recipe");
        let mut conflicted = profile();
        conflicted.roles.description = vec![selector("p", "ambiguous")];
        conflicted.roles.method = vec![selector("p", "ambiguous")];
        let mut assignments = AssignmentMap::new();
        assignments.insert(
            OriginalLineCoordinate {
                original_chunk_index: 0,
                line_index: 1,
                document_line: 1,
            },
            Some(Role::Method),
        );
        let output = plan_with_assignments(
            vec![indexed(vec![
                (
                    "Example soup",
                    source_line(0, vec![element(11, "h2", "title", vec![recipe.clone()])]),
                ),
                (
                    "Conflicting line",
                    source_line(1, vec![element(12, "p", "ambiguous", vec![recipe])]),
                ),
            ])],
            &conflicted,
            &assignments,
        );
        assert!(output.output.candidates.is_empty());
        assert!(output.applied_assignments.is_empty());
        assert!(output.residual_regions.is_empty());
        assert!(
            output.output.unassessed[0]
                .reasons
                .iter()
                .any(|reason| reason.contains("conflicting roles"))
        );
    }
}
