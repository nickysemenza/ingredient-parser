use reqwest::{Request, Response};

use reqwest_middleware::ClientBuilder;

use reqwest_middleware::{ClientWithMiddleware, Result};

use reqwest_tracing::{default_on_request_end, reqwest_otel_span, ReqwestOtelSpanBackend};

use http::Extensions;
use std::time::Instant;

use tracing::Span;

pub struct TimeTrace;

#[allow(unexpected_cfgs)]
impl ReqwestOtelSpanBackend for TimeTrace {
    fn on_request_start(req: &Request, extension: &mut Extensions) -> Span {
        extension.insert(Instant::now());
        reqwest_otel_span!(name = "reqwest-http-request", req)
    }

    fn on_request_end(span: &Span, outcome: &Result<Response>, extension: &mut Extensions) {
        if let Some(start_time) = extension.get::<Instant>() {
            let time_elapsed = start_time.elapsed().as_millis() as i64;
            default_on_request_end(span, outcome);
            span.record("time_elapsed", time_elapsed);
        } else {
            default_on_request_end(span, outcome);
        }
    }
}

pub fn http_client() -> ClientWithMiddleware {
    ClientBuilder::new(reqwest::Client::new())
        .with(reqwest_tracing::TracingMiddleware::<TimeTrace>::new())
        .build()
}
