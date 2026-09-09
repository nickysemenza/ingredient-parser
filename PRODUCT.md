# Maintainer toolkit

<!-- impeccable:product-schema 1 -->

## Platform

Native Rust desktop application using egui/eframe, paired with a headless CLI.

## Users and purpose

Maintainers inspecting ingredient parsing, corpus failures, cookbook extraction, and source evidence. Parser development and cookbook review are equally important daily workflows.

## Capabilities and constraints

The desktop and CLI share existing parser, recipe, corpus, and durable cookbook-run APIs. Saved runs support offline inspection, review, replay, and comparison. Opening a source or restoring a session does not authorize extraction. User input and review decisions remain distinct from computed results.

Parser semantics, library APIs, corpus schemas, and saved-run formats are outside the redesign. Tool commands may break cleanly; current help and repository callers describe the new interface without compatibility aliases or migration documentation.

## Brand commitments

Use native developer tools, specifically Xcode and Instruments, as visual references. Favor dense resizable panes, system typography, keyboard navigation, neutral light/dark surfaces, and semantic diagnostic colors. Advanced tools belong in context.

## Product principles

- Keep source, result, and inspection evidence connected.
- Make network and extraction actions explicit.
- Keep terminal data composable and diagnostics separate.
- Preserve working context while changing views.
