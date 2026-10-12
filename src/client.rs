//! The client: construction, request shaping, retries, and the escape hatch.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::crypto;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::Query;
use crate::resources;
use crate::retry::{is_write, parse_retry_after, RetryPolicy};
use crate::transport::{HttpRequest, HttpResponse, Transport, UreqTransport};

/// The production API host.
///
/// CONTRACT.md §1 is blunt about this and so is the git history: four SDKs
/// shipped with `api.livechat.local` or `api.chatly.example` as the default,
/// both of which resolve nowhere, so every call failed for anyone who
/// followed the docs and did not override it. No test caught it because
/// every test passed an explicit URL. A caller that needs another environment
/// overrides it;
/// it is the only caller that ever should.
pub const DEFAULT_BASE_URL: &str = "https://api.chatlychat.com";

/// Default per-request deadline (CONTRACT.md §1).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// The `User-Agent` this crate sends: `chatly-sdk-rust/<crate version>`.
pub const USER_AGENT: &str = concat!("chatly-sdk-rust/", env!("CARGO_PKG_VERSION"));

/// Step-by-step construction of a [`ChatlyClient`].
///
/// Use [`ChatlyClient::builder`] to get one.
#[derive(Clone)]
pub struct ChatlyClientBuilder {
    api_key: String,
    base_url: String,
    timeout: Duration,
    retry: RetryPolicy,
    transport: Option<Arc<dyn Transport>>,
    user_agent: String,
}

impl std::fmt::Debug for ChatlyClientBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key is deliberately not printed. A workspace API key in a log
        // line is a workspace-wide credential in a log line.
        f.debug_struct("ChatlyClientBuilder")
            .field("base_url", &self.base_url)
            .field("timeout", &self.timeout)
            .field("retry", &self.retry)
            .field("user_agent", &self.user_agent)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl ChatlyClientBuilder {
    /// Override the API host. Defaults to [`DEFAULT_BASE_URL`].
    ///
    /// Only another environment or a test server needs this. A trailing slash is
    /// trimmed for you.
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Override the per-request deadline. Defaults to [`DEFAULT_TIMEOUT`].
    ///
    /// Ignored if you also supply your own [`ChatlyClientBuilder::transport`],
    /// which owns its own connection settings.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Override the retry policy. Defaults to [`RetryPolicy::default`].
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Substitute the HTTP layer — a fake in tests, or your own pool.
    pub fn transport(mut self, transport: impl Transport) -> Self {
        self.transport = Some(Arc::new(transport));
        self
    }

    /// Substitute the HTTP layer with an already-shared transport.
    pub fn shared_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Append a token to the `User-Agent`, e.g. your own product name.
    ///
    /// The result is `chatly-sdk-rust/<version> <token>`; the SDK's own
    /// identity is never replaced, because we use it to tell SDK traffic
    /// from hand-rolled HTTP in our own metrics.
    pub fn user_agent_suffix(mut self, suffix: impl AsRef<str>) -> Self {
        let suffix = suffix.as_ref().trim();
        if !suffix.is_empty() {
            self.user_agent = format!("{USER_AGENT} {suffix}");
        }
        self
    }

    /// Validate and build.
    ///
    /// # Errors
    ///
    /// [`ChatlyError::Config`] if the API key or the base URL is empty, or if
    /// the base URL has no `http://` or `https://` scheme. CONTRACT.md §1
    /// requires an empty key be rejected here and not at the first call.
    pub fn build(self) -> Result<ChatlyClient, ChatlyError> {
        if self.api_key.trim().is_empty() {
            return Err(ChatlyError::Config(
                "api key is empty — pass the workspace `ck_…` key".to_owned(),
            ));
        }
        let base_url = self.base_url.trim().trim_end_matches('/').to_owned();
        if base_url.is_empty() {
            return Err(ChatlyError::Config("base url is empty".to_owned()));
        }
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(ChatlyError::Config(format!(
                "base url must start with http:// or https://, got {base_url:?}"
            )));
        }
        let transport = match self.transport {
            Some(transport) => transport,
            None => Arc::new(UreqTransport::new(self.timeout)),
        };
        Ok(ChatlyClient {
            inner: Arc::new(Inner {
                api_key: self.api_key,
                base_url,
                timeout: self.timeout,
                retry: self.retry,
                transport,
                user_agent: self.user_agent,
                pinned_idempotency_key: None,
            }),
        })
    }
}

struct Inner {
    api_key: String,
    base_url: String,
    timeout: Duration,
    retry: RetryPolicy,
    transport: Arc<dyn Transport>,
    user_agent: String,
    pinned_idempotency_key: Option<String>,
}

/// A handle on one workspace's API, authenticated by one `ck_…` key.
///
/// Cheap to clone: everything behind the handle is shared, so clone it
/// freely across threads rather than building a second client (which would
/// build a second connection pool).
///
/// **Server-side only.** The key is a bearer credential for the entire
/// workspace; shipping it to a browser hands every contact, conversation and
/// report to anyone who opens the network tab. The browser SDK is
/// `@chatlychat/sdk-js`.
#[derive(Clone)]
pub struct ChatlyClient {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for ChatlyClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatlyClient")
            .field("base_url", &self.inner.base_url)
            .field("timeout", &self.inner.timeout)
            .field("retry", &self.inner.retry)
            .field("pinned_idempotency_key", &self.inner.pinned_idempotency_key)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl ChatlyClient {
    /// A client against the production API with default timeout and retries.
    ///
    /// # Errors
    ///
    /// [`ChatlyError::Config`] if `api_key` is empty or whitespace.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), chatly::ChatlyError> {
    /// let chatly = chatly::ChatlyClient::new(std::env::var("CHATLY_API_KEY").unwrap_or_default())?;
    /// let me = chatly.whoami()?;
    /// println!("{me}");
    /// # Ok(()) }
    /// ```
    pub fn new(api_key: impl Into<String>) -> Result<Self, ChatlyError> {
        Self::builder(api_key).build()
    }

    /// Start configuring a client.
    pub fn builder(api_key: impl Into<String>) -> ChatlyClientBuilder {
        ChatlyClientBuilder {
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            timeout: DEFAULT_TIMEOUT,
            retry: RetryPolicy::default(),
            transport: None,
            user_agent: USER_AGENT.to_owned(),
        }
    }

    /// The base URL in use, without a trailing slash.
    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    /// The retry policy in use.
    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.inner.retry
    }

    /// The pinned `Idempotency-Key`, if this client has one.
    pub fn pinned_idempotency_key(&self) -> Option<&str> {
        self.inner.pinned_idempotency_key.as_deref()
    }

    /// A derived client that sends `key` as the `Idempotency-Key` on every
    /// write, instead of minting a fresh UUID per call.
    ///
    /// Use it to retry the *same* logical operation across process restarts
    /// or across your own retry loop: same key + same body replays the first
    /// response, same key + a different body is a 409
    /// ([`ChatlyError::Conflict`]).
    ///
    /// The original client is untouched, and the derived one shares its
    /// transport and connection pool.
    ///
    /// # Errors
    ///
    /// [`ChatlyError::Config`] if `key` is empty or whitespace.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), chatly::ChatlyError> {
    /// # let chatly = chatly::ChatlyClient::new("ck_x")?;
    /// # let body = serde_json::json!({});
    /// let pinned = chatly.with_idempotency_key("order-4711-welcome")?;
    /// pinned.contacts().upsert(&body)?;
    /// # Ok(()) }
    /// ```
    pub fn with_idempotency_key(&self, key: impl Into<String>) -> Result<Self, ChatlyError> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err(ChatlyError::Config("idempotency key is empty".to_owned()));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                api_key: self.inner.api_key.clone(),
                base_url: self.inner.base_url.clone(),
                timeout: self.inner.timeout,
                retry: self.inner.retry.clone(),
                transport: Arc::clone(&self.inner.transport),
                user_agent: self.inner.user_agent.clone(),
                pinned_idempotency_key: Some(key),
            }),
        })
    }

    /// Describe the credential this client is using.
    ///
    /// `GET /v1/api-keys/whoami` is the only route on the api-keys controller
    /// that accepts an API key at all — managing keys with a key is a
    /// privilege-escalation shape. Make it your first call: it turns a
    /// mis-scoped key into an obvious error at integration time instead of a
    /// 403 from an unrelated endpoint three weeks later.
    ///
    /// The response carries the workspace id and the granted scopes. It is
    /// returned untyped because this crate does not model response bodies —
    /// see the crate-level docs for why.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Authentication`] means the key is
    /// not valid.
    pub fn whoami(&self) -> Result<Value, ChatlyError> {
        self.get("/v1/api-keys/whoami", &Query::new())
    }

    /// Contacts: list, get, upsert, bulk upsert, merge, tags, unsubscribe,
    /// journey.
    pub fn contacts(&self) -> resources::Contacts<'_> {
        resources::Contacts::new(self)
    }

    /// Conversations: list, get, messages, send, unattended, agent handling,
    /// generated replies, scheduled messages.
    pub fn conversations(&self) -> resources::Conversations<'_> {
        resources::Conversations::new(self)
    }

    /// Tickets: list, comments, for-conversation, queues, merge, split, bulk.
    pub fn tickets(&self) -> resources::Tickets<'_> {
        resources::Tickets::new(self)
    }

    /// Macros: list, create, update, delete.
    pub fn macros(&self) -> resources::Macros<'_> {
        resources::Macros::new(self)
    }

    /// Knowledge base: articles, one article by slug, collections.
    pub fn kb(&self) -> resources::Kb<'_> {
        resources::Kb::new(self)
    }

    /// Full-text search across contacts, conversations, messages, tickets
    /// and KB articles.
    pub fn search(&self) -> resources::Search<'_> {
        resources::Search::new(self)
    }

    /// CDP events: track and read back.
    pub fn events(&self) -> resources::Events<'_> {
        resources::Events::new(self)
    }

    /// Webhook deliveries: list, get, redeliver.
    pub fn webhooks(&self) -> resources::Webhooks<'_> {
        resources::Webhooks::new(self)
    }

    /// Reports: overview, agents, conversations by day, SLA.
    pub fn reports(&self) -> resources::Reports<'_> {
        resources::Reports::new(self)
    }

    /// Channels: list and get.
    pub fn channels(&self) -> resources::Channels<'_> {
        resources::Channels::new(self)
    }

    /// Billing: plans and usage.
    pub fn billing(&self) -> resources::Billing<'_> {
        resources::Billing::new(self)
    }

    /// Workspace configuration: settings, labels, inboxes, team rosters and
    /// the agent-skill taxonomy.
    pub fn workspace(&self) -> resources::Workspace<'_> {
        resources::Workspace::new(self)
    }

    /// The escape hatch: call any key-accessible route by method and path.
    ///
    /// 140 of the API's 886 routes accept a `ck_` key;
    /// `packages/sdk-spec/surface.json` hand-writes methods for 55 of them.
    /// This is how you reach the other 85 without abandoning the SDK — it
    /// goes through the same auth, `Idempotency-Key`, retry and error
    /// handling as every typed method above.
    ///
    /// `method` is case-insensitive. `POST`, `PUT`, `PATCH` and `DELETE` are
    /// treated as writes and get an `Idempotency-Key`. `path` is appended to
    /// the base URL; a leading `/` is added if you omit it.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    ///
    /// ```no_run
    /// # fn main() -> Result<(), chatly::ChatlyError> {
    /// # let chatly = chatly::ChatlyClient::new("ck_x")?;
    /// use chatly::Query;
    /// let out = chatly.request(
    ///     "POST",
    ///     "/v1/some/route",
    ///     &Query::new(),
    ///     Some(&serde_json::json!({ "hello": "world" })),
    /// )?;
    /// println!("{out}");
    /// # Ok(()) }
    /// ```
    pub fn request(
        &self,
        method: &str,
        path: &str,
        query: &Query,
        body: Option<&Value>,
    ) -> Result<Value, ChatlyError> {
        self.send_json(method, path, query, body)
    }

    /// [`ChatlyClient::request`], deserialising into a type of your choosing.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Decode`] if the response does not
    /// fit `T`.
    pub fn request_as<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        query: &Query,
        body: Option<&Value>,
    ) -> Result<T, ChatlyError> {
        let value = self.send_json(method, path, query, body)?;
        decode_value(value)
    }

    /// Auto-paginate any cursor-paginated route, including one this crate
    /// has no typed method for.
    ///
    /// Returns an [`Iterator`] over items, fetching each page as needed. The
    /// `cursor` parameter of `query` is managed for you and any value you
    /// set is used as the starting point.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), chatly::ChatlyError> {
    /// # let chatly = chatly::ChatlyClient::new("ck_x")?;
    /// use chatly::Query;
    /// for row in chatly.paginate::<serde_json::Value>("/v1/contacts", &Query::new().limit(100)) {
    ///     println!("{}", row?);
    /// }
    /// # Ok(()) }
    /// ```
    pub fn paginate<T: DeserializeOwned + 'static>(
        &self,
        path: &str,
        query: &Query,
    ) -> PageIter<T> {
        let client = self.clone();
        let path = path.to_owned();
        let base_query = query.clone();
        let start = base_query.get("cursor").map(str::to_owned);
        PageIter::new(start, move |cursor| {
            let page_query = match cursor {
                Some(cursor) => base_query.clone().cursor(cursor),
                None => base_query.clone().remove("cursor"),
            };
            client.get_page(&path, &page_query)
        })
    }

    // ---- internals used by the resource modules ----

    pub(crate) fn get(&self, path: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.send_json("GET", path, query, None)
    }

    pub(crate) fn get_page<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &Query,
    ) -> Result<Page<T>, ChatlyError> {
        let value = self.send_json("GET", path, query, None)?;
        decode_value(value)
    }

    pub(crate) fn write<B: Serialize>(
        &self,
        method: &str,
        path: &str,
        query: &Query,
        body: Option<&B>,
    ) -> Result<Value, ChatlyError> {
        let encoded = match body {
            Some(body) => Some(serde_json::to_vec(body).map_err(|err| {
                ChatlyError::Config(format!("could not serialize request body as JSON: {err}"))
            })?),
            None => None,
        };
        let response = self.send(method, path, query, encoded)?;
        decode_response(&response)
    }

    pub(crate) fn delete(&self, path: &str, query: &Query) -> Result<Value, ChatlyError> {
        let response = self.send("DELETE", path, query, None)?;
        decode_response(&response)
    }

    fn send_json(
        &self,
        method: &str,
        path: &str,
        query: &Query,
        body: Option<&Value>,
    ) -> Result<Value, ChatlyError> {
        self.write(method, path, query, body)
    }

    /// Build the request once, then attempt it up to `max_attempts` times.
    ///
    /// The `Idempotency-Key` is minted **before** the loop so every attempt
    /// carries the same one (CONTRACT.md §4). Minting inside the loop would
    /// make an SDK-level retry of a POST a second write.
    fn send(
        &self,
        method: &str,
        path: &str,
        query: &Query,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, ChatlyError> {
        let method = method.to_ascii_uppercase();
        let mut url = String::with_capacity(self.inner.base_url.len() + path.len() + 32);
        url.push_str(&self.inner.base_url);
        if !path.starts_with('/') {
            url.push('/');
        }
        url.push_str(path);
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.to_query_string());
        }

        let mut headers = vec![
            (
                "authorization".to_owned(),
                format!("Bearer {}", self.inner.api_key),
            ),
            ("accept".to_owned(), "application/json".to_owned()),
            ("user-agent".to_owned(), self.inner.user_agent.clone()),
        ];
        if body.is_some() {
            headers.push(("content-type".to_owned(), "application/json".to_owned()));
        }
        if is_write(&method) {
            let key = self
                .inner
                .pinned_idempotency_key
                .clone()
                .unwrap_or_else(crypto::uuid_v4);
            headers.push(("idempotency-key".to_owned(), key));
        }

        let request = HttpRequest {
            method,
            url,
            headers,
            body,
            timeout: self.inner.timeout,
        };

        let max_attempts = self.inner.retry.max_attempts.max(1);
        let mut attempt = 1u32;
        loop {
            let error = match self.inner.transport.execute(&request) {
                Ok(response) if response.is_success() => return Ok(response),
                Ok(response) => {
                    let request_id = response.header("x-request-id").map(str::to_owned);
                    let retry_after = parse_retry_after(response.header("retry-after"));
                    ChatlyError::from_http(
                        response.status,
                        response.body_text(),
                        request_id,
                        retry_after,
                    )
                }
                Err(error) => error,
            };
            if attempt >= max_attempts || !error.is_retryable() {
                return Err(error);
            }
            let delay = self.inner.retry.backoff_for(attempt, error.retry_after());
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            attempt += 1;
        }
    }
}

/// Decode a success response body into JSON.
///
/// A 204, or any empty body, decodes to [`Value::Null`] rather than an error:
/// several write routes answer with no content and a caller should not have
/// to special-case that.
fn decode_response(response: &HttpResponse) -> Result<Value, ChatlyError> {
    if response.status == 204 || response.body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&response.body).map_err(|err| ChatlyError::Decode {
        message: format!("response was not valid JSON: {err}"),
        body: response.body_text(),
    })
}

fn decode_value<T: DeserializeOwned>(value: Value) -> Result<T, ChatlyError> {
    let rendered = value.to_string();
    serde_json::from_value(value).map_err(|err| ChatlyError::Decode {
        message: format!("response did not match the expected shape: {err}"),
        body: rendered,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// The production default, asserted here as well as in
    /// `tests/contract.rs` — CONTRACT.md §1 and §10.1. Four sibling SDKs
    /// shipped a dev host as the default and every test passed an explicit
    /// URL, so nothing failed until a customer integrated from the docs.
    #[test]
    fn the_default_base_url_is_production() {
        assert_eq!(DEFAULT_BASE_URL, "https://api.chatlychat.com");
        for bad in [
            "livechat.local",
            "chatly.example",
            "localhost",
            "127.0.0.1",
            "example",
        ] {
            assert!(
                !DEFAULT_BASE_URL.contains(bad),
                "default base url must not reference {bad}"
            );
        }
    }

    #[test]
    fn empty_and_204_bodies_decode_to_null() {
        assert_eq!(
            decode_response(&HttpResponse::new(204, vec![], Vec::new())).unwrap(),
            Value::Null
        );
        assert_eq!(
            decode_response(&HttpResponse::new(200, vec![], b"  \n ".to_vec())).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn a_non_json_success_body_is_a_decode_error_carrying_the_body() {
        let err = decode_response(&HttpResponse::new(200, vec![], b"<html/>".to_vec()))
            .expect_err("should not decode");
        assert!(matches!(err, ChatlyError::Decode { .. }));
        assert_eq!(err.body(), Some("<html/>"));
    }

    #[test]
    fn decode_value_reports_a_shape_mismatch_with_the_body() {
        let err = decode_value::<Page<Value>>(serde_json::json!({ "items": 7 }))
            .expect_err("items is not an array");
        assert!(matches!(err, ChatlyError::Decode { .. }));
        assert!(err.body().unwrap_or_default().contains("7"));
    }
}
