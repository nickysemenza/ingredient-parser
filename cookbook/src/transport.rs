//! The one seam between the runtime-agnostic core and a host: carrying a fully
//! built gateway request over HTTP.
//!
//! The core builds the path, headers, and body; it never sees credentials. A
//! native host adds the gateway base URL and `cf-aig-authorization` and sends
//! with reqwest; the browser host hands the request to cubby's server, which
//! signs and forwards it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use serde::{Deserialize, Serialize};

/// A request relative to the gateway root, e.g. path `/anthropic/v1/messages`.
/// Headers never include authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct HttpRequest {
    pub path: String,
    pub headers: Vec<(String, String)>,
    #[cfg_attr(feature = "typescript", ts(type = "unknown"))]
    #[cfg_attr(feature = "wasm", tsify(type = "unknown"))]
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// First header value with this name, case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// No usable HTTP response was obtained.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("request timed out")]
    Timeout,
    #[error("could not connect")]
    Connect,
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

impl TransportError {
    pub fn kind(&self) -> &'static str {
        match self {
            TransportError::Timeout => "timeout",
            TransportError::Connect => "connect",
            TransportError::Cancelled => "cancelled",
            TransportError::Other(_) => "other",
        }
    }
}

/// Carries one request. Implementations may consult `cancel` to abandon a
/// call in flight; the core stops issuing calls once it is set either way.
#[allow(async_fn_in_trait)]
pub trait Transport {
    async fn send(
        &self,
        request: HttpRequest,
        cancel: &CancelToken,
    ) -> Result<HttpResponse, TransportError>;

    /// Wait before a retry. The default returns at once, which is right for
    /// tests; hosts back it with their timer.
    async fn sleep(&self, _ms: u64) {}
}

/// A shared cancellation flag that also wakes anyone awaiting it.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<Inner>);

#[derive(Debug, Default)]
struct Inner {
    cancelled: AtomicBool,
    wakers: Mutex<Vec<Waker>>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        let wakers = std::mem::take(&mut *self.0.wakers.lock().unwrap_or_else(|e| e.into_inner()));
        for waker in wakers {
            waker.wake();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// Resolves when the token is cancelled.
    pub fn cancelled(&self) -> Cancelled {
        Cancelled(self.clone())
    }
}

pub struct Cancelled(CancelToken);

impl Future for Cancelled {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0.is_cancelled() {
            return Poll::Ready(());
        }
        self.0
            .0
            .wakers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(cx.waker().clone());
        // Re-check after registering so a cancel between the check and the
        // registration is not missed.
        if self.0.is_cancelled() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_lookup_is_case_insensitive() {
        let r = HttpResponse {
            status: 200,
            headers: vec![("CF-AIG-Log-Id".into(), "abc".into())],
            body: String::new(),
        };
        assert_eq!(r.header("cf-aig-log-id"), Some("abc"));
        assert!(r.is_success());
        assert!(!HttpResponse { status: 429, ..r }.is_success());
    }

    #[test]
    fn cancel_wakes_waiters() {
        let token = CancelToken::new();
        let waiter = token.cancelled();
        let mut waiter = Box::pin(waiter);
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        assert!(waiter.as_mut().poll(&mut cx).is_pending());
        token.cancel();
        assert!(token.is_cancelled());
        assert!(waiter.as_mut().poll(&mut cx).is_ready());
    }
}
