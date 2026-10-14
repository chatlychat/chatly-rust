//! Knowledge gaps — questions the knowledge base does not answer.
//!
//! The AI records a gap when it cannot ground an answer. This namespace is
//! the list, the triage and the draft. `kb:read` / `kb:write` rather than an
//! AI permission: a gap is a hole in the help centre and the draft it
//! produces is a KB article, so it belongs to whoever owns that.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `status` values a gap can have.
pub const GAP_STATUSES: [&str; 4] = ["open", "drafted", "resolved", "dismissed"];

/// The `knowledge_gaps` namespace. Get one from
/// [`ChatlyClient::knowledge_gaps`].
#[derive(Debug, Clone, Copy)]
pub struct KnowledgeGaps<'a> {
    client: &'a ChatlyClient,
}

impl<'a> KnowledgeGaps<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/knowledge-gaps` — the gaps, as a **bare JSON array**,
    /// most-asked first.
    ///
    /// Pagination-exempt because the recorder caps a workspace at 200 open
    /// gaps; `limit` is clamped to 1–200 and defaults to 100.
    ///
    /// **`status` is clamped, not validated.** The handler `safeParse`s it
    /// and *drops* an unrecognised value, so `?status=oepn` returns every
    /// status with a 200 rather than a 400. If a filter looks like it did
    /// nothing, check the spelling against [`GAP_STATUSES`] — nothing else
    /// will tell you. (The same string on
    /// [`KnowledgeGaps::update`] is strict and does 400.)
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/knowledge-gaps", query)
    }

    /// `PATCH /v1/knowledge-gaps/{id}` — triage: resolved, dismissed, or
    /// back to open.
    ///
    /// Body: `{ "status": … }` and nothing else — the schema is `.strict()`,
    /// so an extra key is a 400. One of [`GAP_STATUSES`], and here the enum
    /// really is enforced.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if the id is not this
    /// workspace's, [`ChatlyError::Validation`] for an unknown status.
    pub fn update(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PATCH",
            &format!("/v1/knowledge-gaps/{}", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// [`KnowledgeGaps::update`] with the one-field body built for you.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn set_status(&self, id: &str, status: &str) -> Result<Value, ChatlyError> {
        self.update(id, &serde_json::json!({ "status": status }))
    }

    /// `POST /v1/knowledge-gaps/{id}/draft` — write a **draft** KB article
    /// from the conversation the gap was found in.
    ///
    /// Takes no body. Answers **201** with the gap plus either the new draft
    /// article's id **or a reason there is none** — so a 201 is not a promise
    /// that an article exists, and the field is the thing to read.
    ///
    /// A gap with no conversation to draft from is a **400**, not that
    /// shape: there is nothing for the model to read.
    ///
    /// The article is created as a draft and is not published. It makes a
    /// real LLM call, so it is slower than the rest of this namespace and
    /// it is metered against the workspace's AI spend
    /// ([`crate::Ai::quota`]).
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for a gap with no
    /// source conversation.
    pub fn draft(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.write::<Value>(
            "POST",
            &format!("/v1/knowledge-gaps/{}/draft", encode_path_segment(id)),
            &Query::new(),
            None,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_statuses_include_the_one_the_draft_route_sets() {
        assert_eq!(GAP_STATUSES.len(), 4);
        assert!(GAP_STATUSES.contains(&"drafted"));
        // `closed` is the plausible wrong guess.
        assert!(!GAP_STATUSES.contains(&"closed"));
    }
}
