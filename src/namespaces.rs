//! The namespace accessors on [`ChatlyClient`] — one method per `ns` in
//! `packages/sdk-spec/surface.json`.
//!
//! A second `impl ChatlyClient` block, split out of `client.rs` for the
//! contract's 650-line ceiling (§9), and a real seam: `client.rs` is about
//! building a request, retrying it and decoding the answer, and nothing here
//! does any of those. Every one of these is the same two lines — hand back a
//! borrowed view that holds a reference and nothing else — so the file is a
//! directory of the surface rather than logic.
//!
//! The one-line summaries are deliberately route lists rather than prose:
//! this file is what someone reads in rustdoc's sidebar when they are
//! looking for where a feature lives.

use crate::client::ChatlyClient;
use crate::resources;

impl ChatlyClient {
    /// AI configuration: BYOK provider keys, the spend meter, tone of voice,
    /// custom tools.
    pub fn ai(&self) -> resources::Ai<'_> {
        resources::Ai::new(self)
    }

    /// Billing: summary, plan catalogue, invoices, metered usage.
    pub fn billing(&self) -> resources::Billing<'_> {
        resources::Billing::new(self)
    }

    /// Bot routing rules: the ordered table, the field catalogue, and the
    /// dry-run.
    pub fn bot_routes(&self) -> resources::BotRoutes<'_> {
        resources::BotRoutes::new(self)
    }

    /// Bots: Chatly personas and external agents, plus the delivery log,
    /// redelivery and test deliveries.
    pub fn bots(&self) -> resources::Bots<'_> {
        resources::Bots::new(self)
    }

    /// Brands: multi-brand sub-tenants inside one workspace.
    pub fn brands(&self) -> resources::Brands<'_> {
        resources::Brands::new(self)
    }

    /// Canvas steps: read a pending interactive step, or resolve it with the
    /// next canvas.
    pub fn canvas_steps(&self) -> resources::CanvasSteps<'_> {
        resources::CanvasSteps::new(self)
    }

    /// Channels: list, get, create, update, the adapter registry, and
    /// secret rotation for a custom channel.
    pub fn channels(&self) -> resources::Channels<'_> {
        resources::Channels::new(self)
    }

    /// Contacts: list, get, upsert, bulk upsert, merge, tags, unsubscribe,
    /// journey, mood, customer cards.
    pub fn contacts(&self) -> resources::Contacts<'_> {
        resources::Contacts::new(self)
    }

    /// Conversations: list, get, messages, send, unattended, agent
    /// handling, generated replies, scheduled messages, outcomes, blocks,
    /// return-to-bot.
    pub fn conversations(&self) -> resources::Conversations<'_> {
        resources::Conversations::new(self)
    }

    /// Customer cards: the sidebar panels Chatly renders from your backend.
    pub fn customer_cards(&self) -> resources::CustomerCards<'_> {
        resources::CustomerCards::new(self)
    }

    /// Customer-card endpoints: one signing secret per URL, with a
    /// two-phase rotation.
    pub fn customer_card_endpoints(&self) -> resources::CustomerCardEndpoints<'_> {
        resources::CustomerCardEndpoints::new(self)
    }

    /// Documentation sources: connect a docs site and index it into the
    /// knowledge base.
    pub fn docs_sources(&self) -> resources::DocsSources<'_> {
        resources::DocsSources::new(self)
    }

    /// Email templates: the workspace's copy for Chatly's transactional
    /// mail, addressed by kind.
    pub fn email_templates(&self) -> resources::EmailTemplates<'_> {
        resources::EmailTemplates::new(self)
    }

    /// CDP events: track and read back.
    pub fn events(&self) -> resources::Events<'_> {
        resources::Events::new(self)
    }

    /// Knowledge base: articles, one article by slug, collections.
    pub fn kb(&self) -> resources::Kb<'_> {
        resources::Kb::new(self)
    }

    /// Knowledge gaps: questions the knowledge base does not answer, their
    /// triage, and the draft article.
    pub fn knowledge_gaps(&self) -> resources::KnowledgeGaps<'_> {
        resources::KnowledgeGaps::new(self)
    }

    /// Macros: the library, create, update, delete, the action vocabulary,
    /// and the starter templates.
    pub fn macros(&self) -> resources::Macros<'_> {
        resources::Macros::new(self)
    }

    /// Reports: twelve aggregates, each with its own parameter vocabulary.
    pub fn reports(&self) -> resources::Reports<'_> {
        resources::Reports::new(self)
    }

    /// Full-text search across contacts, conversations, messages, tickets
    /// and KB articles.
    pub fn search(&self) -> resources::Search<'_> {
        resources::Search::new(self)
    }

    /// Tickets: list, comments, queues, merge, split, bulk, lineage, status
    /// history, time, and the conversations one covers.
    pub fn tickets(&self) -> resources::Tickets<'_> {
        resources::Tickets::new(self)
    }

    /// Triggers: the event-condition-action rules, as a page, an upsert and
    /// a delete.
    pub fn triggers(&self) -> resources::Triggers<'_> {
        resources::Triggers::new(self)
    }

    /// Visitors: open a conversation with a contact who has not written.
    ///
    /// One method only. The two reads on the same controller
    /// (`/v1/visitors/live` and `/v1/visitors/{contactId}/pages`) are not
    /// key-accessible, so they are deliberately absent.
    pub fn visitors(&self) -> resources::Visitors<'_> {
        resources::Visitors::new(self)
    }

    /// Webhooks: the subscriptions, the event and schema-version
    /// catalogues, and the delivery log. Note the two resources share a
    /// prefix — an endpoint is `/v1/webhooks/{id}`, a delivery is
    /// `/v1/webhooks/deliveries/{id}`.
    pub fn webhooks(&self) -> resources::Webhooks<'_> {
        resources::Webhooks::new(self)
    }

    /// Workspace configuration: settings, labels, inboxes, team rosters,
    /// the agent-skill taxonomy, the key scope vocabulary, the credential
    /// census and the workspaces this credential can reach.
    pub fn workspace(&self) -> resources::Workspace<'_> {
        resources::Workspace::new(self)
    }
}
