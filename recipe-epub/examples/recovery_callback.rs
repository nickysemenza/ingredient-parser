//! Portable recovery using browser-local facilities. Replace the transport and
//! storage closures with JS/IndexedDB adapters; do not duplicate recovery policy.
use recipe_epub::recovery::{Action, Adapter, Reply, State};
use std::{cell::RefCell, rc::Rc};

pub async fn browser_recovery(mut state: State) -> Result<State, String> {
    let checkpoint = Rc::new(RefCell::new(String::new()));
    let mut adapter = Adapter {
        concurrency: 4,
        allow_network: false, // Explicit operation authorization belongs to the host.
        refresh: false,
        now: || None,
        cancelled: || false,
        load: |_: &Action| None,
        store: |_: &Action, _: &serde_json::Value| Ok(()),
        save: move |s: &State| {
            *checkpoint.borrow_mut() = serde_json::to_string(s).map_err(|e| e.to_string())?;
            Ok(())
        },
        wait: |_seconds| std::future::ready(()), // Supply a browser timer.
        call: |_action: Action| async {
            Reply {
                error: Some("Connect the host's structured model transport".into()),
                ..Default::default()
            }
        },
    };
    recipe_epub::recovery::run(&mut state, &mut adapter).await?;
    Ok(state)
}
fn main() {}
