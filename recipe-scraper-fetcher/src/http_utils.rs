use reqwest::{Request, Response};

use reqwest_middleware::ClientBuilder;

use reqwest_middleware::{ClientWithMiddleware, Result};

use reqwest_tracing::{ReqwestOtelSpanBackend, default_on_request_end, reqwest_otel_span};

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
        let time_elapsed = extension
            .get::<Instant>()
            .map(|start| start.elapsed().as_millis() as i64)
            .unwrap_or(0);
        default_on_request_end(span, outcome);
        span.record("time_elapsed", time_elapsed);
    }
}

pub fn http_client() -> ClientWithMiddleware {
    // Bounded timeouts: without them a hung/slow-loris server stalls
    // scrape_url forever (reqwest's default has NO total or connect timeout).
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|e| {
            // Builder only fails on TLS/resolver misconfiguration. The fallback
            // default client has NO timeouts, defeating the slow-loris protection
            // this function exists to provide — so make the degradation loud
            // rather than swallowing it.
            tracing::error!(
                "http_client builder failed; falling back to un-timed default client: {e}"
            );
            reqwest::Client::new()
        });
    ClientBuilder::new(client)
        .with(reqwest_tracing::TracingMiddleware::<TimeTrace>::new())
        .build()
}
