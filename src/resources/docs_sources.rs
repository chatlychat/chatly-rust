//! Documentation sources — a customer's existing docs site, indexed into the
//! knowledge base.
//!
//! Connect a provider and a worker pulls its pages into KB articles the AI
//! can cite. The usual flow is
//! [`providers`](DocsSources::providers) → [`discover`](DocsSources::discover)
//! → [`create`](DocsSources::create) → optionally
//! [`webhook`](DocsSources::webhook).

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `provider` values these routes accept.
///
/// Five, as the API's enum spells them — `website` is the sitemap reader for
/// a docs site with no API, and `forumulate` is Chatly's own docs renderer.
/// [`DocsSources::providers`] returns the same list with the per-provider
/// flags, which is what a connect form should render from.
pub const DOCS_PROVIDERS: [&str; 5] = ["gitbook", "forumulate", "discourse", "readme", "website"];

/// The `docs_sources` namespace. Get one from
/// [`ChatlyClient::docs_sources`].
#[derive(Debug, Clone, Copy)]
pub struct DocsSources<'a> {
    client: &'a ChatlyClient,
}

impl<'a> DocsSources<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/docs-sources/providers` — the registry, as a **bare JSON
    /// array**.
    ///
    /// Static and tenant-free. Each row carries `needsBaseUrl` and
    /// `needsToken`, which is why a connect form should read this rather
    /// than keep its own list of which providers are self-hosted — that copy
    /// goes stale the first time one is added.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn providers(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/docs-sources/providers", query)
    }

    /// `GET /v1/docs-sources` — the connected sources, as a **bare JSON
    /// array**.
    ///
    /// No field carries the stored token back. Pagination-exempt: a
    /// workspace connects a handful.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/docs-sources", query)
    }

    /// `POST /v1/docs-sources/discover` — which spaces a token can reach,
    /// as a **bare JSON array**.
    ///
    /// A POST that answers an array, which is unusual on this surface and
    /// worth noting if you are decoding by route shape: the body carries a
    /// *candidate* token (`provider`, `token`, optional `baseUrl`, optional
    /// `organizationId`) and nothing is stored, so it could not be a GET.
    ///
    /// **Pass `organizationId` when you have one.** An org-scoped Forumulate
    /// token cannot read the provider's `GET /orgs`, so discovery without it
    /// fails the whole connect flow with an error naming a path the operator
    /// never asked about; and an account-wide token without it returns every
    /// space in every organisation, which was 60 unrelated spaces for real
    /// customers.
    ///
    /// Answers **201** despite creating nothing.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an unknown
    /// provider or a token the provider rejects.
    pub fn discover(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            "/v1/docs-sources/discover",
            &Query::new(),
            Some(body),
        )
    }

    /// `POST /v1/docs-sources` — connect a provider.
    ///
    /// `provider`, `token`, `organizationId` and `spaceId` are required —
    /// note `organizationId` is *optional* on
    /// [`discover`](DocsSources::discover) and *required* here. Then
    /// `baseUrl`, `label`, `publishOnSync`, `syncIntervalMinutes` (15 to
    /// 10,080 — a shorter interval than the 15-minute scan cadence cannot be
    /// honoured, so it is refused rather than accepted and ignored),
    /// `includePaths` and `excludePaths`.
    ///
    /// Path filters are only meaningful for `forumulate` and `website`; the
    /// service **refuses** them for the other three rather than storing a
    /// filter that can never match. Prefixes match at a segment boundary, so
    /// `/guides/selling` does not capture `/guides/selling-tips`, and an
    /// exclude beats an include. Two sources over one space with different
    /// prefixes get their own collections, which is how per-audience help
    /// scoping is expressed.
    ///
    /// **The first sync is enqueued before this returns**, so the source
    /// comes back with `syncStatus: "queued"` and no counts — that is not an
    /// error state. Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a refused path
    /// filter, an out-of-range interval, or a token that cannot see the
    /// space.
    pub fn create(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/docs-sources", &Query::new(), Some(body))
    }

    /// `PATCH /v1/docs-sources/{id}` — rename, pause, re-schedule, or rotate
    /// the token.
    ///
    /// `label`, `status` (`active` | `paused`), `publishOnSync`,
    /// `syncIntervalMinutes`, `token`, `baseUrl` — all optional.
    ///
    /// **A replacement `token` is verified against the provider before it is
    /// stored**, and specifically against *this source's space*: a token that
    /// authenticates but cannot see the space is refused and the working
    /// credential is left in place. Nothing echoes a token back.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a token that
    /// cannot see the space, [`ChatlyError::NotFound`] if no such source.
    pub fn update(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/docs-sources/{}", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `POST /v1/docs-sources/{id}/sync` — enqueue the same job the
    /// scheduler runs.
    ///
    /// Body: `{ "full": bool }`, optional. **Send `full: true` whenever the
    /// reader changed rather than the content.** A source holding a
    /// checkpoint answers an ordinary "sync now" with a delta, so after a
    /// provider-side fix the run correctly finds nothing new and the
    /// knowledge base stays as wrong as it was — which is exactly what
    /// happened on 2026-10-01, after a fix for a provider that had indexed 6
    /// of 100 pages. `full: true` resets the checkpoint first.
    ///
    /// Answers `{ queued: true, full }` with a **201**, and the queue is the
    /// point: this returns before the sync has run, so poll
    /// [`DocsSources::list`] for `syncStatus`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such source.
    pub fn sync(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!("/v1/docs-sources/{}/sync", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// [`DocsSources::sync`] with `{ "full": … }` built for you.
    ///
    /// `full = false` sends the field explicitly rather than omitting it;
    /// the server treats the two identically here.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn sync_now(&self, id: &str, full: bool) -> Result<Value, ChatlyError> {
        self.sync(id, &serde_json::json!({ "full": full }))
    }

    /// `POST /v1/docs-sources/{id}/webhook` — subscribe the source to its
    /// provider's change events.
    ///
    /// Push closes the gap between "the docs changed" and "the bot knows"
    /// from one sync interval — six hours by default — to a round trip.
    ///
    /// **`subscribed: false` with a `reason` is a success, not an error**,
    /// and it is the common case: customers paste read-only tokens, and
    /// Forumulate needs `webhooks:manage` or `admin` to create an endpoint.
    /// The source keeps polling either way, and the reason names the scope to
    /// grant. So check the field; a 201 alone does not mean push is live.
    ///
    /// Takes no body. Answers **201**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such source.
    pub fn webhook(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!("/v1/docs-sources/{}/webhook", encode_path_segment(id)),
            &Query::new(),
            None,
        )
    }

    /// `DELETE /v1/docs-sources/{id}` — disconnect, **keeping the articles**.
    ///
    /// Answers `{ kept: … }` with a 200: `kept` is how many articles stayed
    /// behind, and none are deleted. So this stops the sync and leaves the
    /// knowledge base as it stands — if you want the articles gone, that is a
    /// separate job through the KB routes.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such source.
    pub fn delete(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/docs-sources/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_names_match_the_apis_enum() {
        assert_eq!(DOCS_PROVIDERS.len(), 5);
        for expected in ["gitbook", "forumulate", "discourse", "readme", "website"] {
            assert!(DOCS_PROVIDERS.contains(&expected), "{expected}");
        }
        // Stainless has no docs API, so there is no connector for it.
        assert!(!DOCS_PROVIDERS.contains(&"stainless"));
    }
}
