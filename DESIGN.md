---
name: ingredient-parser Maintainer Toolkit
description: Dense native workspaces for parser inspection and cookbook review.
colors:
  dark-base: "#1e1f22"
  dark-mantle: "#252629"
  dark-crust: "#18191c"
  dark-surface0: "#2a2b2f"
  dark-surface1: "#3c3e43"
  dark-surface2: "#4a4d53"
  dark-overlay1: "#9499a3"
  dark-subtext0: "#adb1bb"
  dark-text: "#ebedf1"
  dark-blue: "#70b1ff"
  dark-lavender: "#8fbcff"
  dark-green: "#73cf97"
  dark-yellow: "#edc663"
  dark-peach: "#e1b382"
  dark-red: "#ff8a8a"
  light-base: "#f9f9fb"
  light-mantle: "#eff0f3"
  light-crust: "#ffffff"
  light-surface0: "#eceef2"
  light-surface1: "#d4d7de"
  light-surface2: "#c1c8d3"
  light-overlay1: "#666d7b"
  light-subtext0: "#5c6371"
  light-text: "#23272f"
  light-blue: "#195bad"
  light-lavender: "#225cad"
  light-green: "#1b7341"
  light-yellow: "#825b0c"
  light-peach: "#8b5321"
  light-red: "#b52a2f"
typography:
  body:
    fontFamily: "SF Pro, Helvetica Neue, Arial, egui proportional"
    fontSize: "14pt"
  label:
    fontFamily: "SF Pro, Helvetica Neue, Arial, egui proportional"
    fontSize: "14pt"
  title:
    fontFamily: "SF Pro, Helvetica Neue, Arial, egui proportional"
    fontSize: "22pt"
  mono:
    fontFamily: "SF Mono, Menlo, egui monospace"
    fontSize: "13pt"
  small:
    fontFamily: "SF Pro, Helvetica Neue, Arial, egui proportional"
    fontSize: "12pt"
  workspace:
    fontFamily: "SF Pro, Helvetica Neue, Arial, egui proportional"
    fontSize: "24pt"
  run:
    fontFamily: "SF Pro, Helvetica Neue, Arial, egui proportional"
    fontSize: "20pt"
  pane:
    fontFamily: "SF Pro, Helvetica Neue, Arial, egui proportional"
    fontSize: "17pt"
rounded:
  control: "4pt"
  group: "3pt"
spacing:
  item-horizontal: "8pt"
  item-vertical: "5pt"
  button-horizontal: "8pt"
  button-vertical: "4pt"
  group-inset: "8pt"
  sidebar-inset: "12pt"
  workspace-inset: "16pt"
  evidence-inset: "16pt"
components:
  button:
    rounded: "{rounded.control}"
    typography: "{typography.label}"
    padding: "4pt 8pt"
  button-primary:
    rounded: "{rounded.control}"
    typography: "{typography.label}"
    height: "32pt"
  workspace-navigation:
    typography: "{typography.label}"
    height: "36pt"
  group:
    rounded: "{rounded.group}"
    padding: "{spacing.group-inset}"
---

# Design System: ingredient-parser Maintainer Toolkit

## Overview

**Creative North Star: Native developer tools — Xcode and Instruments.**

The application is a compact egui desktop workbench. Parser and Cookbooks have equal standing in a persistent navigation rail. System typography, neutral light and dark surfaces, and resizable working panes keep source material, results, and inspection evidence close together.

The visual character is restrained and practical, with a clear hierarchy from workspace to run to selected document and evidence. Distinct inset surfaces, readable text, compact controls, and diagnostic color organize the work. Advanced actions live in contextual menus; inspectors and disclosure sections reveal supporting evidence.

**Key Characteristics:**

- Dense native controls and system typography.
- Neutral surfaces with semantic color.
- Resizable panes and context-preserving selection.
- Explicit source-loading and extraction actions.

## Colors

The frontmatter records both palettes from `food-app/src/theme.rs`. Dark is the initial appearance; the navigation rail switches between light and dark. The serialized names `Mocha` and `Latte` identify these neutral palettes.

### Primary

Blue identifies ingredient names and graph hubs. Lavender identifies hyperlinks and active control outlines. Selection uses blue with alpha 28/255 and a one-point lavender stroke; it remains translucent over the current surface.

### Secondary

Green means a complete trace match; yellow means an incomplete match; red means a failed trace or error. Peach identifies ingredient amounts. Secondary metadata uses subtext rather than an additional accent.

### Neutral

`base` fills the working area; `mantle` distinguishes navigation, inspectors, and windows; `crust` distinguishes source evidence and provides the extreme background. `surface0` fills inactive controls and grouped content. `surface1` provides borders and hover/open fills; `surface2` fills active controls. `overlay1` outlines hovered and open controls. `text` is the main foreground and `subtext0` supports secondary information.

**Semantic color rule.** Reuse palette accessors for parser and graph roles so both appearances remain coherent. Color supports the result or label already visible in the control.

## Typography

All dimensions use **egui logical points**, not CSS pixels or typographic print points. The frontmatter's `pt` strings represent these native logical units; do not convert them into browser sizing rules.

Body and buttons use the body/label roles; section headings use title; input source, trace, and structured evidence use mono. macOS fonts are loaded from the operating system in priority order: SFNS, Helvetica Neue, then Arial for proportional text; SFNSMono, then Menlo for code. egui embedded fonts remain the fallback when these files are unavailable. Material Symbols supply inline affordance icons.

Workspace titles and the selected cookbook document use the workspace role. Run titles and recipe/source headings use the run role; pane headings use the pane role. These headings use strong text. Small text has an explicit small role; weak text marks paths and secondary metadata. The theme does not establish a fixed line-height scale or custom font-weight scale.

## Layout

The shell has a fixed-width left workspace rail (176 points) with full-width navigation buttons (36 points high). Parser source modes are Ingredients, Recipe URL, and Corpus. Workspace and central frames use the workspace inset; navigation and inspectors use the sidebar inset; source evidence uses the evidence inset.

Item spacing and button padding follow the frontmatter. The standard interaction height is 28 points. Parser result rows are body size plus 8 points (22 points with the current theme), with a 22-point header. Resizable columns and independent scroll areas support dense datasets without flattening the heading hierarchy.

Pane defaults and thresholds are measured against the **local available width**, after surrounding panes consume space; they are not whole-window breakpoints:

| Context | Wide arrangement | Compact arrangement |
| --- | --- | --- |
| Parser ingredient inspector | Right pane, default 380 points | Below 900 points: bottom pane, default 260 points |
| Cookbook ingredient inspector | Right pane, default 330 points | Below 1050 points, when the focused mode below does not apply: bottom pane, default 185 points; maximum is 42% of available height with a 130-point floor |
| Cookbook document navigation | Left pane, default 220 points, resizable within 170–300 points | Below 760 points: Documents menu, content width 260 points |
| Cookbook source/result comparison | Resizable source pane, initially 45% of comparison width | Below 650 points: Source document / Extracted result tabs show one surface at a time |
| Recipe ingredients/instructions | Two equal columns | Below 640 points: stacked with section labels |

When an ingredient is selected and the available content width is below 760 points **or** height is below 480 points, a focused ingredient inspector replaces the document content. The focused view shows Back to review and the selected document title, and retains the inspector’s close control. Either exit returns to the same selected document. This takes precedence over the side/bottom inspector arrangement.

Below the 650-point comparison threshold, tabs replace the columns and serve as pane labels, so the repeated Source document and Extracted result headings are suppressed. An extracted recipe title matching the selected document title is also suppressed in this compact view; distinct titles remain visible.

The comparison source width is constrained to 30–65% of the comparison area. `theme::comparison_panel` preserves the user's split fraction when resizing the window or opening an inspector changes the available width, rather than retaining a stale absolute pane width.

When the run header has less than 760 points of available width or 580 points of remaining height, its recipe/document counts appear as hover text on the book title instead of a separate summary row. The gap after this header shrinks from 16 to 4 points.

The optional cookbook library is a resizable left pane with default width 250 points. Recipe ingredient navigation is a resizable left pane with default width 260 points. Long source and result content scroll independently.

## Elevation & Depth

The mantle sidebar, base workspace, and crust source-evidence surfaces establish structural depth with their own insets. Separators and one-point control strokes reinforce this hierarchy. The theme defines no custom shadow or motion vocabulary; egui's inherited window and popup behavior remains in effect. Grouped content uses a subtly contrasting surface and border rather than decorative elevation.

## Shapes

Controls and windows have restrained corners using the control radius. Group frames use the smaller group radius and group inset. Panes and tabular data remain rectangular structures. Reuse `theme::card` for existing inset content groups; avoid wrapping every pane in another container.

## Components

### Buttons and navigation

Compact egui buttons use inactive, hovered, active, and open palette states. All four states share the control radius and foreground color. Parser and Cookbooks use equal full-width navigation buttons with a selected frame; inactive entries remain quiet. Source-mode controls use selectable labels. The appearance action sits at the bottom of the rail.

Primary actions such as Save review use a blue fill, strong text, and a minimum height of 32 points. Their foreground is crust in dark appearance and white in light appearance. Keep this emphasis for the local completion action.

### Inputs and explicit actions

Ingredient input is a four-row monospaced editor with a Parse action and Command/Control+Enter shortcut; ordinary Enter inserts a line. Editing input clears the previous results. Recipe URL loading has an explicit action, pending state, and retry guidance. Cookbooks groups EPUB, saved-run, library-folder, and path opening under Open. The run header places Extraction and Run tools menus beside the run summary. Document search and the Filter menu belong inside document navigation. Review status and Save review belong under the selected document title. At comparison widths below 650 points, Review note is an inline menu beside these controls; at wider widths it is a separate collapsing row. Cookbook opening and extraction remain separate operations. Standard egui focus and disabled behavior are inherited rather than restyled with a custom focus system.

### Tables, lists, and inspectors

Parser results use striped rows, selected-row highlighting, sortable headers, and clipped resizable columns. Input initially receives 30% of available table width and Name receives 22%, prioritizing the source text while preserving resizable columns. Arrow navigation moves the selected result when text input does not own the keyboard. Contextual inspectors reveal trace and structured evidence for the current selection. Cookbook document navigation connects source evidence with extracted recipes and review decisions.

### Groups and disclosure

Inset content groups use `surface0` and a one-point `surface1` border. Collapsing sections expose advanced or raw evidence without increasing default density. Errors appear inline in the semantic error color; pending work uses a spinner and a descriptive label. Empty states describe the next concrete action.

## Do's and Don'ts

- **Do** preserve the equal standing of Parser and Cookbooks.
- **Do** use the shared theme and existing egui controls for new surfaces.
- **Do** keep local-width pane transitions and independent scrolling intact.
- **Do** keep selected source, computed result, and evidence visibly connected.
- **Don't** introduce web landing-page typography or decorative dashboard cards into the native workbench.
- **Don't** use semantic diagnostic colors as unrelated decoration.
- **Don't** make opening a source or restoring a session initiate extraction.
