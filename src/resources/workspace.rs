//! Workspace configuration: settings, labels, inboxes, team rosters and
//! the agent-skill taxonomy.
//!
//! These are five separate API routes rather than one `workspace` resource —
//! `/v1/workspace/settings`, `/v1/labels`, `/v1/inboxes`,
//! `/v1/teams/{id}/members` and `/v1/agents/skills`. They are grouped under
//! one namespace here because that is how `packages/sdk-spec/surface.json`
//! groups them, and because "what is this workspace configured like" is one
//! question.

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
}
