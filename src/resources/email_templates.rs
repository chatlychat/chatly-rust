//! Email templates — the operator's control over the words in Chatly's
//! transactional mail.
//!
//! **The resource is the kind, not a row id.** There is one override per
//! workspace per kind, you never see an id, and
//! `PUT /v1/email-templates/workspace_invite` is idempotent by construction —
//! which is what a Save button wants. `DELETE` on the same path means
//! "restore the built-in".
//!
//! `workspace:write` is the bar because the blast radius is real: this copy
//! goes out under the workspace's brand to people who are *not* in the
//! workspace — a new teammate, the billing owner, a hired agent.

use serde::Serialize;
use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `kind` values these routes accept.
///
/// Five, and the list is closed: a value outside it is a **400 naming the
/// bad value**, not a 404. That boundary check exists because the column is
/// a varchar — an unknown literal reaching the query would be a Postgres
/// `22P02`, which 500s rather than matching nothing, and this product has hit
/// that three separate times.
pub const EMAIL_TEMPLATE_KINDS: [&str; 5] = [
    "workspace_invite",
    "trial_ending_reminder",
    "conversation_cap_warning",
    "hired_agent_engagement_invite",
    "hired_agent_engagement_started",
];

/// The `email_templates` namespace. Get one from
/// [`ChatlyClient::email_templates`].
#[derive(Debug, Clone, Copy)]
pub struct EmailTemplates<'a> {
    client: &'a ChatlyClient,
}

impl<'a> EmailTemplates<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/email-templates` — all five kinds, as a **bare JSON array**.
    ///
    /// Every kind is present whether or not this workspace has overridden it;
    /// a row says which copy is in force. So an empty array would mean the
    /// catalogue is broken, not that nothing is customised.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn list(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/email-templates", query)
    }

    /// `POST /v1/email-templates/preview` — render a draft without saving
    /// it.
    ///
    /// Body: `{ "kind", "subject", "body" }`. A POST because the copy *is*
    /// the request — far too long for a query string, and somebody's draft
    /// has no business in an access log.
    ///
    /// The response carries the rendered subject, HTML and text, plus **any
    /// tokens you wrote that this kind does not supply** — which is the
    /// reason to call it. A `{{contact.name}}` in an invite email renders as
    /// nothing at send time and nowhere complains; here it is listed.
    ///
    /// Writes nothing, and answers **201** anyway (Nest's POST default).
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an unknown kind.
    pub fn preview(&self, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "POST",
            "/v1/email-templates/preview",
            &Query::new(),
            Some(body),
        )
    }

    /// `PUT /v1/email-templates/{kind}` — replace this workspace's copy for
    /// one email.
    ///
    /// A PUT, so it is the whole override and re-sending the same body is a
    /// no-op change. `kind` is one of [`EMAIL_TEMPLATE_KINDS`] and is
    /// percent-encoded here, so an unknown value reaches the handler and
    /// comes back as a 400 that names it rather than as a routing 404.
    ///
    /// Answers the kind as stored — the same shape as one element of
    /// [`EmailTemplates::list`].
    ///
    /// The audit row records a machine user if your key is linked to one, and
    /// nothing otherwise. That is not a gap: the actor column is an FK into
    /// `users` and an API key has no row there, which 500s if you write the
    /// key's own id into it.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an unknown kind
    /// or copy that fails the schema.
    pub fn update(&self, kind: &str, body: &impl Serialize) -> Result<Value, ChatlyError> {
        self.client.write(
            "PUT",
            &format!("/v1/email-templates/{}", encode_path_segment(kind)),
            &Query::new(),
            Some(body),
        )
    }

    /// `DELETE /v1/email-templates/{kind}` — restore the built-in copy.
    ///
    /// It **deletes the override** rather than copying today's default into
    /// it, so the workspace follows the built-in as it reads in future
    /// versions, not as it reads now.
    ///
    /// **It has a body, and it is not the restored template.** `{ restored:
    /// true | false }` with a 200: `false` means the workspace was already on
    /// the built-in. The call is idempotent and that flag is the only way to
    /// tell the two outcomes apart.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Validation`] for an unknown kind.
    pub fn delete(&self, kind: &str) -> Result<Value, ChatlyError> {
        self.client.delete(
            &format!("/v1/email-templates/{}", encode_path_segment(kind)),
            &Query::new(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_kinds_are_the_catalogues_five() {
        assert_eq!(EMAIL_TEMPLATE_KINDS.len(), 5);
        assert!(EMAIL_TEMPLATE_KINDS.contains(&"workspace_invite"));
        // snake_case, not kebab or dotted.
        assert!(EMAIL_TEMPLATE_KINDS
            .iter()
            .all(|k| !k.contains('-') && !k.contains('.')));
    }
}
