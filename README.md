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
iterate it. There are four answers over the 163 routes, and they are not
guessable from the method name:

| Shape                                                           | Count | Where                                                                                                                                                                                                                                                 |
| --------------------------------------------------------------- | ----: | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `{ items, nextCursor }` — a `Page`, with an `.iter()` beside it |    13 | `contacts.list`, `contacts.journey`, `conversations.list`, `conversations.messages`, `conversations.unattended`, `tickets.list`, `kb.articles`, `search.query`, `events.list`, `webhooks.deliveries`, `bots.list`, `bots.deliveries`, `triggers.list` |
| a bare JSON **array**                                           |    33 | every `@PaginationExempt` list — `macros.list`, `ai.tools`, `ai.tone`, `brands.list`, `channels.list`, `channels.adapters`, `docs_sources.list`, … and two writes: `ai.set_tone` (a PUT) and `docs_sources.discover` (a POST)                         |
| a single object                                                 |   116 | everything else, including every other write                                                                                                                                                                                                          |
| no body at all — a real `204`, decoded to `Value::Null`         |     1 | `macros.delete`, and only that one. The other seven DELETEs answer `200` with a body                                                                                                                                                                  |

Three traps in that table, each of which has cost somebody an afternoon:

- **An object that carries `items` is not a page.** `conversations.linked`,
  `conversations.outcomes`, `tickets.time`, `macros.templates`,
  `workspace.secrets` and `billing.usage_monthly` all have an `items` array
  and no cursor. `Page` refuses them, deliberately.
- **`billing.invoices` is cursor-paginated and is still an object**, because
  its envelope key is `data` rather than `items`. There is no `.iter()` for
  it; read `out["data"]` and pass the cursor back yourself.
- **A DELETE can answer with a body.** `contacts.remove_tag` returns the
  surviving contact and `conversations.cancel_scheduled` the row it
  cancelled.

Some paged routes add a `hasMore` sibling, `conversations.unattended` adds a
`total` that is **not** `items.len()`, and `search.query` adds `tier` and
`facets`. `Page` ignores every extra key, so you can read `next_cursor` and
not think about it — but call `search().query()` rather than `search().iter()`
if you want the facets, because the iterator yields hits alone.

**Only key-accessible routes have methods.** Of the API's ~880 routes, the
ones carrying `@AllowApiKey()` are the surface; the rest need a signed-in
person and answer a key with `403`. So there is no `reactions` method (a
reaction records _who_ reacted, and a key is not a person), no contact
block/unblock, and no `GET /v1/channels/{id}/secret` — read silently versus
rotate loudly is the line on that one. A method that always fails is worse
than no method, because the failure arrives in production inside somebody
else's integration.

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

163 methods, one per entry in the SDK manifest every Chatly SDK shares.
`surface.lock.json` in this package is generated by `tests/surface_lock.rs`
_executing_ each one and recording the request it issued, so the tables below
are checked rather than asserted.

Every method returns `Result<serde_json::Value, ChatlyError>` unless noted.
`q: &Query` is the query parameters; pass `&Query::new()` for none.

### Root

| Method     | HTTP | Path                  |
| ---------- | ---- | --------------------- |
| `whoami()` | GET  | `/v1/api-keys/whoami` |
| `me()`     | GET  | `/v1/me`              |

Both describe the calling credential and, for a `ck_` key, **return the same
object** — there is no user behind a key. The difference is the bar: `whoami`
needs `workspace:read` and `me` needs no permission at all, deliberately, so
that a key can learn its own scope list without provoking 403s to discover
it. Use `me()` as a liveness check; both answer on a suspended workspace.

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
| `journey(id, q)` → `Page`       | GET    | `/v1/contacts/{id}/journey`     |
| `mood(id, q)`                   | GET    | `/v1/contacts/{id}/mood`        |
| `cards(contact_id, body)`       | POST   | `/v1/contacts/{id}/cards`       |

`ContactInput` builds an upsert body (`external_id` is the upsert key;
without it you create a new contact every time). `bulk_upsert` takes
`BulkContactsInput`, 1–5,000 at a time. The tags route accepts **one tag per
request**, so `add_tags` issues one request per tag and is not atomic;
`add_tag` sends a single one.

`mood` takes `days` 7–365 (default 90) and answers `buckets: []` with a low
`sampleCount` when there is not enough signal — an empty state, not an error.
`cards` is a POST for a read: it pulls each configured card live from **your**
backend, so its latency is yours, and each entry's `state` is what tells an
empty card from a broken one (`blocks: null` is a real value).

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
| `linked(id, q)`                  | GET    | `/v1/conversations/{id}/linked`                       |
| `outcomes(id, q)`                | GET    | `/v1/conversations/{id}/outcomes`                     |
| `record_outcome(id, body)`       | POST   | `/v1/conversations/{id}/outcomes`                     |
| `return_to_bot(id, body)`        | POST   | `/v1/conversations/{id}/return-to-bot`                |
| `update_blocks(id, msg, body)`   | PATCH  | `/v1/conversations/{id}/messages/{id}/blocks`         |
| `unread_counts(q)`               | GET    | `/v1/conversations/unread-counts`                     |

`MessageInput::text(id, body)` and `::note(id, body)` build a send body; a
note is visible to agents and not to the contact. `cancel_scheduled` is keyed
on the scheduled-message id alone — there is no conversation id in that path.
`set_agent_handling` additionally requires a **machine-user** key; an ordinary
workspace key gets `Permission` from it.

`record_outcome`, `outcomes` and `update_blocks` each declare one permission
and then narrow by the caller's _conversation read scope_, which is
`conversations:read` and nothing else. A key without it gets **404 on a
conversation that plainly exists** rather than a 403 — if a thread you can
read through `get()` 404s on one of these three, check the scope list before
the id. `return_to_bot` needs `conversations:assign` and `botId` is required
(`return_to_bot_id(id, bot_id)` builds that body); `unread_counts` is per-USER
state and therefore always empty for a key — use `unattended` instead.

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
| `conversations(id, q)`              | GET  | `/v1/tickets/{id}/conversations`                 |
| `status_history(id, q)`             | GET  | `/v1/tickets/{id}/status-history`                |
| `lineage(id, q)`                    | GET  | `/v1/tickets/{id}/lineage`                       |
| `time(id, q)`                       | GET  | `/v1/tickets/{id}/time`                          |
| `open_count(conversation_id)`       | GET  | `/v1/tickets/open-count/{conversation_id}`       |

The merge body is `{"loserId": …}` here and `{"mergedId": …}` on contacts —
the API's naming, not a typo. `merge_loser(id, loser_id)` builds it.

`open_count` puts its literal segment **before** the id and takes a
_conversation_ id, not a ticket's. `status_history` caps at 500 transitions
and `truncated` is the only sign of it; `time` answers `{ items,
totalMinutes, billableMinutes }` with no cursor, so it is not a page.

### `macros()`

| Method                | HTTP   | Path                          |
| --------------------- | ------ | ----------------------------- |
| `list(q)` → **array** | GET    | `/v1/macros`                  |
| `create(body)`        | POST   | `/v1/macros`                  |
| `update(id, body)`    | PATCH  | `/v1/macros/{id}`             |
| `delete(id)`          | DELETE | `/v1/macros/{id}`             |
| `get(id)`             | GET    | `/v1/macros/{id}`             |
| `actions(q)`          | GET    | `/v1/macros/actions`          |
| `templates(q)`        | GET    | `/v1/macros/templates`        |
| `template(slug)`      | GET    | `/v1/macros/templates/{slug}` |

`list` is the one list route in this SDK that is **not** cursor-paginated.
The handler is pagination-exempt: it answers a bare JSON array, capped at
`MACRO_LIBRARY_CAP` (200) rows and ordered by usage rank. So there is no
`Page` and no `.iter()` here — narrow with `filter[]` if 200 is not enough.

`delete` is the **only real 204** on this whole surface. `actions` answers
`{ limit, actions }` — eleven action kinds, sixteen per macro, two different
numbers. `templates` answers `{ items, total }` over a frozen catalogue, and
_installing_ a template is not key-accessible: read it and build the macro
with `create`.

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

| Method                         | HTTP  | Path                                     |
| ------------------------------ | ----- | ---------------------------------------- |
| `deliveries(q)` → `Page`       | GET   | `/v1/webhooks/deliveries`                |
| `delivery(id)`                 | GET   | `/v1/webhooks/deliveries/{id}`           |
| `redeliver(id)`                | POST  | `/v1/webhooks/deliveries/{id}/redeliver` |
| `list(q)` → **array**          | GET   | `/v1/webhooks`                           |
| `events(q)`                    | GET   | `/v1/webhooks/events`                    |
| `create(body)`                 | POST  | `/v1/webhooks`                           |
| `schema_versions(q)`           | GET   | `/v1/webhooks/schema-versions`           |
| `set_schema_version(id, body)` | PATCH | `/v1/webhooks/{id}/schema-version`       |

`create` is the only response that carries the endpoint `secret` — there is
no reveal route and no rotation route, so losing it means deleting the
endpoint and creating another. Build your event list from `events()` rather
than hard-coding one: the dashboard's picker and the API's validation were
separately hard-coded once and the picker offered three events the platform
does not emit. After `set_schema_version`, read `missingEventTypes`: a 200
can still mean some of your events are not described by the version you just
pinned.

`delete` answers `{ "ok": true }` with a 200. It was closed to keys for a
while, which was the worst of the three options: a key could create a
subscription and never remove one, so a deploy pipeline registering an
endpoint per environment leaked one into the customer's workspace on every
run. Note the two resources on overlapping prefixes — an endpoint is
`/v1/webhooks/{id}` and a delivery is `/v1/webhooks/deliveries/{id}` — so a
delete aimed at the wrong one hits a path that exists.

_Verifying_ a delivery is `verify_webhook_signature`, which needs no client.

### `reports()`

| Method                       | HTTP | Path                                    |
| ---------------------------- | ---- | --------------------------------------- |
| `overview(q)`                | GET  | `/v1/reports/overview`                  |
| `agents(q)`                  | GET  | `/v1/reports/agents`                    |
| `conversations_by_day(q)`    | GET  | `/v1/reports/conversations-by-day`      |
| `sla(q)`                     | GET  | `/v1/reports/sla`                       |
| `sources(q)` → **array**     | GET  | `/v1/reports/sources`                   |
| `attribution(q)` → **array** | GET  | `/v1/reports/attribution`               |
| `cohorts(q)`                 | GET  | `/v1/reports/cohorts`                   |
| `response_times(q)`          | GET  | `/v1/reports/response-times`            |
| `teams(q)`                   | GET  | `/v1/reports/teams`                     |
| `labels(q)`                  | GET  | `/v1/reports/labels`                    |
| `voice(q)`                   | GET  | `/v1/reports/voice`                     |
| `themes(q)`                  | GET  | `/v1/reports/themes`                    |
| `theme_conversations(id, q)` | GET  | `/v1/reports/themes/{id}/conversations` |

The first three take `days` only (1–90; default 7, 7 and **14**) and clamp an
out-of-range value silently. `sla` takes `days`, `from`/`to` (together, or a
`400`) and `group_by`, and rejects rather than clamps. `sla_range(from, to,
group_by)` builds that query.

No two of these share a parameter vocabulary, and nothing complains about a
parameter a route ignores. `cohorts` takes `weeks` (2–26) and **echoes the
window it used**, which can differ from the one you asked for. `voice` allows
`days` up to **365** where everything else stops at 90, reports rates as
0–1, and its `recordingStorage.exact: false` means the byte count is part
extrapolation. `teams` and `labels` take `from`/`to` plus `business_hours`
(snake_case), and `businessHours.applied: false` means the durations are
calendar time — the numbers are never relabelled, so read the flag before
captioning them. `attribution` **refuses** an unknown `dimension`
(`ATTRIBUTION_DIMENSIONS`) rather than defaulting, and its `value: null` row
is the untagged bucket: drop it and every other `percentage` inflates.
`themes` wants `date` as `YYYY-MM-DD` and 400s on anything else, and
`theme_conversations` answers `200` with an empty list for an unknown theme
rather than a 404.

### `channels()`, `billing()`, `workspace()`

| Method                                             | HTTP  | Path                              |
| -------------------------------------------------- | ----- | --------------------------------- |
| `channels().list(q)` → **array**                   | GET   | `/v1/channels`                    |
| `channels().get(id)`                               | GET   | `/v1/channels/{id}`               |
| `channels().adapters(q)` → **array**               | GET   | `/v1/channels/adapters`           |
| `channels().create(body)`                          | POST  | `/v1/channels`                    |
| `channels().update(id, body)`                      | PATCH | `/v1/channels/{id}`               |
| `channels().rotate_secret(id)`                     | POST  | `/v1/channels/{id}/secret/rotate` |
| `billing().summary(q)`                             | GET   | `/v1/billing`                     |
| `billing().plans(q)` → **array**                   | GET   | `/v1/billing/plans`               |
| `billing().usage(event_type, q)`                   | GET   | `/v1/billing/usage`               |
| `billing().usage_monthly(q)`                       | GET   | `/v1/billing/usage/monthly`       |
| `billing().usage_daily(event_type, q)`             | GET   | `/v1/billing/usage/daily`         |
| `billing().invoices(q)`                            | GET   | `/v1/billing/invoices`            |
| `workspace().settings()`                           | GET   | `/v1/workspace/settings`          |
| `workspace().labels(q)` → **array**                | GET   | `/v1/labels`                      |
| `workspace().inboxes(q)` → **array**               | GET   | `/v1/inboxes`                     |
| `workspace().team_members(team_id, q)` → **array** | GET   | `/v1/teams/{id}/members`          |
| `workspace().agent_skills(q)`                      | GET   | `/v1/agents/skills`               |
| `workspace().key_scopes(q)` → **array**            | GET   | `/v1/api-keys/scopes`             |
| `workspace().secrets(q)`                           | GET   | `/v1/workspace/secrets`           |

`billing().usage` and `usage_daily` take the **event type as an argument, not
a query parameter**, because the handler validates it against a closed list
and throws first — a call without it is a `400`, never an all-types rollup.
`usage_monthly` is the opposite: it always reports all four types plus a
total, takes `month` as `YYYY-MM` only, and ignores an `eventType`.
`USAGE_EVENT_TYPES` lists what the server accepts today;
`usage_range(event_type, from, to)` sets the window, which otherwise defaults
to the current UTC month. `usage_daily` is **dense** — one entry per UTC day
in the window, zeros included — so a long `days` array is the window and not
the traffic. `summary` reports `seatsLimit: -1` for uncapped, which is every
plan today.

`channels().rotate_secret` works on a **custom (`api`) channel only** and
answers `409` for any other type; it is a hard cut with no grace window.
`channels().update` merges `config` by RFC 7396 unless you send
`configMode: "replace"`, and an empty patch is a `409`. There is still no
`channels().delete` and no way to read a channel secret back:
`GET /v1/channels/{id}/secret` is the one channel route deliberately closed
to keys.

`workspace().secrets` is the only route on this surface needing **two**
permissions, `workspace:write` AND `integrations:read`. It reports existence
and timestamps and never a value — read `rotatedAtBasis` and `lastUsedBasis`
before acting on a null (they say whether it means _never_ or _nothing
records it_), and `truncated` before concluding a credential does not exist.

`/v1/workspace` singular and `/v1/workspaces` plural are different resources.
`workspace().mine` is the plural one — the workspaces the caller can reach —
and **for a key that array holds exactly one entry**, with a synthetic
`restricted` role rather than a stored one, because what a key may do is its
scope set and reporting `owner` would overstate the credential.

### `ai()`

| Method                       | HTTP   | Path                               |
| ---------------------------- | ------ | ---------------------------------- |
| `providers(q)`               | GET    | `/v1/ai-providers`                 |
| `add_provider(body)`         | POST   | `/v1/ai-providers`                 |
| `test_provider(provider)`    | POST   | `/v1/ai-providers/{provider}/test` |
| `quota(q)`                   | GET    | `/v1/ai/quota`                     |
| `tone(q)` → **array**        | GET    | `/v1/ai/tone`                      |
| `set_tone(body)` → **array** | PUT    | `/v1/ai/tone`                      |
| `tone_preview(q)`            | GET    | `/v1/ai/tone/preview`              |
| `generate_tone(body)`        | POST   | `/v1/ai/tone/generate`             |
| `tools(q)` → **array**       | GET    | `/v1/ai/tools`                     |
| `create_tool(body)`          | POST   | `/v1/ai/tools`                     |
| `update_tool(id, body)`      | PATCH  | `/v1/ai/tools/{id}`                |
| `delete_tool(id)`            | DELETE | `/v1/ai/tools/{id}`                |

`/v1/ai-providers` is a **sibling** of `/v1/ai`, not a child of it:
`/v1/ai/providers` is unmapped. `add_provider` calls the vendor before
storing the key, so it is slow and only an authentication-class _answer_
refuses the save — read `validation.status` rather than trusting the 201.
No response here ever carries a key. `set_tone` is a PUT that replaces the
whole rule set and answers the same bare array the GET does. `update_tool`
keeps stored `secretHeaders` when you **omit** the field and clears them when
you send `{}`, so do not serialise your whole model on every save.

### `bots()`

| Method                                       | HTTP  | Path                                      |
| -------------------------------------------- | ----- | ----------------------------------------- |
| `list(q)` → `Page`                           | GET   | `/v1/bots`                                |
| `get(id)`                                    | GET   | `/v1/bots/{id}`                           |
| `create(body)`                               | POST  | `/v1/bots`                                |
| `update(id, body)`                           | PATCH | `/v1/bots/{id}`                           |
| `update_external(id, body)`                  | PATCH | `/v1/bots/{id}/external`                  |
| `rotate_external_secret(id)`                 | POST  | `/v1/bots/{id}/external/rotate-secret`    |
| `rotate_external_secret_at(id, activate_at)` | POST  | `/v1/bots/{id}/external/rotate-secret`    |
| `analytics(id, q)`                           | GET   | `/v1/bots/{id}/analytics`                 |
| `deliveries(id, q)` → `Page`                 | GET   | `/v1/bots/{id}/deliveries`                |
| `delivery(id, delivery_id)`                  | GET   | `/v1/bots/{id}/deliveries/{delivery_id}`  |
| `redeliver(id, delivery_id)`                 | POST  | `/v1/bots/{id}/deliveries/{id}/redeliver` |
| `test_delivery(id)`                          | POST  | `/v1/bots/{id}/test-delivery`             |

`create` takes a **union of two disjoint bodies and neither half is strict**,
so a field from the wrong one is silently dropped: `type: "external"` plus a
`systemPrompt` is a 201 and an agent with no prompt. The external arm returns
a `signingSecret` and a `cb_` token, each shown exactly once.
`rotate_external_secret(id)` is the hard cut: it sends no body and the old
secret stops signing the moment it returns, so deploy the new one first.
`rotate_external_secret_at(id, "2026-10-16T09:00:00Z")` opens an overlap
window instead — the old secret keeps signing every delivery until that
instant (at most 168h ahead of the server's clock), so hold both in your
verifier meanwhile. The response's `activatesAt` is the switch instant, or
`null` when the new secret is already signing. A list row omits the signed
payload (a page of fifty would be megabytes of transcript); `delivery` carries the exact bytes that were
signed, which is the route to use when your signature check fails.
`redeliver` resends the **same `eventId`**, so a receiver deduping on it will
discard a replay it already processed. `test_delivery` writes nothing and
carries `"test": true` with all-zero UUIDs — acknowledge it and do not reply.

### `triggers()`, `bot_routes()`, `canvas_steps()`

| Method                             | HTTP   | Path                            |
| ---------------------------------- | ------ | ------------------------------- |
| `triggers().list(q)` → `Page`      | GET    | `/v1/triggers`                  |
| `triggers().create(body)`          | POST   | `/v1/triggers`                  |
| `triggers().delete(id)`            | DELETE | `/v1/triggers/{id}`             |
| `bot_routes().list(q)` → **array** | GET    | `/v1/bot-routes`                |
| `bot_routes().fields(q)`           | GET    | `/v1/bot-routes/fields`         |
| `bot_routes().create(body)`        | POST   | `/v1/bot-routes`                |
| `bot_routes().update(id, body)`    | PATCH  | `/v1/bot-routes/{id}`           |
| `bot_routes().delete(id)`          | DELETE | `/v1/bot-routes/{id}`           |
| `bot_routes().resolve(body)`       | POST   | `/v1/bot-routes/resolve`        |
| `canvas_steps().get(id)`           | GET    | `/v1/canvas-steps/{id}`         |
| `canvas_steps().resolve(id, body)` | POST   | `/v1/canvas-steps/{id}/resolve` |

`triggers().create` is an **upsert** keyed on an optional `id` in the body —
there is no PATCH, so an update is a whole replacement — and it answers `201`
on both arms. `actions` must hold 1–20 entries; `eventType` is a free string
validated only for length, matched later by literal equality, so a typo is a
trigger that is syntactically perfect and never fires.

`bot_routes().list` is in **evaluation order** and that order is the data.
`resolve` dry-runs the real decision over a hypothetical visitor and returns
the `event` object the conditions saw, which is where half of every routing
bug is visible; disabled routes are absent from it by design.

`canvas_steps().resolve` 404s if you call it from inside the request that is
about to answer `{ pending: true }` — the row is written after we answer the
visitor — and 409s once the step is spent or ten minutes have passed.

### `visitors()`

| Method                                      | HTTP | Path                                          |
| ------------------------------------------- | ---- | --------------------------------------------- |
| `visitors().start_conversation(contact_id)` | POST | `/v1/visitors/{contactId}/start-conversation` |

One method, and the only input is the path parameter — no body, no query. It
answers `{ "conversationId": … }` and opens a thread with a contact who did
**not** write first.

**A `403` here is probably not your scopes.** The route is gated by a
per-workspace opt-in that is off by default,
`settings.proactiveOutreach.apiKeysMayStartConversations`. An owner or admin
turns it on with `PATCH /v1/workspace/settings`, which needs a signed-in
person and is therefore not in this crate; read the current value with
`workspace().settings()`, which is. `conversations:write` was already
grantable and is already held by keys minted for ticket bulk actions and
schedule-send, so the permission deliberately is not the opt-in.

**Three per contact per hour**, counted per workspace and per _contact_
rather than per caller, because the abuse shape is N threads at one person.

The two 429s this route can answer do not look the same, and only one of them
tells you how long to wait. The **per-contact limit** carries **no
`Retry-After` header** — the TTL is in the JSON body at
`error.details.retryAfterSeconds`, so `err.retry_after()` is `None` and the
value is readable only by parsing `ApiError::body` — this crate builds
`retry_after` from the header alone and has no body fallback, unlike the Ruby
and Python clients, which do populate it on this same refusal. Porting a
retry loop from their examples will silently drop the wait. The **global throttler
buckets** this route inherits (`short`, 30/second; `login`, 100/15 minutes) do
emit a `Retry-After`, so a 429 from those populates `retry_after`. Back off on
`None` here rather than retrying at once: it means "wait out the hour", not
"no delay required".

**An offline visitor is not notified at all.** The greeting is persisted and
the widget renders it from history on their next page view — no email, no
push, no provider message. Reaching someone who has left the site wants an
email or SMS conversation instead.

Unlike `channels().rotate_secret`, this route sits under a class-level
`@RequireIdempotencyKey()` with no opt-out, so the key this crate always
sends is _required_: a retried timeout replays the same thread rather than
opening a second one.

The two reads on the same controller — `/v1/visitors/live` and
`/v1/visitors/{contactId}/pages` — carry no `@AllowApiKey()` and answer a
`ck_` key with `403`, so there is deliberately no method for either.

### `brands()`, `customer_cards()`, `customer_card_endpoints()`

| Method                                          | HTTP   | Path                                               |
| ----------------------------------------------- | ------ | -------------------------------------------------- |
| `brands().list(q)` → **array**                  | GET    | `/v1/brands`                                       |
| `brands().create(body)`                         | POST   | `/v1/brands`                                       |
| `brands().update(id, body)`                     | PATCH  | `/v1/brands/{id}`                                  |
| `brands().delete(id)`                           | DELETE | `/v1/brands/{id}`                                  |
| `customer_cards().list(q)` → **array**          | GET    | `/v1/customer-cards`                               |
| `customer_cards().create(body)`                 | POST   | `/v1/customer-cards`                               |
| `customer_cards().update(id, body)`             | PATCH  | `/v1/customer-cards/{id}`                          |
| `customer_cards().rotate_secret(id)`            | POST   | `/v1/customer-cards/{id}/rotate-secret`            |
| `customer_cards().delete(id)`                   | DELETE | `/v1/customer-cards/{id}`                          |
| `customer_card_endpoints().list(q)` → **array** | GET    | `/v1/customer-card-endpoints`                      |
| `customer_card_endpoints().create(body)`        | POST   | `/v1/customer-card-endpoints`                      |
| `customer_card_endpoints().rotate_secret(id)`   | POST   | `/v1/customer-card-endpoints/{id}/rotate-secret`   |
| `customer_card_endpoints().activate_secret(id)` | POST   | `/v1/customer-card-endpoints/{id}/activate-secret` |
| `customer_card_endpoints().delete(id)`          | DELETE | `/v1/customer-card-endpoints/{id}`                 |

`brands().delete` is a `409` if the brand is the workspace default or if
channels still point at it — both are "fix this first", not failures of the
call. `isDefault: true` on a create or update **demotes** the current
default; `icons` is the one field that merges rather than replaces.

The two card resources spell rotation differently and behave differently.
A **card** rotates in one step with no grace window. An **endpoint** rotates
in two: `rotate_secret` mints the next secret with `active: false`, the
current one keeps signing, and `activate_secret` promotes it — so the safe
sequence is rotate, deploy, activate, and a second `activate_secret` is a
`409` on purpose. Creating an endpoint changes signing for that URL
immediately and drops the `x-chatly-card-id` header; deleting one puts both
back, which is a change at your verifier rather than a cleanup.

### `docs_sources()`, `knowledge_gaps()`, `email_templates()`

| Method                                      | HTTP   | Path                            |
| ------------------------------------------- | ------ | ------------------------------- |
| `docs_sources().providers(q)` → **array**   | GET    | `/v1/docs-sources/providers`    |
| `docs_sources().list(q)` → **array**        | GET    | `/v1/docs-sources`              |
| `docs_sources().discover(body)` → **array** | POST   | `/v1/docs-sources/discover`     |
| `docs_sources().create(body)`               | POST   | `/v1/docs-sources`              |
| `docs_sources().update(id, body)`           | PATCH  | `/v1/docs-sources/{id}`         |
| `docs_sources().sync(id, body)`             | POST   | `/v1/docs-sources/{id}/sync`    |
| `docs_sources().webhook(id)`                | POST   | `/v1/docs-sources/{id}/webhook` |
| `docs_sources().delete(id)`                 | DELETE | `/v1/docs-sources/{id}`         |
| `knowledge_gaps().list(q)` → **array**      | GET    | `/v1/knowledge-gaps`            |
| `knowledge_gaps().update(id, body)`         | PATCH  | `/v1/knowledge-gaps/{id}`       |
| `knowledge_gaps().draft(id)`                | POST   | `/v1/knowledge-gaps/{id}/draft` |
| `email_templates().list(q)` → **array**     | GET    | `/v1/email-templates`           |
| `email_templates().preview(body)`           | POST   | `/v1/email-templates/preview`   |
| `email_templates().update(kind, body)`      | PUT    | `/v1/email-templates/{kind}`    |
| `email_templates().delete(kind)`            | DELETE | `/v1/email-templates/{kind}`    |

`discover` is a POST answering a bare array; pass `organizationId` when you
have one, or an org-scoped token fails the whole connect flow and an
account-wide one returns every space in every organisation. `sync_now(id,
full)` builds the sync body — send `full: true` whenever the _reader_ changed
rather than the content, or the run finds nothing new and the knowledge base
stays as wrong as it was. `webhook` answers `subscribed: false` with a reason
far more often than not (a read-only token cannot manage webhooks), and that
is a success. `delete` keeps the articles and reports how many.

`knowledge_gaps().list` **drops** an unrecognised `status` instead of
refusing it, so a misspelled filter returns everything with a 200; the PATCH
is strict about the same value. `set_status(id, status)` builds that body.
`draft` makes a real LLM call, is metered against the workspace's AI spend,
and its 201 carries either a draft article id **or a reason there is none**.

For email templates the resource is the **kind**, not a row id —
`EMAIL_TEMPLATE_KINDS` has the five — and `delete` means "restore the
built-in", answering `{ restored: true | false }` with a 200 rather than the
template. `preview` lists the tokens you wrote that the kind does not supply,
which is the reason to call it: at send time they render as nothing and
nowhere complains.

### Helpers, and extras beyond the manifest

| Item                                                                                                                                                                                                                                                                                                 | What                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `identity_hash(secret, external_id)`                                                                                                                                                                                                                                                                 | widget identity digest                                            |
| `verify_webhook_signature(body, header, secret)`                                                                                                                                                                                                                                                     | 300 s window                                                      |
| `verify_webhook_signature_with(…, &VerifyOptions)`                                                                                                                                                                                                                                                   | custom window or clock                                            |
| `search().kb(q, params)`                                                                                                                                                                                                                                                                             | `/v1/search` narrowed to `kinds=kb`                               |
| `.iter(q)` on each of the 13 paginated routes                                                                                                                                                                                                                                                        | auto-paginating `Iterator`                                        |
| `iter_messages`, `iter_journey`, `iter_unattended`, `iter_articles`, `iter_deliveries`                                                                                                                                                                                                               | the same, for sub-resources                                       |
| `request`, `request_as`, `paginate`                                                                                                                                                                                                                                                                  | the escape hatch                                                  |
| `with_idempotency_key`                                                                                                                                                                                                                                                                               | pin a key across calls                                            |
| `contacts().add_tag`, `tickets().merge_loser`, `conversations().schedule`, `conversations().set_agent_handling_state`, `conversations().return_to_bot_id`, `reports().sla_range`, `billing().usage_range`, `docs_sources().sync_now`, `knowledge_gaps().set_status`, `bots().deliveries_with_status` | typed bodies and queries for routes whose shape is a single field |
| `AI_PROVIDERS`, `ATTRIBUTION_DIMENSIONS`, `CHANNEL_TYPES`, `DELIVERY_STATUSES`, `DOCS_PROVIDERS`, `EMAIL_TEMPLATE_KINDS`, `GAP_STATUSES`, `ROUTE_CONDITION_OPS`, `TRIGGER_CONDITION_OPS`                                                                                                             | the closed vocabularies, for pickers — not for validating input   |

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
