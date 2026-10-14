//! Workspace configuration: settings, labels, inboxes, team rosters, the
//! agent-skill taxonomy, the key scope vocabulary and the credential census.
//!
//! These are eight separate API routes under five different prefixes rather
//! than one `workspace` resource — `/v1/workspace/settings`,
//! `/v1/workspace/secrets`, `/v1/labels`, `/v1/inboxes`,
//! `/v1/teams/{id}/members`, `/v1/agents/skills`, `/v1/api-keys/scopes` and
//! `/v1/workspaces/me`. They are grouped under one namespace here because
//! that is how `packages/sdk-spec/surface.json` groups them, and because
//! "what is this workspace configured like" is one question.
//!
//! Note the last one: `/v1/workspaces` **plural** is the collection of
//! workspaces a caller belongs to, and `/v1/workspace` singular is this
//! workspace. The two prefixes are different resources, not a typo.

use serde_json::Value;

use crate::client::ChatlyClient;
use crate::error::ChatlyError;
use crate::query::{encode_path_segment, Query};

/// The `workspace` namespace. Get one from [`ChatlyClient::workspace`].
///
/// Read-only. Every write on these surfaces needs a signed-in person.
#[derive(Debug, Clone, Copy)]
pub struct Workspace<'a> {
    client: &'a ChatlyClient,
}

impl<'a> Workspace<'a> {
    pub(crate) fn new(client: &'a ChatlyClient) -> Self {
        Self { client }
    }

    /// `GET /v1/workspace/settings`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn settings(&self) -> Result<Value, ChatlyError> {
        self.client.get("/v1/workspace/settings", &Query::new())
    }

    /// `GET /v1/labels` — the workspace's labels, as a **JSON array**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn labels(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/labels", query)
    }

    /// `GET /v1/inboxes` — as a **JSON array**.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn inboxes(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/inboxes", query)
    }

    /// `GET /v1/teams/{id}/members` — the roster of one team, as a **JSON
    /// array**.
    ///
    /// Deliberately not cursor-paginated by the API: a team roster is a
    /// subset of the workspace's seats and is read as one list, so there is
    /// no iterator for it here either.
    ///
    /// Takes a team id, and no route in this crate lists teams, so get the
    /// id from the dashboard or from a conversation's `teamId`.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::NotFound`] if no such team.
    pub fn team_members(&self, team_id: &str, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get(
            &format!("/v1/teams/{}/members", encode_path_segment(team_id)),
            query,
        )
    }

    /// `GET /v1/agents/skills` — the workspace's skill taxonomy, as
    /// `{ "skills": [...] }` — an object, not a bare array.
    ///
    /// The distinct set of skill labels currently assigned to any agent, not
    /// a separately managed vocabulary: a skill exists because somebody
    /// holds it, so removing it from the last agent removes it from this
    /// list.
    ///
    /// Which skills a *particular* agent holds is a different route,
    /// `GET /v1/agents/{userId}/skills`; reach it with
    /// [`ChatlyClient::request`].
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn agent_skills(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/agents/skills", query)
    }

    /// `GET /v1/api-keys/scopes` — every scope a key can hold, as a **bare
    /// JSON array** of `{ value, group }`.
    ///
    /// Static. `group` is the part of the name before the colon, so you can
    /// render a grouped picker without parsing the strings yourself.
    ///
    /// The enum is built from the same `API_KEY_SCOPES` constant the guard
    /// and the key service validate against, so the documented vocabulary
    /// cannot drift from the enforced one — which is the reason to read this
    /// rather than hard-code a list.
    ///
    /// This and `GET /v1/api-keys/whoami` ([`ChatlyClient::whoami`]) are the
    /// only two routes on the api-keys controller a key may call. Listing,
    /// creating and revoking keys with a key is a privilege-escalation
    /// shape and stays closed.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn key_scopes(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/api-keys/scopes", query)
    }

    /// `GET /v1/workspace/secrets` — the credential census: **existence and
    /// timestamps, never a value.**
    ///
    /// One entry per stored credential across nine tables. No entry carries
    /// the secret, a prefix of it, a hash of it, or its length. This is the
    /// route for "what credentials does this workspace hold, and when were
    /// they last rotated" — not for reading one.
    ///
    /// Three things to read before acting on the answer:
    ///
    /// - **`rotatedAtBasis` and `lastUsedBasis`** say whether a null
    ///   timestamp means *never* or *nothing records it*. Those are very
    ///   different facts about a credential and the null looks identical.
    /// - **`truncated`** on the envelope: each of the nine sources is
    ///   separately capped and there is no cursor, so a census can be
    ///   incomplete. Read it before concluding a credential does not exist.
    /// - `total` is `items.len()`, so the object has an `items` array and no
    ///   cursor — it is not a page.
    ///
    /// **This is the one route on this surface requiring TWO permissions**,
    /// `workspace:write` AND `integrations:read`, ANDed. A key holding only
    /// one gets [`ChatlyError::Permission`], which is easy to misread as the
    /// usual single-scope refusal.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`]; [`ChatlyError::Permission`] unless the key holds
    /// both scopes.
    pub fn secrets(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/workspace/secrets", query)
    }

    /// `GET /v1/workspaces/me` — the workspaces this credential can reach, as
    /// a **bare JSON array**.
    ///
    /// **For a `ck_` key the array holds exactly one entry**, the workspace
    /// the key was minted in; it is a per-user list on the human path and a
    /// key is not a user. So treat it as a one-element array rather than
    /// something to iterate for a tenant list.
    ///
    /// `role` on that entry is the **synthetic** `restricted` that the auth
    /// layer mints for every key, not a stored membership role. That is the
    /// honest value — what a key may do is its scope set, which
    /// [`ChatlyClient::me`] and [`ChatlyClient::whoami`] answer — and
    /// reporting `owner` because the key's creator was one would overstate
    /// the credential.
    ///
    /// Reachable on a **suspended** workspace, so a caller stranded in one
    /// can see what else there is to switch to. Everything else on
    /// `/v1/workspaces` stays closed to keys: creating a workspace makes the
    /// *caller* its owner, which has no meaning without a person.
    ///
    /// # Errors
    ///
    /// Any [`ChatlyError`].
    pub fn mine(&self, query: &Query) -> Result<Value, ChatlyError> {
        self.client.get("/v1/workspaces/me", query)
    }
}
