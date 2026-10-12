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

    /// Queue the response shape a route really answers with.
    pub fn push_shape(&self, shape: BodyShape) -> &Self {
        if shape == BodyShape::Empty {
            return self.push(HttpResponse::new(shape.status(), vec![], Vec::new()));
        }
        self.push_json(shape.status(), shape.body())
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
    /// `{ "items": [...], "nextCursor": … }` — a cursor page. Several routes
    /// add siblings (`hasMore`, `total`, `tier`/`facets`); those are the same
    /// shape as far as a client is concerned.
    Page,
    /// A bare top-level JSON array. Fourteen routes do this.
    Array,
    /// A single top-level JSON object that is not a page.
    Object,
    /// `204 No Content`, which this crate decodes to `Value::Null`.
    Empty,
}

impl BodyShape {
    /// The HTTP status to answer with.
    pub fn status(self) -> u16 {
        match self {
            Self::Empty => 204,
            _ => 200,
        }
    }

    /// A body of this shape.
    ///
    /// **Non-empty on purpose.** The empty forms are not distinguishable by
    /// a lenient deserialiser — `[]`, `{}` and
    /// `{"items":[],"nextCursor":null}` were all accepted by a derived
    /// `Page`, so a fake that answered `[]` could not tell a correct
    /// `macros().list` from one that still deserialised into a `Page`. Each
    /// of these carries a row, so the shapes are actually distinct.
    pub fn body(self) -> &'static str {
        match self {
            Self::Page => r#"{"items":[{"id":"row_1"}],"nextCursor":null}"#,
            Self::Array => r#"[{"id":"row_1"}]"#,
            Self::Object => r#"{"id":"row_1"}"#,
            Self::Empty => "",
        }
    }
}

/// What `ns.name` answers with, for all 55 manifest methods.
///
/// Resolved from each handler's `@ZodResponse(...)` schema, or from the
/// service's declared return type where the handler has no such decorator.
/// Returns `None` for anything not listed, and `tests/surface_lock.rs`
/// treats that as a failure — there is deliberately **no catch-all arm**, so
/// a method added to the manifest cannot quietly inherit a default shape.
/// That matters because the default used to be `Page`, for every route.
///
/// Three things in here are worth knowing before you trust a shape:
///
/// - **Fourteen routes return a bare array**, not an envelope. All of them
///   are `@PaginationExempt`, so there is no cursor to ask for more.
/// - **`contacts.journey`, `conversations.unattended` and `search.query`
///   return page envelopes but are NOT marked `paged` in the manifest.** The
///   manifest flag says "this SDK offers an auto-paginator", not "the wire
///   shape is a page", and those three do not get one. So the cross-check in
///   `surface_lock.rs` runs one way only: `paged` implies `Page`, not the
///   converse.
/// - **`macros.delete` is the only 204.**
pub fn shape_for(ns: &str, name: &str) -> Option<BodyShape> {
    use BodyShape::{Array, Empty, Object, Page};
    let shape = match (ns, name) {
        ("root", "whoami") => Object,

        ("contacts", "list") => Page,
        ("contacts", "get") => Object,
        ("contacts", "upsert") => Object,
        ("contacts", "bulk_upsert") => Object,
        ("contacts", "merge") => Object,
        ("contacts", "add_tags") => Object,
        // Answers the surviving contact, not a 204.
        ("contacts", "remove_tag") => Object,
        ("contacts", "unsubscribe") => Object,
        // A page envelope, though the manifest does not mark it paged.
        ("contacts", "journey") => Page,

        ("conversations", "list") => Page,
        ("conversations", "get") => Object,
        ("conversations", "messages") => Page,
        ("conversations", "send") => Object,
        // Page plus a `total` that is not `items.len()`.
        ("conversations", "unattended") => Page,
        ("conversations", "agent_handling") => Object,
        ("conversations", "set_agent_handling") => Object,
        ("conversations", "generated_replies") => Array,
        ("conversations", "generate_reply") => Object,
        ("conversations", "scheduled_messages") => Array,
        ("conversations", "schedule_message") => Object,
        // Answers the scheduled message as it ended up, not a 204.
        ("conversations", "cancel_scheduled") => Object,

        // Page plus `hasMore`.
        ("tickets", "list") => Page,
        ("tickets", "comments") => Array,
        ("tickets", "for_conversation") => Array,
        ("tickets", "queues") => Array,
        ("tickets", "merge") => Object,
        ("tickets", "split") => Object,
        ("tickets", "bulk") => Object,

        // The bare array this whole table exists for.
        ("macros", "list") => Array,
        ("macros", "create") => Object,
        ("macros", "update") => Object,
        ("macros", "delete") => Empty,

        ("kb", "articles") => Page,
        ("kb", "article") => Object,
        ("kb", "collections") => Array,

        // Page plus `tier` and `facets`; not manifest-paged.
        ("search", "query") => Page,

        ("events", "track") => Object,
        ("events", "list") => Page,

        ("webhooks", "deliveries") => Page,
        ("webhooks", "delivery") => Object,
        ("webhooks", "redeliver") => Object,

        ("reports", "overview") => Object,
        ("reports", "agents") => Array,
        ("reports", "conversations_by_day") => Array,
        ("reports", "sla") => Object,

        ("channels", "list") => Array,
        ("channels", "get") => Object,

        ("billing", "plans") => Array,
        ("billing", "usage") => Object,

        ("workspace", "settings") => Object,
        ("workspace", "labels") => Array,
        ("workspace", "inboxes") => Array,
        ("workspace", "team_members") => Array,
        // `{ "skills": [...] }`, not a bare array.
        ("workspace", "agent_skills") => Object,

        _ => return None,
    };
    Some(shape)
}

// ---------------------------------------------------------------------------
// The manifest, loaded once, shared by the suites.
// ---------------------------------------------------------------------------

/// One entry of `packages/sdk-spec/surface.json`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ManifestEntry {
    /// Namespace, e.g. `contacts`.
    pub ns: String,
    /// Method name, e.g. `list`.
    pub name: String,
    /// HTTP verb, uppercase.
    pub http: String,
    /// Path with `{...}` parameters.
    pub path: String,
    /// `true` on the routes that answer `{ items, nextCursor }` and for
    /// which every SDK offers an auto-paginating form. Ten of them.
    #[serde(default)]
    pub paged: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct ManifestFile {
    #[serde(rename = "baseUrl")]
    base_url: String,
    entries: Vec<ManifestEntry>,
}

/// Where the manifest lives, relative to this package.
pub fn manifest_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sdk-spec/surface.json")
}

fn manifest_file() -> &'static ManifestFile {
    static LOADED: std::sync::OnceLock<ManifestFile> = std::sync::OnceLock::new();
    LOADED.get_or_init(|| {
        let path = manifest_path();
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!(
                "cannot read the surface manifest at {}: {err}. This is the \
                 SDK's parity evidence and must not be skipped.",
                path.display()
            )
        });
        serde_json::from_str(&raw).expect("surface.json is valid JSON")
    })
}

/// Every manifest entry, in file order.
pub fn manifest() -> &'static [ManifestEntry] {
    &manifest_file().entries
}

/// The manifest's declared base URL.
pub fn manifest_base_url() -> &'static str {
    &manifest_file().base_url
}

/// Find the manifest entry a concrete request corresponds to.
///
/// Matches on the verb and on every **non-parameter** path segment, with the
/// parameter positions taken from the manifest itself. So
/// `GET /v1/contacts/c_1/tags/vip` resolves to `contacts.remove_tag`'s
/// `/v1/contacts/{id}/tags/{tag}` without the caller naming either.
///
/// This exists so a test can assert a verb and a path and get the route's
/// response shape for free, instead of restating it. Restating it is how a
/// hand-written shape drifts: a sweep of this suite once left `/v1/macros`
/// claiming `Page` while the shape table said `Array`, and only a one-off
/// script caught it. Now there is nothing to drift.
pub fn entry_for_request(method: &str, path: &str) -> Option<&'static ManifestEntry> {
    let path = path.split('?').next().unwrap_or(path);
    let actual: Vec<&str> = path.split('/').collect();
    manifest().iter().find(|entry| {
        if !entry.http.eq_ignore_ascii_case(method) {
            return false;
        }
        let expected: Vec<&str> = entry.path.split('/').collect();
        expected.len() == actual.len()
            && expected
                .iter()
                .zip(actual.iter())
                .all(|(want, got)| (want.starts_with('{') && want.ends_with('}')) || want == got)
    })
}

/// The response shape of the route a concrete request addresses.
///
/// Panics if the path is not a manifest path, or if the manifest entry has
/// no recorded shape — both are failures worth stopping for.
pub fn shape_for_request(method: &str, path: &str) -> BodyShape {
    let entry = entry_for_request(method, path)
        .unwrap_or_else(|| panic!("{method} {path} does not correspond to any surface.json entry"));
    shape_for(&entry.ns, &entry.name)
        .unwrap_or_else(|| panic!("no response shape recorded for {}.{}", entry.ns, entry.name))
}
