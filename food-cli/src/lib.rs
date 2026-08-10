//! Library half of `food-cli`: the corpus and diagnostic verbs.
//!
//! These were private modules of the binary, which meant nothing could call
//! them — not a test, not `food-app`, not another tool. `corpus_lint`'s stage
//! coverage and `explain`'s decomposition renderer are genuinely useful beyond
//! the CLI, and the shadow harness's only way to report a result was its
//! process exit code.
//!
//! The rule here: **every verb returns its output.** Printing, file writes,
//! opening a browser and `std::process::exit` all live in `main.rs`, which is
//! the only place allowed to decide a process exit code.

pub mod corpus_lint;
pub mod corpus_table;
pub mod explain;
pub mod tables;
