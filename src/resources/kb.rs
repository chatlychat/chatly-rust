//! The knowledge base / help centre.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::{encode_path_segment, Query};

/// The `kb` namespace. Get one from [`ChatlyClient::kb`].
///
/// Read-only. Authoring articles is not key-accessible.
#[derive(Debug, Clone, Copy)]
pub struct Kb<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Kb<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/kb/articles` — one page of articles.
    ///
    /// Takes `cursor`, `limit` (1–100, default 25), `sort`, `fields` and
    /// `filter[]`. There is no `?status=` parameter — it was removed —
    /// so to list only live articles, filter:
    /// `Query::new().filter("status:eq:published")`. Filterable fields
    /// include `collectionId`, `status`, `locale`, `authorId`, `createdAt`,
    /// `updatedAt`, `sourceId` and `sourceStale`, and the filter syntax is
    /// `field:op:value`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn articles(&self, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page("/v1/kb/articles", query)
    }

    /// `GET /v1/kb/articles` — every page, as an iterator.
    pub fn iter_articles(&self, query: &Query) -> PageIter<Value> {
        self.client.paginate("/v1/kb/articles", query)
    }

    /// `GET /v1/kb/articles/{slug}` — one article by slug.
    ///
    /// Addressed by **slug**, not id, and percent-encoded here so a slug
    /// containing a `/` reaches this route rather than a different one.
    ///
    /// Two caveats, both the API's. This route returns an article whatever
    /// its status, **including drafts and archived ones** — do not render
    /// its output on a public page without checking `status` yourself. And
    /// a slug does not identify one article on its own: locale is part of
    /// an article's identity, and this route takes no `locale`, so with
    /// translations present you get whichever one the tiebreak picks. The
    /// anonymous, published-only, locale-aware variant is a different route
    /// — `GET /v1/public/kb/{workspaceId}/articles/{slug}` — which takes no
    /// API key and so has no method here.
    ///
    /// Takes no query parameters at all.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no article has that
    /// slug.
    pub fn article(&self, slug: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/kb/articles/{}", encode_path_segment(slug)),
            &Query::new(),
        )
    }

    /// `GET /v1/kb/collections` — the collections articles are grouped into.
    ///
    /// Not cursor-paginated.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn collections(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/kb/collections", query)
    }
}
