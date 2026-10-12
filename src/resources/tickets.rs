//! Tickets — the help-desk side of a conversation.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::{encode_path_segment, Query};

/// The `action` values [`Tickets::bulk`] accepts.
pub const TICKET_BULK_ACTIONS: [&str; 4] = ["status", "assign", "priority", "queue"];

/// The `tickets` namespace. Get one from [`ChatlyClient::tickets`].
#[derive(Debug, Clone, Copy)]
pub struct Tickets<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Tickets<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/tickets` — one page.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page("/v1/tickets", query)
    }

    /// `GET /v1/tickets` — every page, as an iterator.
    pub fn iter(&self, query: &Query) -> PageIter<Value> {
        self.client.paginate("/v1/tickets", query)
    }

    /// `GET /v1/tickets/{id}/comments` — the comment thread on a ticket, as
    /// a **JSON array**.
    ///
    /// Pagination-exempt: a ticket discussion is bounded and the service
    /// caps it at 500 comments, so there is no envelope and no cursor.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn comments(&self, id: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/tickets/{}/comments", encode_path_segment(id)),
            query,
        )
    }

    /// `GET /v1/tickets/for-conversation/{conversation_id}` — the tickets
    /// attached to a conversation, as a **JSON array**.
    ///
    /// An array and not one ticket: a conversation can carry more than one,
    /// though in practice a handful at most.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn for_conversation(&self, conversation_id: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!(
                "/v1/tickets/for-conversation/{}",
                encode_path_segment(conversation_id)
            ),
            &Query::new(),
        )
    }

    /// `GET /v1/tickets/queues` — the ticket queues configured in this
    /// workspace, as a **JSON array**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn queues(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/tickets/queues", query)
    }

    /// `POST /v1/tickets/{id}/merge` — fold another ticket into this one.
    ///
    /// The body is `{ "loserId": "<uuid>" }`: `id` is the ticket that
    /// survives, `loser_id` the one absorbed. Use
    /// [`Tickets::merge_loser`] to have that built for you.
    ///
    /// Note that the field is `loserId` here and `mergedId` on
    /// [`crate::Contacts::merge`]. The two routes do the same kind of thing
    /// and spell the argument differently; that is the API's naming, not a
    /// typo here.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn merge(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!("/v1/tickets/{}/merge", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `POST /v1/tickets/{id}/split` — carve a new ticket out of this one.
    ///
    /// `title` (1–280 chars) is the only required field. `description`,
    /// `priority` (`low`/`normal`/`high`/`urgent`), `type`
    /// (`question`/`incident`/`problem`/`task`), `assigneeUserId`, `tags`
    /// (up to 20, each ≤40 chars) and `conversationIds` (up to 100, each
    /// already linked to the origin ticket) are optional — and *omitting*
    /// one is not the same as sending `null`: an omitted field inherits
    /// from the origin ticket, where `null` clears it and `[]` empties it.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if `title` is
    /// missing.
    pub fn split(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!("/v1/tickets/{}/split", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `POST /v1/tickets/bulk` — apply one change to up to 200 tickets.
    ///
    /// The body is `{ "ids": [...], "action": …, "payload": {...} }`.
    /// `action` is one of [`TICKET_BULK_ACTIONS`], and `payload` carries
    /// the field that action needs — `status`, `assigneeUserId`,
    /// `priority` or `queueId`. The schema does not enforce which payload
    /// goes with which action; the server does, so a mismatch is a 4xx and
    /// not a silent no-op.
    ///
    /// `assigneeUserId: null` unassigns and `queueId: null` removes from the
    /// queue, so for those two an explicit `null` is meaningful and
    /// omitting the key is not.
    ///
    /// Unlike every other write in this crate, this route does not
    /// *require* an `Idempotency-Key`. One is sent anyway — CONTRACT.md §2
    /// says every write carries one, and a header the server ignores costs
    /// nothing next to a retry that double-applies a bulk action.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an empty `ids`,
    /// more than 200 of them, or a payload the action cannot use.
    pub fn bulk(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client
            .write("POST", "/v1/tickets/bulk", &Query::new(), Some(body))
    }

    /// [`Tickets::merge`] with the `{ "loserId": … }` body built for you.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn merge_loser(&self, id: &str, loser_id: &str) -> Result<Value, ChatlyError> {
        self.merge(id, &serde_json::json!({ "loserId": loser_id }))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_bulk_actions_are_the_four_the_api_accepts() {
        assert_eq!(TICKET_BULK_ACTIONS.len(), 4);
        for action in ["status", "assign", "priority", "queue"] {
            assert!(TICKET_BULK_ACTIONS.contains(&action), "{action}");
        }
        // Not `assignee`, not `set_status`.
        assert!(!TICKET_BULK_ACTIONS.contains(&"assignee"));
    }
}
