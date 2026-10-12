//! The one error type every call in this crate returns.

use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;

/// The parts of an HTTP error response a caller might need.
///
/// Attached to every [`ChatlyError`] variant that came back from the API with
/// a status line. `code` and `message` are read from the API's error envelope
/// (`{"error":{"code":…,"message":…}}`); `body` is the response text exactly
/// as received, so nothing the envelope does not model is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// HTTP status code.
    pub status: u16,
    /// The API's machine-readable `code`, e.g. `not_found`, `validation_failed`.
    ///
    /// `None` when the response had no JSON error envelope — a gateway's HTML
    /// 502, for instance. Branch on the [`ChatlyError`] variant rather than on
    /// this string.
    pub code: Option<String>,
    /// The API's human-readable `message`, or a generated fallback naming the
    /// status when the response carried none.
    pub message: String,
    /// The `x-request-id` response header, for correlating with our logs.
    ///
    /// `None` when the header is absent; not every deployment sets it, and a
    /// self-hosted install behind no reverse proxy generally will not.
    pub request_id: Option<String>,
    /// The raw response body, lossily decoded as UTF-8.
    pub body: String,
}

impl ApiError {
    /// Build from a status, the raw body text and the `x-request-id` header.
    ///
    /// Parses the API's `{"error":{"code","message"}}` envelope, then falls
    /// back to a top-level `{"code","message"}`, then to a status-derived
    /// message. Never fails: an unparseable body still yields a usable error.
    pub(crate) fn from_response(status: u16, body: String, request_id: Option<String>) -> Self {
        let parsed: Option<serde_json::Value> = serde_json::from_str(&body).ok();
        let envelope = parsed
            .as_ref()
            .and_then(|v| v.get("error"))
            .or(parsed.as_ref());
        let code = envelope
            .and_then(|v| v.get("code"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let message = envelope
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("HTTP {status}"));
        Self {
            status,
            code,
            message,
            request_id,
            body,
        }
    }
}

/// Everything that can go wrong in this crate.
///
/// Branch on the variant, not on a message string: the whole point of the
/// split is that "my key is wrong" ([`ChatlyError::Authentication`]) is
/// distinguishable from "my key lacks a scope"
/// ([`ChatlyError::Permission`]) without parsing prose.
///
/// Marked `#[non_exhaustive]`: a future API status may earn its own variant,
/// so a `match` needs a `_` arm. [`ChatlyError::Api`] is where any status
/// without a dedicated variant arrives today.
#[derive(Debug)]
#[non_exhaustive]
pub enum ChatlyError {
    /// Raised before any request leaves the process: an empty API key, an
    /// empty base URL, an empty pinned idempotency key, an empty
    /// identity-verification secret, or a request body that could not be
    /// serialised to JSON.
    Config(String),
    /// HTTP 401. The key is missing, malformed, revoked or expired.
    Authentication(ApiError),
    /// HTTP 403. The key is valid but lacks the scope for this route, or the
    /// route does not accept API keys at all.
    Permission(ApiError),
    /// HTTP 404.
    NotFound(ApiError),
    /// HTTP 409. A genuine conflict, or an `Idempotency-Key` replayed with a
    /// different body.
    Conflict(ApiError),
    /// The body failed validation — **HTTP 400**, and also 422.
    ///
    /// 400 is what this API actually sends: `ValidationError` in
    /// `packages/shared/src/errors` is `httpStatus: 400, code:
    /// 'validation_failed'`, and the exception filter maps a raw `ZodError`
    /// the same way. Nothing returns 422. Both are mapped here so a caller
    /// is correct either way, per CONTRACT.md §3 — but do not write code
    /// that waits for a 422.
    ///
    /// Because 400 is claimed by validation, a malformed *request* that is
    /// not a schema failure (a bad method, an unparseable request line from
    /// a proxy) also arrives here. `ApiError::code` distinguishes them:
    /// a genuine schema refusal carries `validation_failed`.
    Validation(ApiError),
    /// HTTP 429.
    RateLimited {
        /// Status, code, message, request id and raw body.
        error: ApiError,
        /// The `Retry-After` header as a duration, when present and numeric.
        ///
        /// `None` when the header was absent or was an HTTP-date rather than
        /// a count of seconds; this crate does not parse HTTP-dates.
        retry_after: Option<Duration>,
    },
    /// HTTP 5xx.
    Server(ApiError),
    /// Any other non-2xx status: 405, 413, 415, a 3xx we declined to
    /// follow, and anything the API grows later.
    Api(ApiError),
    /// The request never produced a status line: DNS failure, refused
    /// connection, TLS failure, timeout. Distinct from an HTTP status, per
    /// CONTRACT.md §3.
    Transport {
        /// What went wrong, in the transport's own words.
        message: String,
        /// The underlying error, when the transport surfaced one.
        source: Option<Box<dyn StdError + Send + Sync + 'static>>,
    },
    /// A 2xx response whose body was not the JSON this crate expected.
    Decode {
        /// What the decoder objected to.
        message: String,
        /// The raw body, so you can see what actually arrived.
        body: String,
    },
}

impl ChatlyError {
    /// Build a transport failure with no underlying error object.
    ///
    /// Public because [`crate::Transport`] implementors return this type and
    /// need a way to report "could not reach the server" — which must not be
    /// faked as an HTTP status, or the retry policy will treat a DNS failure
    /// as a genuine 503 on every attempt.
    pub fn transport(message: impl Into<String>) -> Self {
        Self::Transport {
            message: message.into(),
            source: None,
        }
    }

    /// Build a transport failure that wraps the underlying error, so
    /// [`std::error::Error::source`] reaches it.
    pub fn transport_from(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self::Transport {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Classify an HTTP response into the right variant.
    pub(crate) fn from_http(
        status: u16,
        body: String,
        request_id: Option<String>,
        retry_after: Option<Duration>,
    ) -> Self {
        let error = ApiError::from_response(status, body, request_id);
        match status {
            // 400 before 422: validation on this API is 400. See the
            // `Validation` variant's docs and CONTRACT.md §3.
            400 | 422 => Self::Validation(error),
            401 => Self::Authentication(error),
            403 => Self::Permission(error),
            404 => Self::NotFound(error),
            409 => Self::Conflict(error),
            429 => Self::RateLimited { error, retry_after },
            500..=599 => Self::Server(error),
            _ => Self::Api(error),
        }
    }

    /// The HTTP status, when this error came from a response.
    pub fn status(&self) -> Option<u16> {
        self.api_error().map(|e| e.status)
    }

    /// The API's `code` field, when there was one.
    pub fn code(&self) -> Option<&str> {
        self.api_error().and_then(|e| e.code.as_deref())
    }

    /// A message suitable for a log line. Always present.
    pub fn message(&self) -> &str {
        match self {
            Self::Config(message) => message,
            Self::Transport { message, .. } | Self::Decode { message, .. } => message,
            _ => self
                .api_error()
                .map_or("unknown error", |e| e.message.as_str()),
        }
    }

    /// The `x-request-id` of the failing response, when there was one.
    pub fn request_id(&self) -> Option<&str> {
        self.api_error().and_then(|e| e.request_id.as_deref())
    }

    /// The raw response body, when there was one.
    pub fn body(&self) -> Option<&str> {
        match self {
            Self::Decode { body, .. } => Some(body),
            _ => self.api_error().map(|e| e.body.as_str()),
        }
    }

    /// `Retry-After`, only ever `Some` on [`ChatlyError::RateLimited`].
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// The attached [`ApiError`], for the variants that carry one.
    pub fn api_error(&self) -> Option<&ApiError> {
        match self {
            Self::Authentication(e)
            | Self::Permission(e)
            | Self::NotFound(e)
            | Self::Conflict(e)
            | Self::Validation(e)
            | Self::Server(e)
            | Self::Api(e) => Some(e),
            Self::RateLimited { error, .. } => Some(error),
            Self::Config(_) | Self::Transport { .. } | Self::Decode { .. } => None,
        }
    }

    /// Whether retrying this exact request is safe.
    ///
    /// True for transport failures, 429 and 5xx — and nothing else.
    /// CONTRACT.md §4: never retry a 4xx that is not 429. A 409 in
    /// particular must not be retried; it is how the API reports an
    /// `Idempotency-Key` reused with a different body, and a retry would
    /// report the same conflict again.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport { .. } | Self::RateLimited { .. } | Self::Server(_)
        )
    }

    /// True for [`ChatlyError::Authentication`] — a bad key.
    pub fn is_authentication(&self) -> bool {
        matches!(self, Self::Authentication(_))
    }

    /// True for [`ChatlyError::Permission`] — a key missing a scope.
    pub fn is_permission(&self) -> bool {
        matches!(self, Self::Permission(_))
    }

    /// True for [`ChatlyError::NotFound`].
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }

    /// True for [`ChatlyError::Conflict`].
    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict(_))
    }

    /// True for [`ChatlyError::Validation`].
    pub fn is_validation(&self) -> bool {
        matches!(self, Self::Validation(_))
    }

    /// True for [`ChatlyError::RateLimited`].
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::RateLimited { .. })
    }
}

impl fmt::Display for ChatlyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => write!(f, "chatly: configuration error: {message}"),
            Self::Transport { message, .. } => write!(f, "chatly: transport error: {message}"),
            Self::Decode { message, .. } => {
                write!(f, "chatly: could not decode response: {message}")
            }
            _ => {
                let Some(error) = self.api_error() else {
                    return write!(f, "chatly: unknown error");
                };
                write!(f, "chatly: HTTP {}", error.status)?;
                if let Some(code) = &error.code {
                    write!(f, " {code}")?;
                }
                write!(f, ": {}", error.message)?;
                if let Some(retry_after) = self.retry_after() {
                    write!(f, " (retry after {}s)", retry_after.as_secs())?;
                }
                if let Some(request_id) = &error.request_id {
                    write!(f, " (request_id={request_id})")?;
                }
                Ok(())
            }
        }
    }
}

impl StdError for ChatlyError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Transport { source, .. } => source.as_ref().map(|b| b.as_ref() as &dyn StdError),
            _ => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_api_error_envelope() {
        let err = ChatlyError::from_http(
            404,
            r#"{"error":{"code":"not_found","message":"contact not found"}}"#.to_owned(),
            Some("req_abc".to_owned()),
            None,
        );
        assert!(err.is_not_found());
        assert_eq!(err.status(), Some(404));
        assert_eq!(err.code(), Some("not_found"));
        assert_eq!(err.message(), "contact not found");
        assert_eq!(err.request_id(), Some("req_abc"));
        assert!(err.body().unwrap().contains("not_found"));
    }

    #[test]
    fn falls_back_to_a_top_level_code_and_message() {
        let err = ChatlyError::from_http(
            400,
            r#"{"code":"validation_failed","message":"email is required"}"#.to_owned(),
            None,
            None,
        );
        assert!(err.is_validation());
        assert_eq!(err.code(), Some("validation_failed"));
        assert_eq!(err.message(), "email is required");
    }

    #[test]
    fn survives_a_body_that_is_not_json() {
        let err = ChatlyError::from_http(502, "<html>bad gateway</html>".to_owned(), None, None);
        assert!(matches!(err, ChatlyError::Server(_)));
        assert_eq!(err.code(), None);
        assert_eq!(err.message(), "HTTP 502");
        assert_eq!(err.body(), Some("<html>bad gateway</html>"));
    }

    #[test]
    fn only_transport_429_and_5xx_are_retryable() {
        let retryable = [
            ChatlyError::transport("connection refused"),
            ChatlyError::from_http(429, String::new(), None, None),
            ChatlyError::from_http(500, String::new(), None, None),
            ChatlyError::from_http(503, String::new(), None, None),
        ];
        for err in &retryable {
            assert!(err.is_retryable(), "{err} should be retryable");
        }
        for status in [400, 401, 403, 404, 409, 418, 422] {
            let err = ChatlyError::from_http(status, String::new(), None, None);
            assert!(!err.is_retryable(), "{status} must not be retried");
        }
    }

    /// CONTRACT.md §3: "Validation is 400 on this API, not 422. ... Map
    /// both, so a client is correct either way, but do not expect 422."
    #[test]
    fn both_400_and_422_are_validation() {
        for status in [400u16, 422] {
            let err = ChatlyError::from_http(status, String::new(), None, None);
            assert!(err.is_validation(), "{status} should be Validation");
            assert_eq!(err.status(), Some(status));
            assert!(!err.is_retryable(), "{status} must not be retried");
        }
    }

    #[test]
    fn a_status_with_no_dedicated_variant_lands_in_the_catch_all() {
        for status in [405u16, 413, 415, 302] {
            let err = ChatlyError::from_http(status, String::new(), None, None);
            assert!(matches!(err, ChatlyError::Api(_)), "{status}: {err:?}");
            assert_eq!(err.status(), Some(status));
        }
    }

    #[test]
    fn display_is_one_line_and_names_the_request_id() {
        let err = ChatlyError::from_http(
            429,
            r#"{"error":{"code":"rate_limited","message":"slow down"}}"#.to_owned(),
            Some("req_9".to_owned()),
            Some(Duration::from_secs(7)),
        );
        let shown = err.to_string();
        assert_eq!(
            shown,
            "chatly: HTTP 429 rate_limited: slow down (retry after 7s) (request_id=req_9)"
        );
        assert!(!shown.contains('\n'));
    }

    #[test]
    fn transport_errors_expose_their_source() {
        let io = std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out");
        let err = ChatlyError::transport_from("connect failed", io);
        assert!(StdError::source(&err).is_some());
        assert_eq!(err.status(), None);
        assert!(err.is_retryable());
    }
}
