//! Extract a structured recipe tree from an EPUB cookbook.
//!
//! The book is read into one cleaned line stream, cut into chunks, and each
//! chunk is sent to a model that answers with *line indices* — Rust copies the
//! text, so nothing is ever paraphrased. Deterministic validation and a
//! table-of-contents cross-check run on every book; a second model re-reads
//! only what was flagged. The result is a [`Cookbook`]: chapters of recipes,
//! techniques and essays with dependency edges, photos, and provenance, plus a
//! [`RunReport`] recording every call, its cost, and its outcome.
//!
//! Hosts differ only in the transport that carries a fully built gateway
//! request: native code uses reqwest, the browser hands it to a JavaScript
//! callback.

pub mod assemble;
pub mod cache;
pub mod chunk;
pub mod classify;
pub mod contract;
pub mod cost;
pub mod crosscheck;
pub mod epub;
mod error;
pub mod eta;
#[cfg(feature = "native")]
pub mod eval;
pub mod gateway;
pub mod library;
pub mod lines;
pub mod model;
pub mod models;
pub mod names;
#[cfg(feature = "native")]
pub mod native;
pub mod parse;
pub mod refs;
pub mod report;
pub mod run;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod transport;
pub mod validate;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use cost::{Usage, cost_for_usage};
pub use error::{Error, Result};
pub use model::*;
pub use report::*;
pub use transport::{CancelToken, HttpRequest, HttpResponse, Transport, TransportError};

use std::collections::BTreeMap;

use jiff::Timestamp;

use crate::cache::{ChunkCache, cache_key};
use crate::chunk::{Chunk, ChunkOptions};
use crate::contract::{CONTRACT_VERSION, build_request};
use crate::epub::nav::Nav;
use crate::epub::open::{Package, read_image, sha256_hex};
use crate::gateway::{CallMeta, build_http};
use crate::lines::BookLines;
use crate::models::Model;
use crate::parse::LineParser;
use crate::run::{RunInput, run};

/// An opened EPUB: bytes plus everything derived from them without a model.
/// Cheap to keep around; `read_image` reads the archive again on demand.
pub struct Book {
    bytes: Vec<u8>,
    package: Package,
    source: BookSource,
    nav: Nav,
    lines: BookLines,
    chunks: Vec<Chunk>,
    cover: Option<ImageRef>,
    open_stages: Vec<StageTiming>,
}

impl Book {
    /// Open, clean, read the contents, and chunk.
    pub fn open(bytes: Vec<u8>, label: impl Into<String>) -> Result<Book> {
        Self::open_with(bytes, label, &ChunkOptions::default())
    }

    pub fn open_with(
        bytes: Vec<u8>,
        label: impl Into<String>,
        chunking: &ChunkOptions,
    ) -> Result<Book> {
        let mut stages = Vec::new();
        let mut mark = Timestamp::now();
        let mut stage = |phase: Phase, stages: &mut Vec<StageTiming>| {
            let now = Timestamp::now();
            stages.push(StageTiming {
                stage: phase,
                ms: elapsed_ms(mark, now),
            });
            mark = now;
        };
        let package = Package::parse(&bytes)?;
        let docs = package.spine_docs(&bytes)?;
        let sha256 = sha256_hex(&bytes);
        stage(Phase::Open, &mut stages);
        let nav = Nav::read(&bytes, &package);
        stage(Phase::Nav, &mut stages);
        let lines = BookLines::build(&docs, &nav);
        stage(Phase::Clean, &mut stages);
        let chunks = chunk::chunk(&lines, chunking);
        stage(Phase::Chunk, &mut stages);
        let source = BookSource {
            label: label.into(),
            sha256,
            title: package.title.clone(),
            authors: package.authors.clone(),
            identifiers: package.identifiers.clone(),
            subjects: package.subjects.clone(),
            spine_docs: docs.len(),
            lines: lines.len(),
        };
        let cover = package.cover_ref();
        Ok(Book {
            bytes,
            package,
            source,
            nav,
            lines,
            chunks,
            cover,
            open_stages: stages,
        })
    }

    pub fn source(&self) -> &BookSource {
        &self.source
    }

    pub fn package(&self) -> &Package {
        &self.package
    }

    pub fn nav(&self) -> &Nav {
        &self.nav
    }

    pub fn lines(&self) -> &BookLines {
        &self.lines
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    pub fn cover(&self) -> Option<&ImageRef> {
        self.cover.as_ref()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn read_image(&self, path: &str) -> Option<(Vec<u8>, String)> {
        read_image(&self.bytes, path)
    }

    pub fn outline(&self) -> BookOutline {
        BookOutline {
            source: self.source.clone(),
            cover: self.cover.clone(),
            chapters: crosscheck::nav_chapter_entries(&self.lines, &self.nav)
                .into_iter()
                .map(|(label, _)| label)
                .collect(),
            nav_recipe_titles: crosscheck::nav_recipe_titles(&self.lines, &self.nav).len(),
            chunks: self.chunks.len(),
            lines: self.lines.len(),
        }
    }

    /// Chunks the cache already answers for the ladder's first model.
    fn cache_hits(
        &self,
        ladder: &[&'static Model],
        options: &ExtractOptions,
        cache: &impl ChunkCache,
    ) -> usize {
        let Some(primary) = ladder.first() else {
            return 0;
        };
        self.chunks
            .iter()
            .filter(|chunk| {
                let request = build_request(chunk, &self.lines);
                let meta = CallMeta {
                    cookbook: &options.label,
                    chunk: &chunk.id,
                    purpose: "extract",
                };
                let http = build_http(primary, &request, options.max_output_tokens, &meta);
                cache
                    .get(&cache_key(
                        CONTRACT_VERSION,
                        primary.id,
                        primary.route.as_str(),
                        &http.body,
                    ))
                    .is_some()
            })
            .count()
    }

    /// Cost and time before spending anything.
    pub fn estimate(&self, options: &ExtractOptions, cache: &impl ChunkCache) -> Result<Estimate> {
        let ladder = models::resolve_ladder(&options.ladder)?;
        let hits = self.cache_hits(&ladder, options, cache);
        Ok(eta::cold_estimate(
            &self.chunks,
            &ladder,
            options.concurrency,
            hits,
        ))
    }

    /// Run the whole pipeline. `progress` is called after every settled chunk
    /// and at each phase change. Cancellation returns the partial report.
    pub async fn extract<T: Transport, C: ChunkCache>(
        &self,
        options: &ExtractOptions,
        transport: &T,
        cache: &C,
        cancel: &CancelToken,
        mut progress: impl FnMut(Progress) + Send,
    ) -> Result<Extraction> {
        let started = Timestamp::now();
        let ladder = models::resolve_ladder(&options.ladder)?;
        let hits = self.cache_hits(&ladder, options, cache);
        let estimate = eta::cold_estimate(&self.chunks, &ladder, options.concurrency, hits);
        let input = RunInput {
            book: &self.lines,
            nav: &self.nav,
            chunks: &self.chunks,
            ladder: ladder.clone(),
            options,
            started,
        };
        let output = run(&input, transport, cache, cancel, &mut progress).await;
        let mut stages = self.open_stages.clone();
        let mut mark = Timestamp::now();
        stages.push(StageTiming {
            stage: Phase::Extract,
            ms: elapsed_ms(started, mark),
        });

        let parser = LineParser::new();
        let assemble::Assembled {
            mut chapters,
            orphan_photos,
        } = assemble::assemble(
            &self.lines,
            &self.nav,
            &output.chunks,
            &output.crosscheck.phantom,
            &parser,
        );
        let now = Timestamp::now();
        stages.push(StageTiming {
            stage: Phase::Assemble,
            ms: elapsed_ms(mark, now),
        });
        mark = now;
        let (edges, unresolved_refs) = refs::resolve(&mut chapters, &self.lines, orphan_photos);
        let now = Timestamp::now();
        stages.push(StageTiming {
            stage: Phase::Refs,
            ms: elapsed_ms(mark, now),
        });
        mark = now;
        names::assign_names(&mut chapters);
        stages.push(StageTiming {
            stage: Phase::Names,
            ms: elapsed_ms(mark, Timestamp::now()),
        });

        let cookbook = Cookbook {
            contract: CONTRACT_VERSION.to_string(),
            source: self.source.clone(),
            cover: self.cover.clone(),
            chapters,
            edges,
        };

        let mut by_model: BTreeMap<String, ModelUsage> = BTreeMap::new();
        let mut cost_complete = true;
        for call in &output.calls {
            let entry = by_model
                .entry(call.model.clone())
                .or_insert_with(|| ModelUsage {
                    model: call.model.clone(),
                    calls: 0,
                    usage: Usage::default(),
                    cost_usd: Some(0.0),
                });
            entry.calls += 1;
            entry.usage.add(&call.usage);
            match (entry.cost_usd, call.cost_usd) {
                (Some(sum), Some(c)) => entry.cost_usd = Some(sum + c),
                _ => {
                    entry.cost_usd = None;
                    cost_complete = false;
                }
            }
        }
        let total_cost_usd = by_model.values().filter_map(|m| m.cost_usd).sum();
        let finished = Timestamp::now();
        let report = RunReport {
            run_id: format!(
                "{}-{}",
                started.strftime("%Y%m%dT%H%M%SZ"),
                &self.source.sha256[..8]
            ),
            started_at: started.to_string(),
            finished_at: finished.to_string(),
            book: self.source.clone(),
            options: options.clone(),
            estimate,
            stages,
            calls: output.calls,
            chunks: output.chunks.into_iter().map(|c| c.report).collect(),
            crosscheck: output.crosscheck,
            escalation: output.escalation,
            unresolved_refs,
            usage_by_model: by_model.into_values().collect(),
            total_cost_usd,
            cost_complete,
            eta_trace: output.eta_trace,
            wall_ms: elapsed_ms(started, finished),
            incomplete: output.incomplete,
            cancelled: output.cancelled,
        };
        if output.cancelled {
            return Err(Error::Cancelled(Box::new(report)));
        }
        Ok(Extraction { cookbook, report })
    }
}

impl Cookbook {
    /// Items in an order where every dependency precedes what needs it.
    pub fn dependency_order(&self) -> Vec<String> {
        refs::dependency_order(self)
    }
}

/// Token usage from a raw provider response for `model`, for hosts that
/// record usage per call. `None` for unknown models or bodies without usage.
pub fn usage_from_response(model: &str, body: &str) -> Option<Usage> {
    let m = models::model(model)?;
    gateway::usage_from_response(m.route, body)
}

fn elapsed_ms(from: Timestamp, to: Timestamp) -> u64 {
    (to.as_millisecond() - from.as_millisecond()).max(0) as u64
}

#[cfg(all(test, feature = "typescript"))]
mod typescript_tests {
    use ts_rs::TS;

    #[test]
    fn book_tree_declares_in_typescript() {
        let cfg = ts_rs::Config::default();
        let decl = super::Cookbook::decl(&cfg);
        assert!(decl.contains("chapters: Array<Chapter>"), "{decl}");
        let line = super::IngredientLine::decl(&cfg);
        assert!(line.contains("parsed: Ingredient"), "{line}");
        assert!(line.contains("ref: RecipeRef | null"), "{line}");
    }
}
