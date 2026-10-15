//! The two checks on the response-shape column that the lock cannot make.
//!
//! `tests/surface_lock.rs` proves, per route, that the method copes with the
//! shape `common::shape_for` declares and hands back a value of that shape.
//! Two things are still missing from that, and both are the kind of hole this
//! whole exercise exists to close:
//!
//! 1. **The column could be unanimous.** A table that said `object` for all
//!    163 routes would satisfy every assertion in the lock suite — each method
//!    would be fed an object, most of them really do answer objects, and the
//!    manifest would have to be wrong in the same way for the diff to pass,
//!    which is exactly the correlated failure the per-SDK column is supposed
//!    to break. So the distribution is asserted outright.
//!
//! 2. **The positive test cannot show the declaration MATTERS.** A decoder
//!    that ignored the body entirely would pass it. The test that bites is
//!    the negative one: feed every auto-paginator a bare array where it
//!    expects `{ items, nextCursor }` and require an `Err`.
//!
//! That second test is the one that should have existed. A derived
//! `Deserialize` for `Page<T>` read a bare JSON array as
//! `Page { items: [], next_cursor: None }` — serde can build a struct from a
//! sequence — so `macros().list`, typed `Page` while the manifest wrongly
//! said `paged: true`, answered a caller with an EMPTY PAGE and a 200. Not a
//! decode error: a wrong answer, silently. `Page`'s deserialiser is
//! hand-written and strict now (`src/pagination.rs`), and this is the suite
//! that holds it that way through the client, not just in a unit test on
//! `serde_json::from_str`.
//!
//! Run with `cargo test --test surface_shapes`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;

use chatly::{ChatlyClient, ChatlyError, PageIter, Query};
use common::{client, manifest, shape_for, BodyShape, MockTransport};
use serde_json::Value;

/// The declared column, as a histogram, over exactly the manifest's routes.
///
/// Built from the manifest's keys rather than from the table itself, because
/// `shape_for` is a `match` and cannot be enumerated — which is also why it
/// has no catch-all arm.
fn declared_counts() -> BTreeMap<&'static str, usize> {
    let mut counts: BTreeMap<&'static str, usize> =
        [("array", 0), ("empty", 0), ("object", 0), ("page", 0)]
            .into_iter()
            .collect();
    for entry in manifest() {
        let shape = shape_for(&entry.ns, &entry.name).unwrap_or_else(|| {
            panic!(
                "no response shape declared for {}.{} in tests/common/mod.rs",
                entry.ns, entry.name
            )
        });
        *counts.entry(shape.name()).or_insert(0) += 1;
    }
    counts
}

/// Anti-vacuity: the declared shapes are not all the same value.
///
/// The numbers come from reading every `@ZodResponse` schema on the surface:
/// 14 / 10 / 30 / 1 over the first 55 routes on 2026-10-12, and 32 `array`,
/// 13 `page`, 113 `object`, 1 `empty` over the 159 on 2026-10-14, and
/// 33 / 13 / 115 / 1 once `root.me`, `webhooks.delete` and `workspace.mine`
/// joined the manifest later that day, and 33 / 13 / 116 / 1 once
/// `visitors.start_conversation` joined it on 2026-10-14 — one more `object`,
/// read off the service return type because that handler has no
/// `@ZodResponse`. They are
/// asserted exactly rather than as floors because the surface is pinned by a
/// committed lock; when a route is added, this test is one of the places that
/// must be updated on purpose.
///
/// All four values must stay represented, and `empty` must stay at exactly
/// one until a second real 204 genuinely ships — `macros.delete` is the only
/// one today, and the other six DELETEs on this surface answer with a body
/// (`contacts.remove_tag` with the surviving contact,
/// `conversations.cancel_scheduled` with the cancelled row, and
/// `ai.delete_tool`, `bot_routes.delete`, `brands.delete`,
/// `customer_cards.delete`, `customer_card_endpoints.delete`,
/// `docs_sources.delete`, `email_templates.delete`, `triggers.delete` and
/// `webhooks.delete` each with an acknowledgement object).
#[test]
fn the_declared_shapes_are_distributed_as_the_handlers_are() {
    let counts = declared_counts();
    assert_eq!(
        counts.values().sum::<usize>(),
        manifest().len(),
        "every manifest route must be counted exactly once"
    );
    assert_eq!(
        counts,
        [("array", 33), ("empty", 1), ("object", 116), ("page", 13)]
            .into_iter()
            .collect::<BTreeMap<&str, usize>>(),
        "the declared shape column changed shape; if a route was added, say \
         which and update these counts deliberately"
    );
}

/// Drive an auto-paginator one step and surface its first outcome.
///
/// `None` — the iterator finishing without yielding and without complaining —
/// maps to `Ok(())` on purpose. That is precisely the old bug's signature: a
/// bare array swallowed as an empty page looks to a caller like a correct,
/// finished, empty collection. The test below requires `Err`, so this arm
/// fails it rather than hiding it.
fn first_outcome(iter: PageIter<Value>) -> Result<(), ChatlyError> {
    match iter.into_iter().next() {
        Some(Ok(_)) => Ok(()),
        Some(Err(err)) => Err(err),
        None => Ok(()),
    }
}

type Walk = fn(&ChatlyClient) -> Result<(), ChatlyError>;

/// Every auto-paginating method on this client, keyed by its manifest route.
///
/// Hand-written, like the shape table, and asserted below to be exactly the
/// routes declared `page`. Both directions: a walker over a route with no
/// cursor could only ever return one page and look complete (which is why
/// `macros().list` has no `iter()`), and a `page` route with no walker here
/// is a route this test silently stops covering.
#[allow(clippy::type_complexity)]
fn walkers() -> Vec<(&'static str, &'static str, Walk)> {
    vec![
        ("contacts", "list", |c| {
            first_outcome(c.contacts().iter(&Query::new()))
        }),
        ("contacts", "journey", |c| {
            first_outcome(c.contacts().iter_journey("c_1", &Query::new()))
        }),
        ("conversations", "list", |c| {
            first_outcome(c.conversations().iter(&Query::new()))
        }),
        ("conversations", "messages", |c| {
            first_outcome(c.conversations().iter_messages("v_1", &Query::new()))
        }),
        ("conversations", "unattended", |c| {
            first_outcome(c.conversations().iter_unattended(&Query::new()))
        }),
        ("tickets", "list", |c| {
            first_outcome(c.tickets().iter(&Query::new()))
        }),
        ("kb", "articles", |c| {
            first_outcome(c.kb().iter_articles(&Query::new()))
        }),
        ("search", "query", |c| {
            first_outcome(c.search().iter("refund", &Query::new()))
        }),
        ("events", "list", |c| {
            first_outcome(c.events().iter(&Query::new()))
        }),
        ("webhooks", "deliveries", |c| {
            first_outcome(c.webhooks().iter_deliveries(&Query::new()))
        }),
        // The three the manifest did not flag until 2026-10-14. All three end
        // in `buildPaginatedResponse`, so all three really are pages.
        ("bots", "list", |c| {
            first_outcome(c.bots().iter(&Query::new()))
        }),
        ("bots", "deliveries", |c| {
            first_outcome(c.bots().iter_deliveries("b_1", &Query::new()))
        }),
        ("triggers", "list", |c| {
            first_outcome(c.triggers().iter(&Query::new()))
        }),
    ]
}

/// The set of walkers is exactly the set of `page` routes.
///
/// Without this, the test below could quietly shrink to nothing: delete a
/// walker and the loop still passes, over one fewer route.
#[test]
fn there_is_one_auto_paginator_for_each_page_route_and_nothing_else() {
    let mut walked: Vec<String> = walkers()
        .iter()
        .map(|(ns, name, _)| format!("{ns}.{name}"))
        .collect();
    walked.sort();
    assert_eq!(walked.len(), walkers().len(), "duplicate walker entry");

    let mut paged: Vec<String> = manifest()
        .iter()
        .filter(|e| shape_for(&e.ns, &e.name) == Some(BodyShape::Page))
        .map(|e| format!("{}.{}", e.ns, e.name))
        .collect();
    paged.sort();

    assert_eq!(
        walked, paged,
        "the auto-paginators and the routes declared `page` must be the same set"
    );
    assert_eq!(
        paged.len(),
        13,
        "expected 13 page routes, see the shape table"
    );
}

/// Every auto-paginator, fed a bare array where it expects a page, must fail.
///
/// This is the test whose absence shipped a wrong answer. `[{"id":"row_1"}]`
/// is a real response from this API — `GET /v1/macros` sends it — so a
/// paginator meeting one is not a hypothetical, and "an empty page, done" is
/// the worst possible reading of it.
#[test]
fn every_auto_paginator_refuses_a_bare_array_where_a_page_is_expected() {
    for (ns, name, walk) in walkers() {
        let mock = MockTransport::new();
        // Several queued: a walker that accepted the first bad body and asked
        // for another page must not panic the mock before it is judged.
        for _ in 0..3 {
            mock.push_shape(BodyShape::Array);
        }
        let outcome = walk(&client(&mock));
        assert!(
            outcome.is_err(),
            "{ns}.{name}'s auto-paginator accepted a bare array as a page \
             (body {}). A `Page` that deserialises from a sequence reads this \
             as an empty page and reports success, which is a wrong answer \
             rather than a missing one.",
            BodyShape::Array.body()
        );
        assert!(
            matches!(outcome.as_ref().err(), Some(ChatlyError::Decode { .. })),
            "{ns}.{name} rejected the bare array, but not as a decode error: {:?}",
            outcome.err()
        );
    }
}

/// And the positive control for the test above: the same walkers, fed real
/// pages, do yield the row.
///
/// Without this, "every paginator errors" would also be satisfied by a client
/// that could not paginate at all.
#[test]
fn the_same_auto_paginators_accept_a_real_page() {
    for (ns, name, walk) in walkers() {
        let mock = MockTransport::new();
        mock.push_shape(BodyShape::Page);
        assert!(
            walk(&client(&mock)).is_ok(),
            "{ns}.{name}'s auto-paginator rejected a real page envelope"
        );
        assert_eq!(
            mock.request_count(),
            1,
            "{ns}.{name} should stop on a null nextCursor"
        );
    }
}
