---
name: ingredient-parser Maintainer Toolkit
description: Native developer-tool workspaces built with Tauri, React, and TypeScript.
colors:
  dark-base: "#1e1f22"
  dark-mantle: "#252629"
  dark-evidence: "#18191c"
  dark-control: "#2a2b2f"
  dark-hover: "#3c3e43"
  dark-border: "#3c3e43"
  dark-text: "#ebedf1"
  dark-muted: "#adb1bb"
  dark-blue: "#70b1ff"
  dark-green: "#73cf97"
  dark-yellow: "#edc663"
  dark-red: "#ff8a8a"
  dark-amount: "#e1b382"
  dark-selection: "rgba(112, 177, 255, 0.11)"
  dark-primary-ink: "#18191c"
  light-base: "#f9f9fb"
  light-mantle: "#eff0f3"
  light-evidence: "#fff"
  light-control: "#eceef2"
  light-hover: "#d4d7de"
  light-border: "#d4d7de"
  light-text: "#23272f"
  light-muted: "#5c6371"
  light-blue: "#195bad"
  light-green: "#1b7341"
  light-yellow: "#825b0c"
  light-red: "#b52a2f"
  light-amount: "#8b5321"
  light-selection: "rgba(25, 91, 173, 0.11)"
  light-primary-ink: "#fff"
typography:
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "14px"
  workspace:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI, sans-serif"
    fontSize: "24px"
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace"
    fontSize: "13px"
rounded:
  control: "5px"
  input: "4px"
spacing:
  workspace-inset: "20px"
  workspace-gap: "16px"
  evidence-inset: "16px"
  inspector-inset: "14px"
---

# Design System: ingredient-parser Maintainer Toolkit

## Direction and platform

**Creative North Star: Native developer tools — Xcode and Instruments.**

This is an Operate surface: a macOS Tauri 2 application with a React/TypeScript
webview and portable Rust application services. The interface keeps Parser and
Cookbooks equally prominent. System typography, neutral surfaces, resizable
working panes, and contextual evidence support daily maintenance work.

Hierarchy runs from workspace to cookbook/run to selected document to evidence.
Opening, extraction, and advanced tools have separate, explicit controls. Keep
source material, computed results, and review decisions visibly distinct.

The implementation authority is `food-app/ui/src/style.css`, together with
`components.tsx`, `Inspector.tsx`, `Parser.tsx`, and `Cookbooks.tsx`. Dimensions
below are CSS pixels inside the native webview.

## Color and surfaces

The frontmatter records the current CSS custom properties for both appearances.
`base` is the work area, `mantle` holds navigation and inspectors, and `evidence`
provides the source/editor surface. `control`, `hover`, and `border` establish
compact control states. Avoid adding decorative cards around working panes.

Blue marks primary actions, ingredient names, links, and keyboard focus. Green,
yellow, and red communicate diagnostic outcomes with accompanying text. Amounts
use the amount token; metadata uses muted text. Selection uses an 11% blue fill
in both appearances, preserving text contrast. Primary buttons use the dedicated
primary-ink token. Do not reuse diagnostic colors for unrelated decoration.

## Typography and spacing

The system sans stack uses 14px body text. Workspace titles are 24px/650 with
-0.025em tracking; selected document titles are 24px, reduced to 22px in compact
composition. Run titles are 20px, reduced to 18px below 950px viewport width.
Pane headings use 17px/600; recipe titles use 20px/600; section labels use 15px.
Captions and table cells are 12px. Input, trace, and JSON evidence use 13px system
monospace. Body copy uses 1.5 line height; instructions use 1.6.

Workspace inset is 20px with a 16px gap, reducing to 16px/12px below 950px.
Source and result panes have 16px inset, reduced to 12px in compact comparison.
Inspector inset is 14px. Controls use a 28px minimum height, 4px 9px padding,
and 5px corners; primary controls have a 32px minimum height. Inputs use 4px
corners. Group content by proximity, reserving larger gaps for semantic sections.

## Shell, panes, and compact composition

The 176px navigation rail uses the mantle surface and 36px workspace rows with
icons and text on one line. The active workspace uses selection fill and
`aria-current`. Appearance is anchored at the bottom. Below 720px viewport
width, the rail becomes 130px; the native window minimum is 800px wide, while
browser QA can exercise the narrower fallback.

`Split` stores bounded percentage sizes per pane identity. Its 7px handle has a
one-pixel separator, pointer capture for dragging, and keyboard support: arrows
move by two percentage points, Home/End choose the bounds. Preserve independent
scroll regions and `min-height: 0` so long evidence remains reachable.

| Context | Arrangement |
| --- | --- |
| Parser results/inspector | Starts 65% results, bounded 40–75%; below 1150px viewport width stacks vertically and hides the horizontal handle. |
| Cookbook document navigation | Starts 22%, bounded 16–30%; below 760px measured workspace width becomes a Documents menu. |
| Cookbook source/result | Starts 45% source, bounded 30–65%; a 650px comparison container query replaces the split with Source document / Extracted result tabs. |
| Cookbook inspector | 310px wide beside review; below 1150px viewport width becomes a bottom region when focused mode is not active. |
| Focused cookbook inspection | Below 760px measured workspace width or 480px measured workspace height, ingredient selection shows a focused inspector with Back to review and selected-document context. |
| Recipe URL content | Ingredient/instruction columns stack below 1000px viewport width. |

Viewport media queries, comparison container queries, and measured workspace
thresholds are distinct; do not substitute one for another. Compact comparison
hides redundant pane headings. The renderer suppresses matching recipe titles
below 850px measured workspace width while retaining distinct titles. Below
950px viewport width, run counts disappear and controls become more compact.

## Components and interaction

The shared `Inspector` provides Fields, Stages, Trace tree, and JSON for selected
ingredients. Its mantle surface separates it from evidence; the input stays
visible above the tabs. Fields use a two-column grid with muted labels and
wrapping values. Copy actions report success or failure. Trace nodes use
expandable details with diagnostic labels as well as color.

Tabs use a roving tab stop and left/right arrow navigation. Virtual lists expose
listbox/option semantics, selection, and Up/Down/Home/End navigation. Parser
columns prioritize Input and Name, with initial proportions 30/22/17/18/13 and
resizable handles. Preserve selection as data views change, and keep stale-result
feedback explicit rather than presenting old computation as current input.

Menus use compact disclosures, close on outside pointer interaction, and return
focus to their summary on Escape. Keyboard focus uses a two-pixel blue outline
with a two-pixel offset. Native dialogs and app menus belong to the Tauri shell;
the extraction form uses a modal HTML dialog. Errors use inline alerts, pending
work has labeled progress, and empty states explain a concrete next action.
Spinner animation is disabled for reduced-motion preferences.

## Source evidence and actions

Render imported titles, source blocks, recipe fields, traces, and JSON as text
through React. Do not inject EPUB or recipe HTML. Source heading tags choose
presentation elements without making source markup executable. Image content
comes through explicit Rust image-loading commands and uses returned data URLs;
source paths remain visible with unloaded-image placeholders when applicable.

Opening a book or saved run never starts extraction. Extraction exposes network,
budget, and chunk choices before the explicit action. Review state and notes are
saved separately from computed recipes. Advanced statistics, comparison, audit,
raw evidence, and references remain contextual. Keep user edits and current
source context intact while switching workspaces and inspector views.

## Working rules

- Preserve equal Parser/Cookbooks standing and the native developer-tool direction.
- Reuse CSS tokens and shared components instead of adding a second visual system.
- Give document identity and evidence priority over toolbar density.
- Keep selected source, result, and inspector visibly connected at every supported size.
- Verify light/dark appearance, long real content, focus, errors, and compact layouts.
- Avoid landing-page typography, decorative dashboards, and automatic extraction.

## Native desktop polish

The transparent native titlebar retains macOS traffic lights and window behavior.
Its background follows the work-area theme; the title shows the selected document
and cookbook identity. Review menu commands are enabled only in an available
saved-run review context. Keep native and in-context shortcut labels aligned:
Cmd–Option–A accepts, Cmd–Option–I marks incorrect, Cmd–Option–U marks uncertain,
and Cmd–Shift–Return saves and advances to the next unreviewed source document.
Advance occurs only after saving succeeds and follows source order, wrapping once.
It clears presentation filters when advancing so the selected document is visible.

A 26px status bar uses the mantle surface, muted 12px text, and tabular counts.
It shows pending work, saved/unsaved review state, and remaining reviews. Cookbook
progress remains visible when switching to Parser. Recent runs appear in Open,
retain at most eight unique paths, and never reopen automatically. Run tools offer
Finder reveal actions; web recipes offer an explicit browser action.

Motion is limited to a 150ms, 3px inspector entrance and a subtle button press.
Reduced-motion preferences disable these effects. Theme colors switch immediately
to avoid transient text/background contrast failures. Notifications, a command
palette, and single-instance handling are outside this pass.
