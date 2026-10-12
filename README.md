# chatly

The official Rust SDK for [Chatly](https://chatlychat.com) — live chat, a
shared inbox, a help centre, and AI agents that answer on their own.

This crate talks to Chatly's REST API from **your server**. If you want the
chat bubble on your website, that is a different thing (`@chatlychat/sdk-js`);
this is for reading conversations, replying to them, syncing contacts,
recording events, and pulling reports from your own backend.

```sh
cargo add chatly
```

Blocking, three dependencies (`ureq`, `serde`, `serde_json`), Rust 1.85+.

---

## Quickstart

```rust
use chatly::{ChatlyClient, ContactInput, MessageInput, Query};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let chatly = ChatlyClient::new(std::env::var("CHATLY_API_KEY")?)?;
    println!("{}", chatly.whoami()?);                               // check the key
    chatly.contacts().upsert(                                       // sync a customer
        &ContactInput::new().external_id("u_42").email("ada@example.com"))?;
    for row in chatly.conversations().iter(&Query::new().limit(50)) {
        let id = row?["id"].as_str().unwrap_or_default().to_owned();
        chatly.conversations().send(&id, &MessageInput::text(&id, "On it."))?;
    }
    Ok(())
}
```

### Getting a key

In the dashboard: **Settings → API keys → Create key**. You get a string
starting `ck_`. Pick the narrowest scopes that do the job — `whoami` tells
you which ones a key actually has, and scope errors arrive as
`ChatlyError::Permission`, separately from "this key is wrong"
(`ChatlyError::Authentication`).

### The key is not safe in a browser

A `ck_` key is a bearer credential for your **entire workspace**: every
contact, every conversation, every report. Keep it server-side and in a
secret store. Anyone who reads it can read your customers.

---

## What you get

- **Auth, headers and idempotency** handled. Every write carries a fresh
  `Idempotency-Key`, so a retry inside this crate can never double-write.
- **Retries** on connection failures, `429` and `5xx` only — three attempts,
  exponential backoff with jitter, `Retry-After` honoured, the same
  idempotency key on each attempt. A `4xx` that is not `429` is never
  retried.
- **Typed errors** you can `match` on, carrying `status`, `code`, `message`,
  `request_id` and the raw body.
- **Auto-pagination** as a plain `Iterator`.
- **Webhook signature verification** and **widget identity hashing**, as free
  functions needing no client.
- **An escape hatch** to any route this crate has no method for.

### What you don't get, on purpose

**Response bodies are `serde_json::Value`, not structs.** This crate is built
from a method manifest that pins verbs and paths and says nothing about
response shapes. A hand-written `struct Contact` would be a guess that
compiles, drifts silently, and makes the tests agree with it. Write your own
types and use `request_as::<T>()` or `paginate::<T>()`, both generic over any
`Deserialize`:

```rust
#[derive(serde::Deserialize)]
struct Contact { id: String, email: Option<String> }

for row in chatly.paginate::<Contact>("/v1/contacts", &chatly::Query::new()) {
    println!("{}", row?.id);
}
```

**It is blocking.** No async variant and no feature flag for one: a blocking
client needs no runtime. From async code, wrap a call in
`tokio::task::spawn_blocking`.

### Which shape comes back

Since everything is a `Value`, you need to know whether to index it or
iterate it. There are three answers, and they are not guessable from the
method name:

| Shape                                                        | Routes                                                                                                                                                                                                                                                                                                                          |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `{ items, nextCursor }` — a `Page`, with `.iter()` available | the 7 paginated routes: `contacts.list`, `conversations.list`, `conversations.messages`, `tickets.list`, `kb.articles`, `events.list`, `webhooks.deliveries`                                                                                                                                                                    |
| a bare JSON **array**                                        | `macros.list`, `channels.list`, `billing.plans`, `kb.collections`, `tickets.comments`, `tickets.for_conversation`, `tickets.queues`, `conversations.generated_replies`, `conversations.scheduled_messages`, `reports.agents`, `reports.conversations_by_day`, `workspace.labels`, `workspace.inboxes`, `workspace.team_members` |
| a single object                                              | everything else, including every write                                                                                                                                                                                                                                                                                          |

Three routes answer a page envelope but have no `.iter()` here, because the
shared manifest does not list them as paginated: `contacts.journey`,
`conversations.unattended` (which adds a `total` that is **not**
`items.len()`) and `search.query` (which adds `tier` and `facets`). Walk
them with `paginate()` if you want to.

Some paged routes add a `hasMore` sibling. `Page` ignores it, and ignores any
other extra key, so you can read `next_cursor` and not think about it.

**Only key-accessible routes have methods.** Of the API's ~880 routes, about
140 accept a `ck_` key; the rest need a signed-in person and answer a key
with `403`. So there is no `reactions` method (a reaction records _who_
reacted, and a key is not a person) and no channel configuration. A method
that always fails is worse than no method, because the failure arrives in
production inside somebody else's integration.

---

## Errors

```rust
use chatly::ChatlyError;

match chatly.contacts().get("c_404") {
    Ok(contact) => println!("{contact}"),

    Err(ChatlyError::Authentication(e)) => eprintln!("key is wrong: {}", e.message),
    Err(ChatlyError::Permission(e))     => eprintln!("key lacks a scope: {:?}", e.code),
    Err(ChatlyError::NotFound(e))       => eprintln!("no such contact ({})", e.status),
    Err(ChatlyError::Conflict(e))       => eprintln!("conflict: {}", e.message),
    Err(ChatlyError::Validation(e))     => eprintln!("bad body: {}", e.body),  // 400

    Err(ChatlyError::RateLimited { error, retry_after }) => {
        eprintln!("slow down for {:?}: {}", retry_after, error.message);
    }
    Err(ChatlyError::Server(e)) => eprintln!("our fault, request_id={:?}", e.request_id),

    Err(ChatlyError::Transport { message, .. }) => eprintln!("never reached us: {message}"),
    Err(ChatlyError::Decode { body, .. })       => eprintln!("unexpected body: {body}"),
    Err(ChatlyError::Config(message))           => eprintln!("my mistake: {message}"),

    // `ChatlyError` is `#[non_exhaustive]`: a future status may get its own
    // variant, and anything without one arrives as `ChatlyError::Api`.
    Err(other) => eprintln!("{other}"),
}
```

**Validation failures are `400`, not `422`.** That is what this API sends —
`code: "validation_failed"`. `ChatlyError::Validation` covers both statuses
so you are right either way, but do not write code that waits for a `422`.
It also means a `400` that is _not_ a schema failure lands in `Validation`;
check `e.code` if you need to tell them apart.

If you only want one question answered, there are predicates and accessors
instead — `err.is_not_found()`, `err.is_rate_limited()`, `err.status()`,
`err.code()`, `err.request_id()`, `err.body()`, `err.retry_after()`,
`err.is_retryable()`.

Quote `request_id` when you contact support: it is the `x-request-id`
response header, and it is how we find your request in our logs. The API
mints one per request and sends the header on every response, errors
included, so it is normally there; it is `None` for a failure that never
got a response at all.

### Retries

Defaults are three attempts with 250 ms of jittered, doubling backoff.

```rust
use chatly::{ChatlyClient, RetryPolicy};
use std::time::Duration;

let chatly = ChatlyClient::builder(key)
    .retry(RetryPolicy { max_attempts: 5, ..RetryPolicy::default() })
    .timeout(Duration::from_secs(30))
    .build()?;

let once = ChatlyClient::builder(key).retry(RetryPolicy::none()).build()?;
```

### Retrying the same operation yourself

To make _your_ retry loop safe, pin an idempotency key. Same key + same body
replays the first response; same key + a different body is a `409`.

```rust
let pinned = chatly.with_idempotency_key(format!("welcome-{order_id}"))?;
pinned.contacts().upsert(&body)?;   // safe to run again after a crash
```

`with_idempotency_key` returns a derived client and leaves the original
alone.

---

## Pagination

Two ways. One page, as the API returned it:

```rust
let page = chatly.contacts().list(&Query::new().limit(100))?;
for contact in &page.items { /* ... */ }
if let Some(cursor) = page.next_cursor { /* fetch the next one yourself */ }
```

Or every page, as an iterator that fetches as it goes:

```rust
for row in chatly.contacts().iter(&Query::new().limit(100)) {
    println!("{}", row?["id"]);
}
```

It stops on a null cursor, and stops rather than spinning if the server ever
repeats a cursor. One error ends the iteration.

**Do not sort or page on `id`.** Ids here are a mix of UUID v4 and v7, so
`id` order is not creation order. The cursor is the only correct sequence.

### Query parameters

`Query` keeps order and keeps repeats, which matters for `filter[]`:

```rust
let q = Query::new()
    .limit(50)
    .filter("status:eq:open")       // appends
    .filter("channel:eq:web")       // appends again — two parameters
    .sort("-createdAt")             // replaces
    .fields("id,email");
```

---

## Webhooks

Chatly signs each delivery and sends the signature in
`x-livechat-signature`:

```text
x-livechat-signature: t=1700000000,v1=c89214b5…d632925
```

`v1` is HMAC-SHA256 over `` `<t>.<raw body>` `` — the timestamp is _inside_
the digest, which is what makes the 300-second replay window enforceable.

**Verify against the raw bytes you received.** If you let your framework
parse the JSON and then re-serialise it, key order and whitespace change and
the digest will not match. Take the body as bytes.

### axum

```rust
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};

async fn chatly_webhook(headers: HeaderMap, body: Bytes) -> StatusCode {
    // `Bytes`, NOT `Json<Event>` — an extractor that parses the body has
    // already destroyed the thing you need to verify. In axum the body
    // extractor must also come last in the argument list.
    let signature = headers
        .get(chatly::WEBHOOK_SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if !chatly::verify_webhook_signature(&body, signature, &std::env::var("CHATLY_WEBHOOK_SECRET").unwrap_or_default()) {
        return StatusCode::UNAUTHORIZED;
    }

    let event: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(event) => event,
        Err(_) => return StatusCode::BAD_REQUEST,
    };
    println!("verified {}", event["type"]);
    StatusCode::OK   // answer fast; do the work on a queue
}
```

### actix-web

```rust
use actix_web::{web, HttpRequest, HttpResponse};

async fn chatly_webhook(req: HttpRequest, body: web::Bytes) -> HttpResponse {
    let signature = req
        .headers()
        .get(chatly::WEBHOOK_SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if !chatly::verify_webhook_signature(&body, signature, &secret()) {
        return HttpResponse::Unauthorized().finish();
    }
    HttpResponse::Ok().finish()
}
```

`verify_webhook_signature` returns `false` and never panics for a malformed
header, a missing field, a non-numeric timestamp, a stale timestamp or a bad
digest — a verifier that panicked would be an _accepted_ webhook in any
handler without a `catch_unwind`, which is all of them. The comparison is
constant-time.

To re-verify an archived delivery, whose timestamp is by definition outside
the live window:

```rust
use chatly::{verify_webhook_signature_with, VerifyOptions};

verify_webhook_signature_with(&body, signature, &secret,
    &VerifyOptions::new().without_tolerance());
```

Only do that for stored deliveries. On a live endpoint it accepts a replay of
anything ever captured from the wire.

---

## Widget identity verification

If you turn identity verification on, the browser widget must prove that the
`externalId` it claims came from your backend and was not typed into a
console. Otherwise anyone can open a chat as anyone else and read their
history.

Your server computes the digest; only the digest reaches the page.

```rust
let secret = std::env::var("CHATLY_IDV_SECRET").expect("CHATLY_IDV_SECRET is set");
let user_hash = chatly::identity_hash(&secret, &user.id)?;
// render into the page:
//   Chatly('identify', { externalId: user.id, userHash, email: user.email })
```

The secret is your workspace's identity-verification secret — not the API
key, not the webhook secret. It must never reach a browser.

---

## The escape hatch

About 85 key-accessible routes have no method here. Reach them without
giving up auth, idempotency, retries or typed errors:

```rust
use chatly::Query;

let out = chatly.request(
    "POST",
    "/v1/some/route",
    &Query::new().push("a", "1"),
    Some(&serde_json::json!({ "hello": "world" })),
)?;

let typed: MyType = chatly.request_as("GET", "/v1/some/route", &Query::new(), None)?;

for row in chatly.paginate::<serde_json::Value>("/v1/some/list", &Query::new()) { /* ... */ }
```

`POST`, `PUT`, `PATCH` and `DELETE` are treated as writes and get an
`Idempotency-Key` automatically. A leading `/` on the path is optional.

### Your own HTTP layer

`Transport` is a one-method trait. Implement it to add instrumentation, use
your own pool, or test without a network:

```rust
use chatly::{ChatlyError, HttpRequest, HttpResponse, Transport};

struct Fake;
impl Transport for Fake {
    fn execute(&self, _req: &HttpRequest) -> Result<HttpResponse, ChatlyError> {
        Ok(HttpResponse::json(200, r#"{"items":[],"nextCursor":null}"#))
    }
}

let chatly = chatly::ChatlyClient::builder("ck_test").transport(Fake).build()?;
```

Report a failure to reach the server as `ChatlyError::transport(msg)` and
never as a synthesised status code — otherwise the retry policy reads a DNS
failure as a genuine `503` on every attempt.

---

## Methods

55 methods, one per entry in the SDK manifest every Chatly SDK shares.
`surface.lock.json` in this package is generated by `tests/surface_lock.rs`
_executing_ each one and recording the request it issued, so the table below
is checked rather than asserted.

Every method returns `Result<serde_json::Value, ChatlyError>` unless noted.
`q: &Query` is the query parameters; pass `&Query::new()` for none.

### Root

| Method     | HTTP | Path                  |
| ---------- | ---- | --------------------- |
| `whoami()` | GET  | `/v1/api-keys/whoami` |

### `contacts()`

| Method                          | HTTP   | Path                            |
| ------------------------------- | ------ | ------------------------------- |
| `list(q)` → `Page`              | GET    | `/v1/contacts`                  |
| `get(id)`                       | GET    | `/v1/contacts/{id}`             |
| `upsert(body)`                  | POST   | `/v1/contacts`                  |
| `bulk_upsert(body)`             | POST   | `/v1/contacts/bulk`             |
| `merge(id, merged_id)`          | POST   | `/v1/contacts/{id}/merge`       |
| `add_tags(id, tags)`            | POST   | `/v1/contacts/{id}/tags`        |
| `remove_tag(id, tag)`           | DELETE | `/v1/contacts/{id}/tags/{tag}`  |
| `unsubscribe(id, flag, reason)` | POST   | `/v1/contacts/{id}/unsubscribe` |
| `journey(id, q)`                | GET    | `/v1/contacts/{id}/journey`     |

`ContactInput` builds an upsert body (`external_id` is the upsert key;
without it you create a new contact every time). `bulk_upsert` takes
`BulkContactsInput`, 1–5,000 at a time. The tags route accepts **one tag per
request**, so `add_tags` issues one request per tag and is not atomic;
`add_tag` sends a single one.

### `conversations()`

| Method                           | HTTP   | Path                                                  |
| -------------------------------- | ------ | ----------------------------------------------------- |
| `list(q)` → `Page`               | GET    | `/v1/conversations`                                   |
| `get(id)`                        | GET    | `/v1/conversations/{id}`                              |
| `messages(id, q)` → `Page`       | GET    | `/v1/conversations/{id}/messages`                     |
| `send(id, body)`                 | POST   | `/v1/conversations/{id}/messages`                     |
| `unattended(q)`                  | GET    | `/v1/conversations/unattended`                        |
| `agent_handling(id)`             | GET    | `/v1/conversations/{id}/agent-handling`               |
| `set_agent_handling(id, body)`   | PUT    | `/v1/conversations/{id}/agent-handling`               |
| `generated_replies(id, q)`       | GET    | `/v1/conversations/{id}/generated-replies`            |
| `generate_reply(id, body)`       | POST   | `/v1/conversations/{id}/generated-replies`            |
| `scheduled_messages(id, q)`      | GET    | `/v1/conversations/{id}/scheduled-messages`           |
| `schedule_message(id, body)`     | POST   | `/v1/conversations/{id}/scheduled-messages`           |
| `cancel_scheduled(scheduled_id)` | DELETE | `/v1/conversations/scheduled-messages/{scheduled_id}` |

`MessageInput::text(id, body)` and `::note(id, body)` build a send body; a
note is visible to agents and not to the contact. `cancel_scheduled` is keyed
on the scheduled-message id alone — there is no conversation id in that path.
`set_agent_handling` additionally requires a **machine-user** key; an ordinary
workspace key gets `Permission` from it.

### `tickets()`

| Method                              | HTTP | Path                                             |
| ----------------------------------- | ---- | ------------------------------------------------ |
| `list(q)` → `Page`                  | GET  | `/v1/tickets`                                    |
| `comments(id, q)`                   | GET  | `/v1/tickets/{id}/comments`                      |
| `for_conversation(conversation_id)` | GET  | `/v1/tickets/for-conversation/{conversation_id}` |
| `queues(q)`                         | GET  | `/v1/tickets/queues`                             |
| `merge(id, body)`                   | POST | `/v1/tickets/{id}/merge`                         |
| `split(id, body)`                   | POST | `/v1/tickets/{id}/split`                         |
| `bulk(body)`                        | POST | `/v1/tickets/bulk`                               |

The merge body is `{"loserId": …}` here and `{"mergedId": …}` on contacts —
the API's naming, not a typo. `merge_loser(id, loser_id)` builds it.

### `macros()`

| Method                | HTTP   | Path              |
| --------------------- | ------ | ----------------- |
| `list(q)` → **array** | GET    | `/v1/macros`      |
| `create(body)`        | POST   | `/v1/macros`      |
| `update(id, body)`    | PATCH  | `/v1/macros/{id}` |
| `delete(id)`          | DELETE | `/v1/macros/{id}` |

`list` is the one list route in this SDK that is **not** cursor-paginated.
The handler is pagination-exempt: it answers a bare JSON array, capped at
`MACRO_LIBRARY_CAP` (200) rows and ordered by usage rank. So there is no
`Page` and no `.iter()` here — narrow with `filter[]` if 200 is not enough.

### `kb()`

| Method                 | HTTP | Path                     |
| ---------------------- | ---- | ------------------------ |
| `articles(q)` → `Page` | GET  | `/v1/kb/articles`        |
| `article(slug)`        | GET  | `/v1/kb/articles/{slug}` |
| `collections(q)`       | GET  | `/v1/kb/collections`     |

`article` takes a **slug**, not an id, and returns drafts and archived
articles too — check `status` before rendering one publicly.

### `search()`

| Method             | HTTP | Path         |
| ------------------ | ---- | ------------ |
| `query(q, params)` | GET  | `/v1/search` |

Narrow by kind with `params.push("kinds", …)`, repeated once per kind.
`SEARCH_KINDS` has the valid spellings. `search().kb(q, params)` is the same
route pre-narrowed to help-centre articles.

The parameter is `kinds` and **not** `kinds[]`. The bracketed spelling was an
unknown key to the API's schema and got stripped, so the search ran
unnarrowed and still returned `200` — a filter that silently did nothing.
`filter[]` genuinely is bracketed; `kinds` is not.

### `events()`

| Method             | HTTP | Path             |
| ------------------ | ---- | ---------------- |
| `track(body)`      | POST | `/v1/cdp/events` |
| `list(q)` → `Page` | GET  | `/v1/cdp/events` |

`EventInput::new(contact_id, name).properties(…)` builds the body. `track`
needs the `contacts:write:events` scope, which is narrower than
`contacts:write`.

### `webhooks()`

| Method                   | HTTP | Path                                     |
| ------------------------ | ---- | ---------------------------------------- |
| `deliveries(q)` → `Page` | GET  | `/v1/webhooks/deliveries`                |
| `delivery(id)`           | GET  | `/v1/webhooks/deliveries/{id}`           |
| `redeliver(id)`          | POST | `/v1/webhooks/deliveries/{id}/redeliver` |

This namespace inspects deliveries. _Verifying_ one is
`verify_webhook_signature`, which needs no client.

### `reports()`

| Method                    | HTTP | Path                               |
| ------------------------- | ---- | ---------------------------------- |
| `overview(q)`             | GET  | `/v1/reports/overview`             |
| `agents(q)`               | GET  | `/v1/reports/agents`               |
| `conversations_by_day(q)` | GET  | `/v1/reports/conversations-by-day` |
| `sla(q)`                  | GET  | `/v1/reports/sla`                  |

The first three take `days` only (1–90; default 7, 7 and **14**) and clamp an
out-of-range value silently. `sla` takes `days`, `from`/`to` (together, or a
`400`) and `group_by`, and rejects rather than clamps. `sla_range(from, to,
group_by)` builds that query.

### `channels()`, `billing()`, `workspace()`

| Method                                 | HTTP | Path                     |
| -------------------------------------- | ---- | ------------------------ |
| `channels().list(q)`                   | GET  | `/v1/channels`           |
| `channels().get(id)`                   | GET  | `/v1/channels/{id}`      |
| `billing().plans(q)`                   | GET  | `/v1/billing/plans`      |
| `billing().usage(event_type, q)`       | GET  | `/v1/billing/usage`      |
| `workspace().settings()`               | GET  | `/v1/workspace/settings` |
| `workspace().labels(q)`                | GET  | `/v1/labels`             |
| `workspace().inboxes(q)`               | GET  | `/v1/inboxes`            |
| `workspace().team_members(team_id, q)` | GET  | `/v1/teams/{id}/members` |
| `workspace().agent_skills(q)`          | GET  | `/v1/agents/skills`      |

All read-only: every write on these surfaces needs a signed-in person.

`billing().usage` takes the **event type as an argument, not a query
parameter**, because the handler validates it against a closed list and
throws first — a call without it is a `400`, never an all-types rollup.
`USAGE_EVENT_TYPES` lists what the server accepts today
(`conversation.opened`, `message.sent.agent`, `message.sent.bot`,
`ai.tokens`); `usage_range(event_type, from, to)` sets the window, which
otherwise defaults to the current UTC month.

`channels().list`, `workspace().labels`, `workspace().inboxes` and
`workspace().team_members` all return **bare JSON arrays**. So does
`billing().plans` — and an empty one is a success there, not an error: it is
what a deployment with no Stripe configuration returns.

### Helpers, and extras beyond the manifest

| Item                                                                                                                                         | What                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| `identity_hash(secret, external_id)`                                                                                                         | widget identity digest                                |
| `verify_webhook_signature(body, header, secret)`                                                                                             | 300 s window                                          |
| `verify_webhook_signature_with(…, &VerifyOptions)`                                                                                           | custom window or clock                                |
| `search().kb(q, params)`                                                                                                                     | `/v1/search` narrowed to `kinds=kb`                   |
| `.iter(q)` on each of the 7 paginated routes                                                                                                 | auto-paginating `Iterator`                            |
| `iter_messages`, `iter_articles`, `iter_deliveries`                                                                                          | the same, for sub-resources                           |
| `request`, `request_as`, `paginate`                                                                                                          | the escape hatch                                      |
| `with_idempotency_key`                                                                                                                       | pin a key across calls                                |
| `contacts().add_tag`, `tickets().merge_loser`, `conversations().schedule`, `conversations().set_agent_handling_state`, `reports().sla_range` | typed bodies for routes whose shape is a single field |

---

## Pointing at another environment

```rust
let chatly = ChatlyClient::builder(key)
    .base_url("http://127.0.0.1:3001")
    .build()?;
```

Another Chatly environment, or a test server in your own suite — those are
the reasons to set `base_url`. A trailing slash is trimmed for you. The
default is `https://api.chatlychat.com` and you should leave it alone
otherwise.

## Tests

`cargo test` runs everything offline — no network, no API key, no running
API. The suite injects a fake `Transport` for request-shaping assertions and
uses a `std::net::TcpListener` on loopback for the ones that need real HTTP.

## Licence

MIT. See `LICENSE`.

---

## About this repository

This is a **mirror**. `chatly-rust` is built from a private monorepo where the rest
of the platform, its migrations and its contract checks live, so a pull request here
cannot be merged directly — but a patch is still the fastest way to get something
fixed and we will port it with attribution.

What you can do here: read the source, run the tests, open an issue, or open a PR
and we will treat the diff as the proposal.

Issues: https://github.com/chatlychat/chatly-rust/issues · Docs: https://chatlychat.com/docs
