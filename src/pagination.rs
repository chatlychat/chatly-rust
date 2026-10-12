//! Cursor pagination: one page, or an iterator over all of them.
//!
//! CONTRACT.md §5: never sort or page on `id`. Ids in this product are a mix
//! of UUID v4 and v7, so `id` order is not time order and a keyset built on
//! it silently reorders. The cursor is the only correct sequence, which is
//! why nothing here exposes an offset.

use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::error::ChatlyError;

/// One page of a list endpoint, exactly as the API returned it.
///
/// `next_cursor` is `null` on the last page. Pass it back as `cursor` to get
/// the next one — or use [`PageIter`] and let the SDK do it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
// `#[serde(default)]` on `items` makes serde's derive infer a `T: Default`
// bound it does not actually need (rust-lang/rust#26925, "imperfect
// derives"). Spelled out here so `Page<T>` deserialises for any
// `Deserialize` T, not only one that is also `Default`.
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct Page<T> {
    /// The rows on this page.
    #[serde(default)]
    pub items: Vec<T>,
    /// The cursor for the next page, or `None` if this was the last.
    #[serde(default)]
    pub next_cursor: Option<String>,
}

impl<T> Page<T> {
    /// Whether this is the last page.
    pub fn is_last(&self) -> bool {
        self.next_cursor.is_none()
    }

    /// Number of rows on this page.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether this page carried no rows.
    ///
    /// An empty page is not necessarily the last one: a server-side filter
    /// can drop every row in a window and still hand back a cursor.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self { items: Vec::new(), next_cursor: None }
    }
}

impl<T> IntoIterator for Page<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

/// The page-fetching closure a [`PageIter`] drives.
///
/// Takes the cursor to request (`None` for the first page) and returns that
/// page. `Send` so the iterator can be moved between threads; boxed so a
/// `PageIter` is a concrete type a caller can name and store.
type Fetch<T> = Box<dyn FnMut(Option<String>) -> Result<Page<T>, ChatlyError> + Send>;

/// An iterator that walks every page of a list endpoint.
///
/// Yields `Result<T, ChatlyError>`: one `Err` ends the iteration, so a
/// `for` loop sees at most one failure and then stops rather than hammering
/// a broken endpoint.
///
/// Termination has two guards, and both matter:
///
/// 1. a `null` `nextCursor` ends it, which is the normal case; and
/// 2. a cursor the iterator has already requested ends it too. A server bug
///    that echoes the same cursor back forever would otherwise spin in
///    `next()` without ever returning, and the caller would see a hang
///    rather than an error.
///
/// What it does **not** guard against is a server that returns an unbounded
/// sequence of *distinct* cursors; that is indistinguishable from a very
/// large collection, and treating it as an error would break legitimate
/// full-workspace scans. Use [`Iterator::take`] if you want a ceiling.
pub struct PageIter<T> {
    fetch: Fetch<T>,
    buffer: VecDeque<T>,
    next_cursor: Option<String>,
    requested: HashSet<String>,
    pages_fetched: usize,
    started: bool,
    done: bool,
}

impl<T> PageIter<T> {
    /// Build an iterator from a page-fetching closure and a starting cursor.
    ///
    /// Mostly internal — [`crate::ChatlyClient::paginate`] is the public way
    /// in — but public so a caller can auto-paginate a route reached through
    /// the escape hatch with a hand-built request.
    pub fn new(
        start_cursor: Option<String>,
        fetch: impl FnMut(Option<String>) -> Result<Page<T>, ChatlyError> + Send + 'static,
    ) -> Self {
        let mut requested = HashSet::new();
        if let Some(cursor) = &start_cursor {
            // Seeded so a server that echoes the caller's own starting cursor
            // back as `nextCursor` is caught by guard 2 on the first page.
            requested.insert(cursor.clone());
        }
        Self {
            fetch: Box::new(fetch),
            buffer: VecDeque::new(),
            next_cursor: start_cursor,
            requested,
            pages_fetched: 0,
            started: false,
            done: false,
        }
    }

    /// How many pages have been fetched so far.
    pub fn pages_fetched(&self) -> usize {
        self.pages_fetched
    }

    /// Collect every item, failing on the first error.
    ///
    /// Convenience for the common "I want them all and an error should
    /// propagate" case. Loads the whole collection into memory; iterate
    /// instead if that is a problem.
    pub fn collect_all(self) -> Result<Vec<T>, ChatlyError> {
        self.collect()
    }
}

impl<T> Iterator for PageIter<T> {
    type Item = Result<T, ChatlyError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(item) = self.buffer.pop_front() {
                return Some(Ok(item));
            }
            if self.done {
                return None;
            }
            // A `None` cursor means "first page" only before the first fetch;
            // afterwards it means the previous page was the last one.
            if self.started && self.next_cursor.is_none() {
                self.done = true;
                return None;
            }

            let cursor = self.next_cursor.take();
            self.started = true;
            let page = match (self.fetch)(cursor) {
                Ok(page) => page,
                Err(err) => {
                    self.done = true;
                    return Some(Err(err));
                }
            };
            self.pages_fetched += 1;
            self.buffer.extend(page.items);

            match page.next_cursor {
                None => self.done = true,
                Some(cursor) => {
                    if !self.requested.insert(cursor.clone()) {
                        // Guard 2: we have already asked for this cursor.
                        self.done = true;
                    } else {
                        self.next_cursor = Some(cursor);
                    }
                }
            }
        }
    }
}

impl<T> std::fmt::Debug for PageIter<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageIter")
            .field("pages_fetched", &self.pages_fetched)
            .field("buffered", &self.buffer.len())
            .field("next_cursor", &self.next_cursor)
            .field("done", &self.done)
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn page(items: &[&str], next: Option<&str>) -> Page<String> {
        Page {
            items: items.iter().map(|s| (*s).to_owned()).collect(),
            next_cursor: next.map(str::to_owned),
        }
    }

    /// A log of the cursors a scripted iterator was asked for.
    type CursorLog = Arc<Mutex<Vec<Option<String>>>>;

    /// Returns the queued pages in order, recording every cursor it was
    /// asked for so a test can assert the cursor actually travelled.
    fn scripted(pages: Vec<Page<String>>) -> (PageIter<String>, CursorLog) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let mut queue = VecDeque::from(pages);
        let iter = PageIter::new(None, move |cursor| {
            if let Ok(mut log) = recorder.lock() {
                log.push(cursor.clone());
            }
            Ok(queue.pop_front().unwrap_or_default())
        });
        (iter, seen)
    }

    #[test]
    fn deserialises_the_api_page_shape() {
        let decoded: Page<serde_json::Value> =
            serde_json::from_str(r#"{"items":[{"id":"a"}],"nextCursor":"c1"}"#).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded.next_cursor.as_deref(), Some("c1"));
        assert!(!decoded.is_last());

        let last: Page<serde_json::Value> =
            serde_json::from_str(r#"{"items":[],"nextCursor":null}"#).unwrap();
        assert!(last.is_last());
        assert!(last.is_empty());
    }

    #[test]
    fn walks_two_pages_and_stops_on_a_null_cursor() {
        let (iter, seen) = scripted(vec![
            page(&["a", "b"], Some("c1")),
            page(&["c"], None),
        ]);
        let items: Vec<String> = iter.map(|r| r.unwrap()).collect();
        assert_eq!(items, vec!["a", "b", "c"]);
        assert_eq!(
            *seen.lock().unwrap(),
            vec![None, Some("c1".to_owned())],
            "the second request must carry the first page's cursor"
        );
    }

    #[test]
    fn stops_when_the_server_repeats_a_cursor() {
        // Without guard 2 this is an infinite loop.
        let calls = Arc::new(Mutex::new(0usize));
        let counter = Arc::clone(&calls);
        let iter = PageIter::new(None, move |_cursor| {
            if let Ok(mut n) = counter.lock() {
                *n += 1;
            }
            Ok(page(&["x"], Some("stuck")))
        });
        let items: Vec<String> = iter.map(|r| r.unwrap()).collect();
        assert_eq!(items, vec!["x", "x"], "one page, then the repeat is refused");
        assert_eq!(*calls.lock().unwrap(), 2);
    }

    #[test]
    fn stops_when_the_server_echoes_the_starting_cursor() {
        let calls = Arc::new(Mutex::new(0usize));
        let counter = Arc::clone(&calls);
        let iter = PageIter::new(Some("start".to_owned()), move |_cursor| {
            if let Ok(mut n) = counter.lock() {
                *n += 1;
            }
            Ok(page(&["y"], Some("start")))
        });
        let items: Vec<String> = iter.map(|r| r.unwrap()).collect();
        assert_eq!(items, vec!["y"]);
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[test]
    fn skips_empty_intermediate_pages() {
        let (iter, _) = scripted(vec![
            page(&[], Some("c1")),
            page(&[], Some("c2")),
            page(&["z"], None),
        ]);
        let items: Vec<String> = iter.map(|r| r.unwrap()).collect();
        assert_eq!(items, vec!["z"]);
    }

    #[test]
    fn an_error_ends_the_iteration() {
        let calls = Arc::new(Mutex::new(0usize));
        let counter = Arc::clone(&calls);
        let iter = PageIter::new(None, move |_cursor| {
            let mut n = counter.lock().map_err(|_| ChatlyError::transport("poisoned"))?;
            *n += 1;
            if *n == 1 {
                Ok(page(&["a"], Some("c1")))
            } else {
                Err(ChatlyError::transport("boom"))
            }
        });
        let results: Vec<_> = iter.collect();
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert_eq!(*calls.lock().unwrap(), 2, "no further pages after an error");
    }

    #[test]
    fn a_single_empty_last_page_yields_nothing() {
        let (iter, seen) = scripted(vec![page(&[], None)]);
        assert_eq!(iter.count(), 0);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn pages_fetched_is_reported() {
        let (mut iter, _) = scripted(vec![page(&["a"], Some("c1")), page(&["b"], None)]);
        assert_eq!(iter.pages_fetched(), 0);
        assert!(iter.next().is_some());
        assert_eq!(iter.pages_fetched(), 1);
        let _ = iter.by_ref().count();
        assert_eq!(iter.pages_fetched(), 2);
    }

    #[test]
    fn collect_all_propagates_the_first_error() {
        let iter: PageIter<String> =
            PageIter::new(None, move |_| Err(ChatlyError::transport("nope")));
        assert!(iter.collect_all().is_err());
    }

    #[test]
    fn page_into_iter_works() {
        let items: Vec<String> = page(&["a", "b"], None).into_iter().collect();
        assert_eq!(items, vec!["a", "b"]);
    }
}
