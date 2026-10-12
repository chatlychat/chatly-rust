//! The HTTP seam.
//!
//! [`Transport`] is the only place this crate touches the network. The
//! default implementation is [`UreqTransport`]; the point of the trait is
//! that the test suite and anyone with their own connection pool, proxy or
//! instrumentation can substitute something else without reimplementing
//! auth, idempotency, retries or error classification.

use std::sync::Arc;
use std::time::Duration;

use crate::error::ChatlyError;

/// One outbound HTTP request, fully resolved.
///
/// By the time a `Transport` sees this, `url` is absolute, `headers` carries
/// the bearer token and (on writes) the `Idempotency-Key`, and `body` is
/// already serialised. A transport must send it as-is: changing the body
/// changes nothing but the digest of a signed replay, and dropping a header
/// breaks the idempotency contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Uppercase HTTP method: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`.
    pub method: String,
    /// Absolute URL including any query string.
    pub url: String,
    /// Header names are lowercase. Order is stable but not significant.
    pub headers: Vec<(String, String)>,
    /// Request body, or `None` for a request that has none.
    pub body: Option<Vec<u8>>,
    /// How long the whole exchange may take.
    pub timeout: Duration,
}

impl HttpRequest {
    /// The first value for a header name, compared case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The body decoded as UTF-8, lossily. Convenience for tests and logs.
    pub fn body_text(&self) -> Option<String> {
        self.body
            .as_ref()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }
}

/// One inbound HTTP response.
///
/// A non-2xx status is **not** an error at this layer: the transport reports
/// what the server said, and [`crate::ChatlyClient`] decides what it means.
/// A transport that turned a 404 into its own error type would hide the body
/// and the `x-request-id` that make the 404 diagnosable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers. Names should be lowercased by the transport.
    pub headers: Vec<(String, String)>,
    /// Response body bytes.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Build a response. Header names are lowercased for you.
    pub fn new(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        let headers = headers
            .into_iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), value))
            .collect();
        Self {
            status,
            headers,
            body,
        }
    }

    /// Build a response with a JSON content type. Convenience for tests and
    /// for hand-written transports.
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self::new(
            status,
            vec![("content-type".to_owned(), "application/json".to_owned())],
            body.into().into_bytes(),
        )
    }

    /// The first value for a header name, compared case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The body decoded as UTF-8, lossily.
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Whether the status is in 200..=299.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Anything that can perform an [`HttpRequest`].
///
/// Implement this to inject a fake in tests, or to route through your own
/// client. The only contract is: send the request as given, return what the
/// server said, and report a failure to reach the server as
/// [`ChatlyError::Transport`] — never as a synthesised status code, or the
/// retry policy will treat a DNS failure as a real 503 forever.
pub trait Transport: Send + Sync + 'static {
    /// Perform the request.
    fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, ChatlyError>;
}

impl<T: Transport + ?Sized> Transport for Arc<T> {
    fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, ChatlyError> {
        (**self).execute(request)
    }
}

/// The default transport: blocking HTTP/1.1 over `ureq`, with rustls.
///
/// Configured deliberately:
///
/// - `http_status_as_error(false)` so a 404 arrives as a response, not as a
///   `ureq::Error` with the body already thrown away.
/// - `max_redirects(0)` with errors off, so a 3xx surfaces as
///   [`ChatlyError::Api`] with its status intact. Following a redirect from
///   an API host is never right, and following one with the
///   `Authorization` header attached would hand a workspace-wide bearer
///   token to whatever the `Location` pointed at.
/// - `https_only(false)`, because a self-hosted install and this crate's own
///   loopback tests both talk plain HTTP. Production is HTTPS because the
///   default base URL is.
#[derive(Debug)]
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    /// A transport whose per-request deadline is `timeout`.
    pub fn new(timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .https_only(false)
            .timeout_global(Some(timeout))
            .build();
        Self {
            agent: config.new_agent(),
        }
    }

    /// A transport over an agent you configured yourself.
    ///
    /// You almost certainly want `http_status_as_error(false)` on it; with
    /// ureq's default of `true`, every 4xx and 5xx arrives here as a
    /// `ureq::Error::StatusCode` carrying a status and no body, so
    /// [`crate::ApiError`] would lose `code`, `message` and the raw body.
    pub fn with_agent(agent: ureq::Agent) -> Self {
        Self { agent }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new(crate::client::DEFAULT_TIMEOUT)
    }
}

impl Transport for UreqTransport {
    fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, ChatlyError> {
        let mut builder = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let body = request.body.clone().unwrap_or_default();
        let outbound = builder
            .body(body)
            .map_err(|err| ChatlyError::transport_from("could not build request", err))?;

        let response = self
            .agent
            .run(outbound)
            .map_err(|err| ChatlyError::transport_from(err.to_string(), err))?;

        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_ascii_lowercase(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let mut inbound = response.into_body();
        let body = inbound
            .read_to_vec()
            .map_err(|err| ChatlyError::transport_from("could not read response body", err))?;

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn response_header_lookup_is_case_insensitive() {
        let response = HttpResponse::new(
            200,
            vec![("X-Request-Id".to_owned(), "req_1".to_owned())],
            b"{}".to_vec(),
        );
        assert_eq!(response.header("x-request-id"), Some("req_1"));
        assert_eq!(response.header("X-REQUEST-ID"), Some("req_1"));
        assert_eq!(response.header("missing"), None);
        assert!(response.is_success());
    }

    #[test]
    fn request_header_lookup_is_case_insensitive() {
        let request = HttpRequest {
            method: "POST".to_owned(),
            url: "https://example.invalid/v1/x".to_owned(),
            headers: vec![("idempotency-key".to_owned(), "k".to_owned())],
            body: Some(b"{\"a\":1}".to_vec()),
            timeout: Duration::from_secs(1),
        };
        assert_eq!(request.header("Idempotency-Key"), Some("k"));
        assert_eq!(request.body_text().unwrap(), "{\"a\":1}");
    }

    #[test]
    fn non_2xx_is_not_a_success() {
        assert!(!HttpResponse::json(404, "{}").is_success());
        assert!(!HttpResponse::json(500, "{}").is_success());
        assert!(HttpResponse::json(204, "").is_success());
    }
}
