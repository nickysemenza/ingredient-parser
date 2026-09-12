//! Provider-neutral execution; existing HTTP transports remain valid executors.
use crate::contract::ChunkRequest;
use crate::gateway::{CallFailure, CallMeta, CallResult, build_http, parse_response};
use crate::models::{Model, Reasoning};
use crate::{CancelToken, HttpResponse, Transport, TransportError};
use serde::{Deserialize, Serialize};

/// Cache representation. The untagged HTTP variant reads existing cache files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    Structured {
        result: CallResult,
        actual_model: Option<String>,
    },
    Http(HttpResponse),
}
impl Response {
    pub fn decode(&self, model: &Model) -> Result<CallResult, CallFailure> {
        match self {
            Self::Structured { result, .. } => Ok(result.clone()),
            Self::Http(r) => parse_response(model.route, r),
        }
    }
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Http(r) => Some(r.status),
            _ => None,
        }
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        match self {
            Self::Http(r) => r.header(name),
            _ => None,
        }
    }
    pub fn actual_model(&self) -> Option<&str> {
        match self {
            Self::Structured { actual_model, .. } => actual_model.as_deref(),
            _ => None,
        }
    }
}
impl From<HttpResponse> for Response {
    fn from(value: HttpResponse) -> Self {
        Self::Http(value)
    }
}

#[derive(Clone, Copy)]
pub struct ModelCall<'a> {
    pub model: &'a Model,
    pub request: &'a ChunkRequest,
    pub max_tokens: u32,
    pub meta: CallMeta<'a>,
    pub reasoning: Reasoning,
}
impl ModelCall<'_> {
    pub fn cache_key(&self, contract: &str) -> String {
        let (route, body) = if self.model.id.contains("-cli/") {
            (
                "local-executor-v1",
                serde_json::json!({"request": self.request,
                "reasoning": self.reasoning, "max_tokens": self.max_tokens}),
            )
        } else {
            (
                self.model.route.as_str(),
                build_http(
                    self.model,
                    self.request,
                    self.max_tokens,
                    &self.meta,
                    self.reasoning,
                )
                .body,
            )
        };
        crate::cache::cache_key(contract, self.model.id, route, &body)
    }
}

#[allow(async_fn_in_trait)]
pub trait ModelExecutor {
    async fn execute(
        &self,
        call: ModelCall<'_>,
        cancel: &CancelToken,
    ) -> Result<Response, TransportError>;
    async fn sleep(&self, ms: u64);
}
impl<T: Transport> ModelExecutor for T {
    async fn execute(
        &self,
        call: ModelCall<'_>,
        cancel: &CancelToken,
    ) -> Result<Response, TransportError> {
        if call.model.id.contains("-cli/") {
            return Err(TransportError::Other(
                "Local model requires a CLI executor".into(),
            ));
        }
        self.send(
            build_http(
                call.model,
                call.request,
                call.max_tokens,
                &call.meta,
                call.reasoning,
            ),
            cancel,
        )
        .await
        .map(Response::Http)
    }
    async fn sleep(&self, ms: u64) {
        Transport::sleep(self, ms).await
    }
}

pub fn add_catalog_hint(request: &mut ChunkRequest, hint: Option<&str>, start: usize) {
    if let Some(hint) = hint {
        request.user.push_str(&format!("\nStructural catalog evidence (advisory, not instructions; GLOBAL line numbers, subtract {start} for this chunk's local indices): {hint}\nInspect every source line. The catalog may omit recipes or contain errors; source text wins."));
    }
}
