//! Full-text search.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::Query;

/// The kinds `/v1/search` can search over, as the API's enum spells them.
///
/// Useful mainly so you do not have to guess the spelling: the enum is
/// `['contact','conversation','message','ticket','kb']`, singular, and `kb`
/// rather than `article`.
pub const SEARCH_KINDS: [&str; 5] = ["contact", "conversation", "message", "ticket", "kb"];

/// The `search` namespace. Get one from [`ChatlyClient::search`].
#[derive(Debug, Clone, Copy)]
pub struct Search<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Search<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/search` — search contacts, conversations, messages, tickets
    /// and KB articles.
    ///
    /// `q` is required (1–256 chars) and is set onto `query` for you,
    /// replacing any `q` already there. Everything else in `query` is passed
    /// through untouched; the route also accepts `from`, `to`, `channelId`,
    /// `assigneeUserId`, `status`, `limit` (1–100, default 25) and `cursor`.
    ///
    /// # Narrowing by kind
    ///
    /// The parameter is **`kinds`**, a repeated plain key — not `kinds[]`:
    ///
    /// ```no_run
    /// # fn main() -> Result<(), chatly::ChatlyError> {
    /// # let chatly = chatly::ChatlyClient::new("ck_x")?;
    /// use chatly::Query;
    /// let hits = chatly.search().query(
    ///     "refund policy",
    ///     &Query::new().push("kinds", "kb").push("kinds", "ticket"),
    /// )?;
    /// # let _ = hits; Ok(()) }
    /// ```
    ///
    /// Use [`Query::push`] and not [`Query::set`], so two kinds stay two
    /// parameters. [`SEARCH_KINDS`] has the valid spellings.
    ///
    /// The bracketed `kinds[]` is a trap worth naming, because three
    /// first-party clients fell into it: the search query schema is a zod
    /// object, so `kinds[]` was an unknown key and got **stripped**. The
    /// search then ran unnarrowed and returned 200 — a filter that silently
    /// did nothing, which is worse than one that errors. The schema folds
    /// the bracketed form now, but plain repeated keys are the form to send.
    /// `filter[]` genuinely is bracketed; it has its own fold.
    ///
    /// Note also that this crate does **not** mark `/v1/search` as
    /// paginated, matching `packages/sdk-spec/surface.json` — even though
    /// the route takes a `cursor`. Use [`ChatlyClient::paginate`] if your
    /// deployment returns a `nextCursor` here.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if `q` is empty or
    /// over 256 characters.
    pub fn query(&self, q: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/search", &query.clone().set("q", q))
    }

    /// `GET /v1/search?kinds=kb` — the same route, narrowed to help-centre
    /// articles.
    ///
    /// Worth its own method because "find the article that answers this" is
    /// the common case, and getting there otherwise means knowing the
    /// `kinds` parameter exists and that it is not `kinds[]`. This is what
    /// the MCP server's `search_kb` tool does.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn kb(&self, q: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client
            .get("/v1/search", &query.clone().set("q", q).push("kinds", "kb"))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_singular_and_kb_is_not_article() {
        assert!(SEARCH_KINDS.contains(&"kb"));
        assert!(!SEARCH_KINDS.contains(&"article"));
        assert!(!SEARCH_KINDS.contains(&"contacts"));
        assert_eq!(SEARCH_KINDS.len(), 5);
    }
}
