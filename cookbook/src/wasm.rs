//! The browser boundary. A JavaScript host opens a book from bytes, asks for
//! an outline or an estimate, and runs extraction with a `send` callback that
//! forwards each gateway request (path, headers, body) through the host's
//! authenticated proxy and resolves with `{status, headers, body}`.
//!
//! Every type crossing here is one of the crate's serde structs; the
//! TypeScript declarations are generated from them by tsify.

use std::rc::Rc;

use futures::FutureExt;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise};

use crate::cache::NoCache;
use crate::model::ImageRef;
use crate::report::{BookOutline, Estimate, ExtractOptions, Progress};
use crate::transport::{CancelToken, HttpRequest, HttpResponse, Transport, TransportError};
use crate::{Error, Usage};

fn serializer() -> serde_wasm_bindgen::Serializer {
    serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true)
}

fn to_js<T: serde::Serialize>(value: &T) -> Result<JsValue, JsError> {
    value
        .serialize(&serializer())
        .map_err(|e| JsError::new(&e.to_string()))
}

fn js_message(value: &JsValue) -> String {
    value
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| value.as_string())
        .unwrap_or_else(|| "JavaScript error".into())
}

/// Carries requests through a JavaScript `send(request) => Promise<response>`.
struct JsTransport {
    send: js_sys::Function,
}

impl Transport for JsTransport {
    async fn send(
        &self,
        request: HttpRequest,
        cancel: &CancelToken,
    ) -> Result<HttpResponse, TransportError> {
        let request = to_js(&request)
            .map_err(|_| TransportError::Other("cannot serialize request".into()))?;
        let promise = self
            .send
            .call1(&JsValue::NULL, &request)
            .map_err(|e| TransportError::Other(js_message(&e)))?;
        let promise = js_sys::Promise::resolve(&promise);
        let response = futures::select_biased! {
            _ = cancel.cancelled().fuse() => return Err(TransportError::Cancelled),
            r = JsFuture::from(promise).fuse() => r.map_err(|e| TransportError::Other(js_message(&e)))?,
        };
        serde_wasm_bindgen::from_value::<HttpResponse>(response)
            .map_err(|e| TransportError::Other(format!("response shape: {e}")))
    }

    async fn sleep(&self, ms: u64) {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            let window = js_sys::global();
            let set_timeout =
                js_sys::Reflect::get(&window, &"setTimeout".into()).unwrap_or(JsValue::UNDEFINED);
            if let Some(f) = set_timeout.dyn_ref::<js_sys::Function>() {
                let _ = f.call2(&window, &resolve, &JsValue::from_f64(ms as f64));
            } else {
                let _ = resolve.call0(&JsValue::NULL);
            }
        });
        let _ = JsFuture::from(promise).await;
    }
}

/// An opened EPUB. Keep it around for `read_image`; drop it to free the bytes.
#[wasm_bindgen]
pub struct Book {
    inner: Rc<crate::Book>,
    cancel: CancelToken,
}

/// Open an EPUB from its bytes. `label` names it in reports and gateway
/// metadata (a file name is fine).
#[wasm_bindgen]
pub fn open_book(bytes: Vec<u8>, label: String) -> Result<Book, JsError> {
    let inner = crate::Book::open(bytes, label).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(Book {
        inner: Rc::new(inner),
        cancel: CancelToken::new(),
    })
}

#[wasm_bindgen]
impl Book {
    pub fn outline(&self) -> BookOutline {
        self.inner.outline()
    }

    /// Cost and time before spending anything. The browser has no chunk
    /// cache, so every chunk counts.
    pub fn estimate(&self, options: ExtractOptions) -> Result<Estimate, JsError> {
        self.inner
            .estimate(&options, &NoCache)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Run extraction. `send(request)` must return a promise of
    /// `{status, headers, body}`; `on_progress(progress)` is called after
    /// every settled chunk. Resolves with the `Extraction`; rejects with the
    /// error message, or with `"cancelled"` after `cancel()`.
    #[wasm_bindgen(unchecked_return_type = "Promise<Extraction>")]
    pub fn extract(
        &self,
        options: ExtractOptions,
        send: js_sys::Function,
        on_progress: js_sys::Function,
    ) -> js_sys::Promise {
        let book = Rc::clone(&self.inner);
        let cancel = self.cancel.clone();
        future_to_promise(async move {
            let transport = JsTransport { send };
            let progress = |p: Progress| {
                if let Ok(value) = to_js(&p) {
                    let _ = on_progress.call1(&JsValue::NULL, &value);
                }
            };
            match book
                .extract(&options, &transport, &NoCache, &cancel, progress)
                .await
            {
                Ok(extraction) => to_js(&extraction).map_err(JsValue::from),
                Err(Error::Cancelled(_)) => Err(JsValue::from_str("cancelled")),
                Err(e) => Err(JsValue::from_str(&e.to_string())),
            }
        })
    }

    /// Stop an extraction in flight. The pending promise rejects with
    /// `"cancelled"`. A new extraction on the same book needs a fresh token,
    /// so this book cannot be extracted again after cancelling.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Image bytes for an `ImageRef.path`, or `undefined`.
    pub fn read_image(&self, path: &str) -> Option<Vec<u8>> {
        self.inner.read_image(path).map(|(bytes, _)| bytes)
    }

    pub fn image_mime(&self, path: &str) -> Option<String> {
        self.inner.read_image(path).map(|(_, mime)| mime)
    }

    pub fn cover(&self) -> Option<ImageRef> {
        self.inner.cover().cloned()
    }

    pub fn sha256(&self) -> String {
        self.inner.source().sha256.clone()
    }
}

/// Token usage from a raw provider response body, for hosts that record usage
/// per call. `model` is the catalog id the request was built for.
#[wasm_bindgen]
pub fn usage_from_response(model: &str, body: &str) -> Option<Usage> {
    crate::usage_from_response(model, body)
}

/// The catalog's default model ladder, for display.
#[wasm_bindgen]
pub fn default_ladder() -> Vec<String> {
    crate::models::DEFAULT_LADDER
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Reports which parts of the extraction were served by which model, so a host
/// can render the catalog entries by id.
#[wasm_bindgen]
pub fn model_catalog() -> Result<JsValue, JsError> {
    to_js(&crate::models::catalog())
}
