---
name: ingredient-parser
description: One React frontend for the website and the macOS app, on Tailwind v4 tokens and Base UI primitives.
colors:
  light-base: "#f9f9fb"
  light-mantle: "#eff0f3"
  light-evidence: "#ffffff"
  light-control: "#eceef2"
  light-hover: "#e2e5ea"
  light-line: "#dcdfe5"
  light-line-strong: "#c4c8d0"
  light-fg: "#23272f"
  light-muted: "#5c6371"
  light-faint: "#8a909c"
  light-accent: "#195bad"
  light-accent-ink: "#ffffff"
  light-ok: "#1b7341"
  light-warn: "#825b0c"
  light-bad: "#b52a2f"
  light-amount: "#8b5321"
  light-violet: "#6b45b8"
  light-selection: "rgba(25, 91, 173, 0.1)"
  dark-base: "#1e1f22"
  dark-mantle: "#252629"
  dark-evidence: "#18191c"
  dark-control: "#2a2b2f"
  dark-hover: "#34363b"
  dark-line: "#36383d"
  dark-line-strong: "#4a4d54"
  dark-fg: "#ebedf1"
  dark-muted: "#adb1bb"
  dark-faint: "#7d818b"
  dark-accent: "#70b1ff"
  dark-accent-ink: "#18191c"
  dark-ok: "#73cf97"
  dark-warn: "#edc663"
  dark-bad: "#ff8a8a"
  dark-amount: "#e1b382"
  dark-violet: "#c3a6ff"
  dark-selection: "rgba(112, 177, 255, 0.12)"
typography:
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, Segoe UI Variable, system-ui, sans-serif"
    fontSize: "12.5–13px (workbench), 15–16px (landing)"
  mono:
    fontFamily: "ui-monospace, SF Mono, Menlo, monospace"
    fontSize: "12–13px"
rounded:
  control: "6px"
  panel: "10px"
---

# Design System: ingredient-parser

## One frontend, two hosts

`ui/` is the only frontend. The same build is the public website (with a landing
page) and the Tauri window (workbench only). The parser and cookbook reader run
in Rust on both: natively through one `core` Tauri command on the desktop,
compiled to WebAssembly in a worker on the web (`food-core::dispatch` in both
cases). Desktop-only abilities — the Calibre library, the run store, model
calls, bundle export, Finder — are gated by `can` in `ui/src/api`, never by a
second component tree.

## Tokens

`ui/src/styles.css` is the implementation authority. Every color is a semantic
CSS variable with a light and a dark value, exposed to Tailwind through
`@theme inline` (`bg-base`, `text-muted`, `border-line`, …). Appearance is
System/Light/Dark, stored under `v2:appearance` and applied before first paint
by `index.html`.

- `base` is the work area, `mantle` holds navigation, inspectors and the status
  bar, and `evidence` is the surface for source text, editors and cards.
- `control`, `hover`, `line` and `line-strong` carry control states.
- `accent` marks primary actions, links, focus and the ingredient **name**.
  `amount` marks amounts, `violet` marks the **modifier**. The three field
  colors appear together wherever a line is decomposed (`.field-*` underlines,
  `FieldLegend`) and nowhere else.
- `ok`, `warn` and `bad` are diagnostic outcomes, always paired with text.

Do not add hex colors or a second set of variables. Translucent tints of a token
(`bg-accent/10`) are the way to make a quieter variant.

## Primitives

Base UI supplies behavior and accessibility; `ui/src/components/ui.tsx` owns the
look: `Button` (primary / secondary / ghost / danger), `IconButton` (always with
a tooltip label), `Tabs` (underlined, animated indicator), `Segmented` (pill
switch), `ActionMenu`, `Badge` tones, `Kbd`, `EmptyState` (title, one line of
explanation, a concrete next action), `Facts`. `components/layout.tsx` has the
resizable `Split` (remembered per id, keyboard resizable) and the virtualized,
keyboard-navigable `VirtualList` listbox. The ingredient vocabulary —
`SourceText`, `ResultSummary`, `StagePipeline`, `TraceTree`, `Inspector` —
lives in `components/ingredient.tsx` and is shared by the landing page, the
parser and the cookbook reader.

Errors are toasts (sonner) unless they belong inline to a form. Every action is
also a ⌘K palette command; ⌘1/⌘2 switch workspaces.

## Workbench

A 212px sidebar (brand, palette button, workspaces, appearance) collapses to an
icon rail below 768px. Each workspace has a 48px header (title on the left, mode
switch or actions on the right) and reports a one-line status and detail to the
28px status bar, which also names the engine (native Rust or WebAssembly). A
running job marks its workspace with a spinner and guards window close.

- **Parser**: Lines (parse as you type; previous results stay visible, dimmed,
  while the next parse runs), Recipe (URL → recipe with scale control and
  annotated method), Corpus (score, status chips with counts, search, field
  comparison, inspect). Selection always drives the inspector on the right.
- **Cookbooks**: Library or drop zone → Book (outline, classification,
  extraction or "needs the desktop app") → Run (contents tree, item with
  Extracted/Source views, ingredient inspector, Diagnostics).

Sizes inside the workbench are compact: 12.5–13px body, 11–11.5px captions,
28–32px controls, 15px workspace titles. Long evidence scrolls in its own pane
(`min-h-0`); the page itself never scrolls.

## Landing

The landing page (web only, `/`) is the one place for display type: a 40–56px
headline, a large live parser input with the decomposed line and the result, and
sections that each run real code (the stage pipeline for the hero line,
rich-text measures). Keep every section live and honest — no screenshots of
features, no invented numbers. Links lead into the workbench with state in the
URL (`/parser?q=…`, `?mode=recipe&url=…`).

## Source evidence and actions

Render imported titles, source lines, recipe fields, traces and JSON as text
through React. Do not inject EPUB or recipe HTML. Images come through explicit
Rust image commands as data URLs. Opening a book or a saved run never starts
extraction; extraction shows backend, estimate and readiness before the explicit
action. In the browser, opened files stay in the tab's worker.

## Working rules

- One visual system: reuse tokens and primitives instead of local styling.
- Parser and Cookbooks have equal standing; the desktop has no landing page.
- Decompositions always use the three field colors with a legend.
- Verify light and dark, keyboard focus, long real content, errors and the
  narrow rail. Motion is short (≤200ms) and decorative motion respects
  `prefers-reduced-motion`.
