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

    /// The name this shape has in `surface.json` and in every lock file.
    ///
    /// The lock's `shape` column is this string, so the four spellings are
    /// cross-language wire values and not local cosmetics.
    pub fn name(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Array => "array",
            Self::Object => "object",
            Self::Empty => "empty",
        }
    }
}

/// What `ns.name` answers with, for all 55 manifest methods.
///
/// ═══ READ THIS BEFORE CHANGING A VALUE ══════════════════════════════════
///
/// Hand-written, from each handler's `@ZodResponse(...)` schema — or from the
/// service return type it forwards where the handler has no such decorator.
/// **Deliberately NOT read from `surface.json`**, even though this module
/// loads that manifest for paths and `paged` flags.
///
/// A fixture derived from the manifest agrees with whatever the manifest
/// says, including when it is wrong, and that is not hypothetical:
/// `GET /v1/macros` was marked `paged: true` for a day while its handler is
/// `@PaginationExempt` and answers a bare array. This crate's `macros().list`
/// deserialised that into a `Page`; worse than the .NET client's decode
/// error, a derived `Page` read the bare array as an **empty page** and
/// handed it back with a 200 — a wrong answer rather than a missing one. The
/// suite stayed green because the fake answered every route with
/// `{ items, nextCursor }`, the one shape that route never sends.
///
/// So this table is a second opinion. `tests/surface_lock.rs` feeds each
/// method the shape declared HERE, asserts the value it hands back really
/// has that shape, and writes this column into `surface.lock.json`;
/// `scripts/check-sdk-surface-parity.mjs` then diffs eight independently
/// written columns against the manifest. A wrong manifest row needs eight
/// authors to misread the same handler.
///
/// Returns `None` for anything not listed, and `tests/surface_lock.rs`
/// treats that as a failure — there is deliberately **no catch-all arm**, so
/// a method added to the manifest cannot quietly inherit a default shape.
/// That matters because the default used to be `Page`, for every route.
///
/// Three things in here are worth knowing before you trust a shape:
///
/// - **Fourteen routes return a bare array**, not an envelope. All of them
///   are `@PaginationExempt`, so there is no cursor to ask for more.
/// - **Three rows look wrong and are not.** `contacts.remove_tag` is a
///   DELETE answering with the surviving contact, and
///   `conversations.cancel_scheduled` a DELETE answering with the cancelled
///   row, so two of this surface's three deletes have bodies. Two of the
///   four report routes are bare arrays.
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

        // Page plus `tier` and `facets`.
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

/// What a method handed back, in a form the suite can classify.
///
/// The value matters, not only that the call returned `Ok`. A method that
/// unwrapped a page into its `items`, or wrapped a bare array into a
/// one-element envelope, issues a *perfect* request and hands back the wrong
/// thing — and nothing here could see that while the invocation table threw
/// the value away with `.map(|_| ())`.
#[derive(Debug)]
pub enum Returned {
    /// An untyped `serde_json::Value`, as most methods on this client return.
    Value(serde_json::Value),
    /// A typed [`chatly::Page`], as the eight `Page`-returning methods do.
    Page(chatly::Page<serde_json::Value>),
}

impl Returned {
    /// Wrap an untyped return value. Written as a function so the invocation
    /// table can say `.map(Returned::value)`.
    pub fn value(value: serde_json::Value) -> Self {
        Self::Value(value)
    }

    /// Wrap a typed page return value.
    pub fn page(page: chatly::Page<serde_json::Value>) -> Self {
        Self::Page(page)
    }

    /// The shape this value really has, in [`BodyShape`]'s vocabulary.
    ///
    /// Structural for a `Value`, the same test the Node suite's `actualShape`
    /// makes: a JSON array is `Array`; `null` — which is what this crate
    /// decodes a 204 to — is `Empty`; an object carrying an `items` array
    /// *and* a `nextCursor` key is `Page`; any other object is `Object`.
    ///
    /// A typed `Page<T>` can only answer `Page`, because its deserialiser
    /// refuses every other body. So for those the shape is not the
    /// interesting question and [`Self::carries_the_fixture_row`] is: a
    /// method that returned `Page::default()` without decoding anything
    /// would classify as a perfectly good page here.
    pub fn shape(&self) -> BodyShape {
        match self {
            Self::Page(_) => BodyShape::Page,
            Self::Value(serde_json::Value::Null) => BodyShape::Empty,
            Self::Value(serde_json::Value::Array(_)) => BodyShape::Array,
            Self::Value(serde_json::Value::Object(map)) => {
                if map.get("items").is_some_and(serde_json::Value::is_array)
                    && map.contains_key("nextCursor")
                {
                    BodyShape::Page
                } else {
                    BodyShape::Object
                }
            }
            // A scalar is not one of the four. Reported as `Object` would
            // hide it, so it is reported as the shape it is least like.
            Self::Value(_) => BodyShape::Array,
        }
    }

    /// Whether the single `{"id":"row_1"}` row the fake sends survived the
    /// trip, wherever that row belongs for this shape.
    ///
    /// `Empty` carries nothing and is vacuously true; everything else must
    /// show the row, which is what catches a method that decodes into a
    /// default value, or into a type that silently drops the payload.
    pub fn carries_the_fixture_row(&self) -> bool {
        let row = |value: Option<&serde_json::Value>| {
            value
                .and_then(|v| v.get("id"))
                .and_then(serde_json::Value::as_str)
                == Some("row_1")
        };
        match self {
            Self::Page(page) => page.items.len() == 1 && row(page.items.first()),
            Self::Value(serde_json::Value::Null) => true,
            Self::Value(value @ serde_json::Value::Object(map)) => match map.get("items") {
                Some(serde_json::Value::Array(items)) => items.len() == 1 && row(items.first()),
                _ => row(Some(value)),
            },
            Self::Value(serde_json::Value::Array(items)) => items.len() == 1 && row(items.first()),
            Self::Value(_) => false,
        }
    }
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
///
/// TWO candidates, and the second one is why this is a function. In the
/// monorepo the spec sits beside this package at `../sdk-spec/`. In the public
/// mirror there is no `packages/` directory, so `sync-sdk-repos.mjs` vendors
/// it to `<repo>/sdk-spec/` instead.
///
/// Looking only at the monorepo path is a mistake this repository has now made
/// four times — three SDKs once, and these shape tests again. It is invisible
/// locally, where the first candidate always resolves, and the mirror CI is
/// the only thing that runs the other case.
///
/// Falls back to the monorepo path when neither exists, so the panic below
/// names the canonical location rather than wherever we looked last.
pub fn manifest_path() -> std::path::PathBuf {
    let package = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for candidate in [package.join("../sdk-spec"), package.join("sdk-spec")] {
        let path = candidate.join("surface.json");
        if path.exists() {
            return path;
        }
    }
    package.join("../sdk-spec/surface.json")
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

/// Fold concrete path segments back to the manifest's `{...}` names, by
/// position.
///
/// Every non-parameter segment is kept as the client actually produced it, so
/// a method that hits `/v1/contact/{id}` instead of `/v1/contacts/{id}` is
/// reported by the caller's comparison rather than normalised into agreement.
pub fn normalise(actual: &str, manifest: &str) -> String {
    let actual_segments: Vec<&str> = actual.split('/').collect();
    let manifest_segments: Vec<&str> = manifest.split('/').collect();
    if actual_segments.len() != manifest_segments.len() {
        // Return the actual path untouched; the caller's comparison reports
        // it, which is more useful than a panic here.
        return actual.to_owned();
    }
    manifest_segments
        .iter()
        .zip(actual_segments.iter())
        .map(|(expected, got)| {
            if expected.starts_with('{') && expected.ends_with('}') {
                (*expected).to_owned()
            } else {
                (*got).to_owned()
            }
        })
        .collect::<Vec<String>>()
        .join("/")
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
