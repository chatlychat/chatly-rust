//! Conversations — threads of messages on a channel.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::pagination::{Page, PageIter};
use crate::query::{encode_path_segment, Query};

/// What kind of message to send, for [`MessageInput::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// Plain text. The default.
    Text,
    /// A formatted body.
    Rich,
    /// An internal note — visible to agents, not to the contact.
    Note,
    /// A system event rather than something a person said.
    SystemEvent,
    /// Attachments with no text body of their own.
    AttachmentOnly,
}

/// A message to send, for [`Conversations::send`].
///
/// The three fields the API's `sendMessageInputSchema` needs for an ordinary
/// reply. That schema has nine more — `attachments`, `metadata`, `blocks`,
/// `bodyHtml`, `canvas`, `clientRef`, `replyToMessageId` among them — which
/// this struct does not model. Reach for a `serde_json::json!` literal for
/// those; [`Conversations::send`] takes any `Serialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageInput {
    /// The conversation this belongs to.
    ///
    /// Also in the URL path, and the path is what counts: the API overwrites
    /// this field with the path id, so a mismatch is silently ignored rather
    /// than rejected. Sent anyway, because the Node SDK sends it and the two
    /// SDKs should put identical bytes on the wire.
    pub conversation_id: String,
    /// The message body. Required, up to 10,000 characters.
    pub body: String,
    /// Message kind. Omitted when `None`, which the API defaults to
    /// `text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<MessageKind>,
}

impl MessageInput {
    /// A plain-text message.
    pub fn text(conversation_id: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            body: body.into(),
            kind: Some(MessageKind::Text),
        }
    }

    /// An internal note — agents see it, the contact does not.
    pub fn note(conversation_id: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            body: body.into(),
            kind: Some(MessageKind::Note),
        }
    }

    /// Override the kind.
    pub fn kind(mut self, kind: MessageKind) -> Self {
        self.kind = Some(kind);
        self
    }
}

/// The `state` values [`Conversations::set_agent_handling`] accepts.
///
/// `taken_over` is deliberately absent: it is set by a database trigger when
/// a human takes a thread over, and is not settable through the API.
pub const AGENT_HANDLING_SETTABLE_STATES: [&str; 3] =
    ["in_progress", "handed_off", "handled"];

/// The `conversations` namespace. Get one from
/// [`ChatlyClient::conversations`].
#[derive(Debug, Clone, Copy)]
pub struct Conversations<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Conversations<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/conversations` — one page.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page("/v1/conversations", query)
    }

    /// `GET /v1/conversations` — every page, as an iterator.
    pub fn iter(&self, query: &Query) -> PageIter<Value> {
        self.client.paginate("/v1/conversations", query)
    }

    /// `GET /v1/conversations/{id}`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such
    /// conversation.
    pub fn get(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/conversations/{}", encode_path_segment(id)),
            &Query::new(),
        )
    }

    /// `GET /v1/conversations/{id}/messages` — one page of a thread.
    ///
    /// Read the thread before replying to it. Do not sort the result by
    /// `id`: ids in this product are a mix of UUID v4 and v7, so `id` order
    /// is not time order.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn messages(&self, id: &str, query: &Query) -> Result<Page<Value>, ChatlyError> {
        self.client.get_page(
            &format!("/v1/conversations/{}/messages", encode_path_segment(id)),
            query,
        )
    }

    /// `GET /v1/conversations/{id}/messages` — the whole thread, as an
    /// iterator.
    pub fn iter_messages(&self, id: &str, query: &Query) -> PageIter<Value> {
        self.client.paginate(
            &format!("/v1/conversations/{}/messages", encode_path_segment(id)),
            query,
        )
    }

    /// `POST /v1/conversations/{id}/messages` — reply to a thread.
    ///
    /// Takes a [`MessageInput`], or any `Serialize`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if the body fails
    /// the API's schema.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), chatly::ChatlyError> {
    /// # let chatly = chatly::ChatlyClient::new("ck_x")?;
    /// use chatly::MessageInput;
    /// chatly
    ///     .conversations()
    ///     .send("cnv_1", &MessageInput::text("cnv_1", "On it — give me five minutes."))?;
    /// # Ok(()) }
    /// ```
    pub fn send(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!("/v1/conversations/{}/messages", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// `GET /v1/conversations/unattended` — threads waiting on a human.
    ///
    /// Accepts `assignee`, `team`, `channel`, `olderThanMinutes` (capped at
    /// 10,080 — seven days), `cursor` and `limit` (1–100, default 25).
    ///
    /// Two things worth knowing. This route does **not** reject a parameter
    /// it cannot use: an unparseable `team` or `olderThanMinutes` is dropped
    /// and you get a wider result set rather than a 400, so check your
    /// spelling against that list rather than relying on an error. And
    /// although `surface.json` does not mark it paginated — which is why
    /// there is no `iter_unattended` here — it does take a `cursor`, so
    /// [`ChatlyClient::paginate`] will walk it if your deployment returns a
    /// `nextCursor`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn unattended(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/conversations/unattended", query)
    }

    /// `GET /v1/conversations/{id}/agent-handling` — who or what is
    /// currently handling this thread.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn agent_handling(&self, id: &str) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/conversations/{}/agent-handling", encode_path_segment(id)),
            &Query::new(),
        )
    }

    /// `PUT /v1/conversations/{id}/agent-handling` — hand a thread to, or
    /// take it from, an automated agent.
    ///
    /// **This route needs a machine-user API key.** It is not enough for the
    /// key to be valid and scoped: the API requires the key be bound to a
    /// machine user, and answers [`ChatlyError::Permission`] to an ordinary
    /// workspace key and to a signed-in person alike. If you are building a
    /// bring-your-own-agent integration, this is the route that tells you
    /// your key is the wrong kind.
    ///
    /// The body is `{ "state": …, "reason"?: … }`. `state` must be one of
    /// [`AGENT_HANDLING_SETTABLE_STATES`]; `taken_over` is **not** settable
    /// — the database sets it when a human takes a thread over. `reason` is
    /// optional, nullable, up to 2,000 characters.
    /// [`Conversations::set_agent_handling_state`] builds the body for you.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Permission`] if the key is not a
    /// machine user's.
    pub fn set_agent_handling(
        &self,
        id: &str,
        body: &impl Serialize,
    ) -> Result<Value, ChatlyError> {
        self.client.write(
            "PUT",
            &format!("/v1/conversations/{}/agent-handling", encode_path_segment(id)),
            &Query::new(),
            Some(body),
        )
    }

    /// [`Conversations::set_agent_handling`] with the body built for you.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`], plus [`ChatlyError::Config`] if `state` is not
    /// one of [`AGENT_HANDLING_SETTABLE_STATES`] — refused here rather than
    /// sent, because a non-member enum literal makes Postgres fail the query
    /// with `22P02` instead of matching nothing, and the resulting 500 says
    /// nothing about the typo.
    pub fn set_agent_handling_state(
        &self,
        id: &str,
        state: &str,
        reason: Option<&str>,
    ) -> Result<Value, ChatlyError> {
        if !AGENT_HANDLING_SETTABLE_STATES.contains(&state) {
            return Err(ChatlyError::Config(format!(
                "agent handling state {state:?} is not settable; expected one of {AGENT_HANDLING_SETTABLE_STATES:?}"
            )));
        }
        let mut body = serde_json::Map::new();
        body.insert("state".to_owned(), Value::String(state.to_owned()));
        if let Some(reason) = reason {
            body.insert("reason".to_owned(), Value::String(reason.to_owned()));
        }
        self.set_agent_handling(id, &Value::Object(body))
    }

    /// `GET /v1/conversations/{id}/generated-replies` — replies an AI agent
    /// has drafted for this thread.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn generated_replies(&self, id: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!(
                "/v1/conversations/{}/generated-replies",
                encode_path_segment(id)
            ),
            query,
        )
    }

    /// `POST /v1/conversations/{id}/generated-replies` — propose a draft
    /// reply.
    ///
    /// The body is `{ "body": …, "basedOnMessageId"?: …,
    /// "proposedByLabel"?: … }`. `body` is required, 1–10,000 characters
    /// after trimming. `proposedByLabel` is honoured **only** on the
    /// API-key path — a signed-in person's request has it discarded — so it
    /// is how a bring-your-own-agent integration names itself in the UI.
    ///
    /// Note that *sending* or *discarding* a generated reply is not
    /// key-accessible: `POST …/generated-replies/{replyId}/send` and
    /// `/discard` both need a signed-in person, so there is no method for
    /// them here.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn generate_reply(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!(
                "/v1/conversations/{}/generated-replies",
                encode_path_segment(id)
            ),
            &Query::new(),
            Some(body),
        )
    }

    /// `GET /v1/conversations/{id}/scheduled-messages` — messages queued to
    /// send later on this thread.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn scheduled_messages(&self, id: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!(
                "/v1/conversations/{}/scheduled-messages",
                encode_path_segment(id)
            ),
            query,
        )
    }

    /// `POST /v1/conversations/{id}/scheduled-messages` — queue a message
    /// for later.
    ///
    /// The body is `{ "body": …, "sendAt": … }` and both are required.
    /// `sendAt` is an ISO-8601 **instant** — a moment in time, not a delay
    /// and not a bare date. [`Conversations::schedule`] builds it for you.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] if `sendAt` is not
    /// an instant.
    pub fn schedule_message(&self, id: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            &format!(
                "/v1/conversations/{}/scheduled-messages",
                encode_path_segment(id)
            ),
            &Query::new(),
            Some(body),
        )
    }

    /// [`Conversations::schedule_message`] with the body built for you.
    ///
    /// `send_at` must be an ISO-8601 instant, e.g. `2026-10-13T09:00:00Z`.
    /// It is passed through as given: this crate has no date type and will
    /// not reformat it, so an invalid value comes back as
    /// [`ChatlyError::Validation`] from the API rather than being silently
    /// corrected.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn schedule(
        &self,
        id: &str,
        body: &str,
        send_at: &str,
    ) -> Result<Value, ChatlyError> {
        self.schedule_message(
            id,
            &serde_json::json!({ "body": body, "sendAt": send_at }),
        )
    }

    /// `DELETE /v1/conversations/scheduled-messages/{scheduled_id}` — cancel
    /// a queued message.
    ///
    /// Note the path: it is keyed on the scheduled-message id alone and does
    /// **not** include the conversation id, which is why this method takes
    /// only one argument.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if it already sent or
    /// was already cancelled.
    pub fn cancel_scheduled(&self, scheduled_id: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!(
                "/v1/conversations/scheduled-messages/{}",
                encode_path_segment(scheduled_id)
            ),
            &Query::new(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn message_input_serialises_to_camel_case_with_a_snake_case_kind() {
        let json = serde_json::to_value(MessageInput::text("cnv_1", "hi")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "conversationId": "cnv_1", "body": "hi", "kind": "text" })
        );
        let json = serde_json::to_value(
            MessageInput::text("cnv_1", "hi").kind(MessageKind::SystemEvent),
        )
        .unwrap();
        assert_eq!(json["kind"], "system_event");
    }

    #[test]
    fn a_note_is_marked_as_one() {
        let json = serde_json::to_value(MessageInput::note("cnv_1", "internal")).unwrap();
        assert_eq!(json["kind"], "note");
    }

    #[test]
    fn attachment_only_is_a_kind() {
        let json = serde_json::to_value(
            MessageInput::text("cnv_1", "").kind(MessageKind::AttachmentOnly),
        )
        .unwrap();
        assert_eq!(json["kind"], "attachment_only");
    }

    #[test]
    fn taken_over_is_not_a_settable_handling_state() {
        assert!(!AGENT_HANDLING_SETTABLE_STATES.contains(&"taken_over"));
        assert_eq!(AGENT_HANDLING_SETTABLE_STATES.len(), 3);
    }

    #[test]
    fn an_unset_kind_is_omitted_rather_than_sent_as_null() {
        let input = MessageInput {
            conversation_id: "cnv_1".to_owned(),
            body: "hi".to_owned(),
            kind: None,
        };
        let json = serde_json::to_value(&input).unwrap();
        assert!(json.get("kind").is_none(), "{json}");
    }
}
