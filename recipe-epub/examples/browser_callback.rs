//! Browser-compatible whole-book extraction without native features.
//!
//! A real wasm adapter replaces the body of `call_model` with its JavaScript
//! Promise bridge. The shared driver does not require `Send` futures: this
//! example deliberately captures `Rc<RefCell<_>>`, which is local to a browser
//! thread.

use std::cell::RefCell;
use std::rc::Rc;

use recipe_epub::{
    CallFailure, CallResult, Chunk, ChunkOutcome, ExtractionReport, ModelTier,
    OrchestrationOptions, Usage, extract_chunks_with, try_extract_chunk_detailed,
};

/// Drive chunks with a browser-local transport. This concrete example returns
/// empty tool output; a JS adapter would deserialize its proxy response into
/// `CallResult` and retain usage/truncation metadata when available.
pub async fn browser_callback_example(chunks: Vec<Chunk>) -> ExtractionReport {
    let calls = Rc::new(RefCell::new(Vec::new()));
    extract_chunks_with(
        chunks,
        "browser.epub",
        &OrchestrationOptions {
            concurrency: 4,
            fallback: true,
            previews: true,
        },
        move |index, chunk, tier| {
            let calls = Rc::clone(&calls);
            async move {
                calls.borrow_mut().push((index, tier));
                let driven = try_extract_chunk_detailed(&chunk.doc_path, || async {
                    call_model(&chunk, tier).await
                })
                .await?;
                Ok(ChunkOutcome {
                    recipes: driven.recipes,
                    usage: driven.usage,
                    cached: false,
                    truncated: driven.truncated,
                })
            }
        },
        |_snapshot| {
            // Serialize `snapshot.preview` for an incremental browser preview.
        },
    )
    .await
}

async fn call_model(_chunk: &Chunk, _tier: ModelTier) -> Result<CallResult, CallFailure> {
    Ok(CallResult {
        input: None,
        usage: Usage::default(),
        truncated: false,
    })
}

fn main() {
    // The example is compile-checked for wasm32 with default features disabled.
}
