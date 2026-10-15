# chatly — changelog

All notable changes to the Rust SDK. This project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### `bots().rotate_external_secret_at` — rotation with an overlap window

`POST /v1/bots/{id}/external/rotate-secret` takes an optional
`{"activateAt": "<ISO 8601 instant>"}` now, and
`rotate_external_secret_at(id, activate_at)` sends it: the old secret keeps
signing every delivery until that instant, so a verifier holding both never
sees a failed signature. The string goes through verbatim; the server owns
the 168-hour bound and treats a past instant as a hard cut.

`rotate_external_secret(id)` is unchanged, on the wire as well as in source:
it still sends no body at all (not `{}`) and is still the hard cut. A sibling
method rather than an `Option` parameter because Rust has no default
arguments, and an added `Option` would have broken every existing caller.
Both answer `{ signingSecret, activatesAt }`; `activatesAt: null` means the
new secret is already signing. `tests/bots_rotate.rs` asserts the recorded
request for both forms.

### The whole manifest: 107 more methods, 55 → 162

`packages/sdk-spec/surface.json` lists 162 key-accessible routes and this
crate implemented 55 of them. It implements all 162 now, across eleven new
namespaces — `ai`, `bots`, `bot_routes`, `brands`, `canvas_steps`,
`customer_cards`, `customer_card_endpoints`, `docs_sources`,
`email_templates`, `knowledge_gaps`, `triggers` — plus new methods on every
namespace that already existed.

The manifest was 159 entries when this started and 162 by the time it
finished — `root.me`, `webhooks.delete` and `workspace.mine` landed mid-task.
All three are implemented here too, which is why the count is 107 and not 104.

Every response shape was derived from the handler's `@ZodResponse` schema
before it was entered in `tests/common/shapes.rs`, not copied from the
manifest's `shape` field. All 107 agreed with the manifest, which is the
first time that column has had four independent derivations behind it
(Python, Ruby, PHP and this one, same day).

### Three more routes were pages that nothing had flagged

`bots.list`, `bots.deliveries` and `triggers.list` all end in
`buildPaginatedResponse` and all three were unflagged in the manifest, the
same way `contacts.journey`, `conversations.unattended` and `search.query`
were before them. The paged floor in `tests/surface_lock.rs` went 10 → 13 and
`tests/surface_shapes.rs` has a walker for each — including the negative
test that a bare array fed to one must fail loudly, which is the only part
that proves the declaration matters.

The declared distribution is now 33 `array` / 13 `page` / 115 `object` /
1 `empty`, asserted exactly.

### Added

- Eleven namespaces and 104 methods. The traps worth naming, because they
  are the ones a reasonable author gets wrong:
  - `/v1/ai-providers` is a **sibling** of `/v1/ai`, not a child —
    `/v1/ai/providers` is unmapped.
  - `channels().rotate_secret` is `{id}/secret/rotate` while
    `customer_cards().rotate_secret` is `{id}/rotate-secret`. One operation,
    two spellings, in one SDK.
  - `tickets().open_count` puts its literal segment **first** and takes a
    _conversation_ id.
  - `ai().set_tone` is a PUT that answers a bare array;
    `docs_sources().discover` is a POST that answers one.
  - `billing().invoices` is cursor-paginated and is still `object`, because
    its envelope key is `data`. No pager is offered for it.
- `conversations().record_outcome`, `outcomes` and `update_blocks` need
  `conversations:read` **in addition** to the permission they advertise. The
  read scope resolves to a closed-door sentinel without it, so a key short of
  that scope gets a 404 on a conversation that plainly exists rather than a 403. Documented on each method.
- `conversations().unread_counts` is implemented and documented as
  near-useless from a key: it reads `membership.userId`, which for a `ck_`
  key is the key's own id, so it answers 200 with zeros. It is here because
  it does not fail, and an absent method reads as an oversight.
- Convenience bodies for the single-field routes:
  `return_to_bot_id`, `set_status`, `sync_now`, `deliveries_with_status`.
- The closed vocabularies, as constants, for pickers and not for validating
  input: `AI_PROVIDERS`, `ATTRIBUTION_DIMENSIONS`, `CHANNEL_TYPES`,
  `CUSTOMER_CARD_CAP`, `DELIVERY_STATUSES`, `DOCS_PROVIDERS`,
  `EMAIL_TEMPLATE_KINDS`, `GAP_STATUSES`, `ROUTE_CONDITION_OPS`,
  `TRIGGER_CONDITION_OPS`.

### `GET /v1/me` is the liveness check, not `whoami`

Both describe the calling credential and both answer a `ck_` key the same
object. `GET /v1/api-keys/whoami` requires `workspace:read`; `GET /v1/me`
requires **no permission at all**, deliberately, because everything it tells
a key is the workspace id and grant list the key presented in order to ask.
`ChatlyClient::me()` is therefore the call that works for a key with no
useful scope — otherwise an integrator discovers their scopes by provoking
403s.

`webhooks().delete` closes an asymmetry worth naming: a key could create a
subscription and had no way to remove one, so a deploy pipeline registering
an endpoint per environment leaked one into the customer's workspace on every
run.

### Changed

- Files split to stay under the contract's 650-line ceiling (§9), with no
  behaviour change: the namespace accessors moved out of `src/client.rs` into
  `src/namespaces.rs`, six conversation routes into
  `src/resources/conversations_ext.rs` as a second `impl` block, the shape
  table into `tests/common/shapes.rs`, the invocation table into
  `tests/common/invocations.rs`, and the new path assertions into
  `tests/surface_paths.rs`.
- `tests/surface.rs` is no longer "all of them". The lock suite drives every
  method and diffs its verb and path against the manifest, which is stronger
  than restating a path by hand; this file keeps the original 55 and adds
  only the new routes whose paths invite a specific mistake.

## 0.1.0 — 2026-10-12

Initial release. Implements `packages/sdk-spec/CONTRACT.md` and all 55
methods in `packages/sdk-spec/surface.json`.

### Response shapes are per-route, not assumed

Fourteen routes answer a bare JSON array, one answers `204`, and the rest
answer a single object or a cursor page. `macros().list` is the one that
mattered: the shared manifest briefly marked `GET /v1/macros` as paginated,
this crate deserialised it into a `Page`, and real callers got
`ChatlyError::Decode` while the suite stayed green — the fake transport
answered every route with a page envelope, so the test agreed with the bug.
The .NET SDK had the identical pair of defects.

Three things changed as a result:

- `macros().list` returns the array, and its `iter()` is gone. There is no
  cursor to ask for row 201; the service caps at `MACRO_LIBRARY_CAP`.
- `Page<T>`'s `Deserialize` is hand-written and strict. A derived one accepts
  a **sequence** as well as a map, so `[]` decoded as an empty page and `{}`
  did too — which is why the first version of the regression test passed. A
  page must now be a map carrying `items`; extra siblings (`hasMore`,
  `total`, `tier`, `facets`) are still ignored.
- The test transport answers each route the shape that route really returns,
  from a table of all 55 with no catch-all arm, and `tests/surface_lock.rs`
  asserts every method _succeeds_ against its own shape. Both halves were
  needed: the closures used to discard the `Result`.

### Added

- `ChatlyClient`, with a builder for base URL, timeout, retry policy,
  `User-Agent` suffix and an injectable `Transport`. Defaults to
  `https://api.chatlychat.com`; rejects an empty API key at construction.
- 55 methods across 13 namespaces — `contacts`, `conversations`, `tickets`,
  `macros`, `kb`, `search`, `events`, `webhooks`, `reports`, `channels`,
  `billing`, `workspace`, plus `whoami`.
- `billing().usage(event_type, query)` takes the event type as a **required
  argument**. The handler validates it against a closed list and throws
  before doing anything else, so an omitted one is a `400` rather than an
  all-types rollup. `USAGE_EVENT_TYPES` lists what the server accepts;
  `usage_range` sets the window.
- `ChatlyError`: a variant per case a caller branches on
  (`Authentication`, `Permission`, `NotFound`, `Conflict`, `Validation`,
  `RateLimited { retry_after }`, `Server`, `Api`, `Transport`, `Decode`,
  `Config`), each carrying `status`, `code`, `message`, `request_id` and the
  raw `body` where applicable. Implements `Error` and `Display`.
  Validation covers **400 and 422**, because 400 is what this API sends.
- Retries on connection failures, `429` and `5xx` only: three attempts,
  exponential backoff jittered into `[0.5x, 1.0x)`, `Retry-After` honoured
  and capped. A retried write reuses the `Idempotency-Key` of the attempt
  that failed.
- A fresh UUID v4 `Idempotency-Key` on every `POST`/`PUT`/`PATCH`/`DELETE`,
  and `with_idempotency_key` to pin one across calls.
- `PageIter`: an `Iterator` that walks cursors, stops on a null cursor, and
  stops rather than spinning if the server repeats one.
- `verify_webhook_signature` / `verify_webhook_signature_with`: HMAC-SHA256
  over `<t>.<raw body>`, constant-time compare, 300-second default window.
  Returns `false` and never panics on malformed input.
- `identity_hash` for widget identity verification.
- `request`, `request_as` and `paginate` as the escape hatch to the ~85
  key-accessible routes with no typed method.
- `Query`: order- and multiplicity-preserving, so `filter[]` repeats instead
  of being comma-joined.
- Typed request bodies where the API's schema is small and verified:
  `ContactInput`, `BulkContactsInput`, `MessageInput`, `MessageKind`,
  `EventInput`; and the vocabularies `SEARCH_KINDS`, `TICKET_BULK_ACTIONS`,
  `MACRO_SCOPES`, `MACRO_ACTION_KEYS`, `AGENT_HANDLING_SETTABLE_STATES`,
  `USAGE_EVENT_TYPES`, `MACRO_LIBRARY_CAP`.
- `surface.lock.json`, generated by `tests/surface_lock.rs` executing all 55
  methods and recording the request each one issues.

### Deliberately absent

- **Response types.** Every method returns `serde_json::Value`. The manifest
  pins verbs and paths and says nothing about response shapes, so a
  hand-written `struct Contact` would be a guess that compiles and drifts
  silently. `request_as::<T>()` and `paginate::<T>()` are generic over any
  `Deserialize` if you want your own.
- **An async variant.** No feature flag either; a blocking client needs no
  runtime. Use `spawn_blocking`.
- **Reactions and channel configuration.** Neither route accepts an API key,
  so the methods would 403 for every caller.
- **`workspace().teams()` and `workspace().agents()`.** `GET /v1/teams`
  carries no `@AllowApiKey()` and a bare `GET /v1/agents` has no handler.
  The manifest replaced them with `team_members` and `agent_skills`, which
  are what a key can actually read.

### Dependencies

Three: `ureq` 3.4 (HTTP + rustls), `serde` and `serde_json`. SHA-256, HMAC,
hex, UUID v4 and percent-encoding are implemented on `std` in this crate
rather than pulled in, so `sha2`, `hmac`, `uuid`, `rand`, `url` and
`percent-encoding` are **not** in your dependency tree.
