//! A scripted [`Transport`] so the suite needs no network, no API key and no
//! running API — CONTRACT.md §9.
//!
//! It records every request it is handed and replays queued responses in
//! order, which is what lets a test assert on headers that were actually
//! built by the real dispatch path rather than on a reimplementation of it.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chatly::{ChatlyClient, ChatlyError, HttpRequest, HttpResponse, RetryPolicy, Transport};

/// A transport that replays a script.
#[derive(Debug, Default)]
pub struct MockTransport {
    queued: Mutex<VecDeque<Outcome>>,
    seen: Mutex<Vec<HttpRequest>>,
}

#[derive(Debug)]
enum Outcome {
    Response(HttpResponse),
    Transport(String),
}

impl MockTransport {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Queue a JSON response.
    pub fn push_json(&self, status: u16, body: &str) -> &Self {
        self.push(HttpResponse::json(status, body))
    }

    /// Queue a response with extra headers.
    pub fn push_with_headers(&self, status: u16, body: &str, headers: &[(&str, &str)]) -> &Self {
        let mut all = vec![("content-type".to_owned(), "application/json".to_owned())];
        all.extend(
            headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        self.push(HttpResponse::new(status, all, body.as_bytes().to_vec()))
    }

    /// Queue a fully built response.
    pub fn push(&self, response: HttpResponse) -> &Self {
        if let Ok(mut queue) = self.queued.lock() {
            queue.push_back(Outcome::Response(response));
        }
        self
    }

    /// Queue a failure to reach the server at all.
    pub fn push_transport_error(&self, message: &str) -> &Self {
        if let Ok(mut queue) = self.queued.lock() {
            queue.push_back(Outcome::Transport(message.to_owned()));
        }
        self
    }

    /// Every request handed to this transport, in order.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.seen.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// The single request this transport saw. Panics if it saw a different
    /// number, which is itself the assertion most of these tests want.
    pub fn only_request(&self) -> HttpRequest {
        let requests = self.requests();
        assert_eq!(requests.len(), 1, "expected exactly one request");
        requests.into_iter().next().expect("checked length")
    }

    pub fn request_count(&self) -> usize {
        self.requests().len()
    }
}

impl Transport for MockTransport {
    fn execute(&self, request: &HttpRequest) -> Result<HttpResponse, ChatlyError> {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(request.clone());
        }
        let next = self.queued.lock().ok().and_then(|mut q| q.pop_front());
        match next {
            Some(Outcome::Response(response)) => Ok(response),
            Some(Outcome::Transport(message)) => Err(ChatlyError::transport(message)),
            None => panic!(
                "MockTransport ran out of scripted responses at request {} ({} {})",
                self.request_count(),
                request.method,
                request.url
            ),
        }
    }
}

/// A client on the **production default base URL**, talking to `mock`.
///
/// Deliberately does not call `.base_url(...)`. CONTRACT.md §1 records that
/// four SDKs shipped a dev host as their default and no test caught it
/// "because every test passes an explicit URL" — so the tests here assert
/// the URL the client actually built instead.
pub fn client(mock: &Arc<MockTransport>) -> ChatlyClient {
    ChatlyClient::builder("ck_test_key")
        .shared_transport(mock.clone())
        .retry(no_retries())
        .build()
        .expect("valid client")
}

/// A client that retries with no sleeping, so retry tests stay instant.
pub fn retrying_client(mock: &Arc<MockTransport>, max_attempts: u32) -> ChatlyClient {
    ChatlyClient::builder("ck_test_key")
        .shared_transport(mock.clone())
        .retry(RetryPolicy {
            max_attempts,
            initial_backoff: std::time::Duration::ZERO,
            max_backoff: std::time::Duration::ZERO,
            respect_retry_after: false,
        })
        .build()
        .expect("valid client")
}

pub fn no_retries() -> RetryPolicy {
    RetryPolicy::none()
}

// ---------------------------------------------------------------------------
// Per-route response shapes.
//
// A fake transport that answers every route with `{"items":[],"nextCursor":
// null}` agrees with any mistake about which routes are paginated. That is
// not hypothetical: the shared manifest briefly marked `GET /v1/macros`
// `paged: true`, this SDK's `macros().list` deserialised into `Page`, and
// real callers got `ChatlyError::Decode` while the suite stayed green —
// because the page envelope the fake returned was the one shape the route
// never sends. The .NET SDK had the identical pair of bugs.
//
// So the fake answers each route the shape that route really returns, and
// `tests/surface_lock.rs` cross-checks this table against the manifest's
// `paged` flags in both directions.
// ---------------------------------------------------------------------------

/// The top-level JSON shape a route answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyShape {
    /// `{ "items": [...], "nextCursor": … }` — a cursor page.
    Page,
    /// A bare top-level JSON array.
    Array,
    /// A single top-level JSON object that is not a page.
    Object,
}

impl BodyShape {
    /// A body of this shape.
    ///
    /// **Non-empty on purpose.** The empty forms are not distinguishable by
    /// a lenient deserialiser — `[]`, `{}` and
    /// `{"items":[],"nextCursor":null}` were all accepted by a derived
    /// `Page`, so a fake that answered `[]` could not tell a correct
    /// `macros().list` from one that still deserialised into a `Page`. Each
    /// of these carries a row, so the three shapes are actually distinct.
    pub fn body(self) -> &'static str {
        match self {
            Self::Page => r#"{"items":[{"id":"row_1"}],"nextCursor":null}"#,
            Self::Array => r#"[{"id":"row_1"}]"#,
            Self::Object => r#"{"id":"row_1"}"#,
        }
    }

    /// A body of this shape carrying one row, for tests that read a value
    /// back out.
    pub fn body_with_row(self, row: &str) -> String {
        match self {
            Self::Page => format!(r#"{{"items":[{row}],"nextCursor":null}}"#),
            Self::Array => format!("[{row}]"),
            Self::Object => row.to_owned(),
        }
    }
}

/// What `ns.name` answers with.
///
/// Every entry in `packages/sdk-spec/surface.json` must appear here;
/// `tests/surface_lock.rs` fails if one does not, so a new manifest method
/// cannot quietly inherit a default shape.
pub fn shape_for(ns: &str, name: &str) -> Option<BodyShape> {
    let shape = match (ns, name) {
        // The 7 genuinely cursor-paginated routes.
        ("contacts", "list")
        | ("conversations", "list")
        | ("conversations", "messages")
        | ("tickets", "list")
        | ("kb", "articles")
        | ("events", "list")
        | ("webhooks", "deliveries") => BodyShape::Page,

        // Bare arrays. `macros.list` is the one this table exists for.
        ("macros", "list") => BodyShape::Array,

        // Everything else is a single object: one entity, a report, a
        // settings blob, a write's result, or a 204 (which this crate
        // decodes to `Value::Null` and which `Object` stands in for here).
        _ => BodyShape::Object,
    };
    Some(shape)
}
