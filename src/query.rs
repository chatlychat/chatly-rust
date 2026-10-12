//! Query-string and path-segment building.
//!
//! Hand-rolled rather than pulling in `url` + `percent-encoding`, and
//! deliberately **order- and multiplicity-preserving**: CONTRACT.md §2
//! requires that `kinds[]=kb&kinds[]=article` go out as two parameters and
//! not one comma-joined value, which a map-backed query builder cannot do.

/// An ordered list of query parameters.
///
/// Built by chaining. [`Query::push`] appends, so a repeated key stays
/// repeated; [`Query::set`] replaces every existing occurrence of a key,
/// which is what you want for `cursor` while paginating.
///
/// ```
/// use chatly::Query;
///
/// let q = Query::new()
///     .limit(50)
///     .filter("status:open")
///     .filter("channel:web")
///     .sort("-created_at");
/// assert_eq!(
///     q.to_query_string(),
///     "limit=50&filter%5B%5D=status%3Aopen&filter%5B%5D=channel%3Aweb&sort=-created_at"
/// );
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pairs: Vec<(String, String)>,
}

impl Query {
    /// An empty query.
    pub fn new() -> Self {
        Self { pairs: Vec::new() }
    }

    /// Append a parameter, keeping any existing one with the same key.
    ///
    /// This is the method to use for `filter[]` and `kinds[]`.
    pub fn push(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.pairs.push((key.into(), value.into()));
        self
    }

    /// Append a parameter only when `value` is `Some`.
    pub fn push_opt(self, key: impl Into<String>, value: Option<impl ToString>) -> Self {
        match value {
            Some(value) => self.push(key, value.to_string()),
            None => self,
        }
    }

    /// Replace every parameter with this key, or append if there is none.
    pub fn set(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        self.pairs.retain(|(existing, _)| existing != &key);
        self.pairs.push((key, value.into()));
        self
    }

    /// Remove every parameter with this key.
    pub fn remove(mut self, key: &str) -> Self {
        self.pairs.retain(|(existing, _)| existing != key);
        self
    }

    /// `cursor=…` — replaces any cursor already set.
    pub fn cursor(self, cursor: impl Into<String>) -> Self {
        self.set("cursor", cursor)
    }

    /// `limit=…` — page size.
    pub fn limit(self, limit: u32) -> Self {
        self.set("limit", limit.to_string())
    }

    /// `filter[]=…` — appends, so call it once per filter.
    pub fn filter(self, filter: impl Into<String>) -> Self {
        self.push("filter[]", filter)
    }

    /// `sort=…`, e.g. `-created_at` for descending.
    pub fn sort(self, sort: impl Into<String>) -> Self {
        self.set("sort", sort)
    }

    /// `fields=…` — a sparse-fieldset projection.
    pub fn fields(self, fields: impl Into<String>) -> Self {
        self.set("fields", fields)
    }

    /// Whether any parameter is set.
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// How many parameters are set, counting repeats separately.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// The parameters in order, for inspection and for a custom transport.
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    /// The first value for a key, if any.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value.as_str())
    }

    /// Encode as `a=1&b=2`, percent-encoding both sides. No leading `?`.
    pub fn to_query_string(&self) -> String {
        let mut out = String::new();
        for (key, value) in &self.pairs {
            if !out.is_empty() {
                out.push('&');
            }
            out.push_str(&encode_component(key));
            out.push('=');
            out.push_str(&encode_component(value));
        }
        out
    }
}

impl<K: Into<String>, V: Into<String>> From<Vec<(K, V)>> for Query {
    fn from(pairs: Vec<(K, V)>) -> Self {
        let mut query = Query::new();
        for (key, value) in pairs {
            query = query.push(key, value);
        }
        query
    }
}

/// Percent-encode everything outside RFC 3986's unreserved set.
///
/// Conservative on purpose: `+` is encoded as `%2B` and a space as `%20`, so
/// the result is safe in a query string and in a path segment alike. A
/// `+`-for-space encoder would corrupt a cursor containing a literal `+`,
/// which base64 cursors do.
fn encode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

/// Percent-encode a value being interpolated into a URL path.
///
/// Every `{id}`, `{slug}` and `{tag}` in `packages/sdk-spec/surface.json` goes
/// through this. A tag is customer-supplied free text and may contain a space
/// or a `/`; interpolating it raw would either produce an invalid URI or,
/// worse, address a different route.
pub(crate) fn encode_path_segment(input: &str) -> String {
    encode_component(input)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn repeated_keys_stay_repeated() {
        let q = Query::new().push("kinds[]", "kb").push("kinds[]", "article");
        assert_eq!(q.len(), 2);
        assert_eq!(q.to_query_string(), "kinds%5B%5D=kb&kinds%5B%5D=article");
    }

    #[test]
    fn set_replaces_and_push_appends() {
        let q = Query::new().push("limit", "10").set("limit", "25");
        assert_eq!(q.to_query_string(), "limit=25");
        let q = Query::new().push("limit", "10").push("limit", "25");
        assert_eq!(q.to_query_string(), "limit=10&limit=25");
    }

    #[test]
    fn cursor_is_replaced_not_appended() {
        // The paginator calls `.cursor()` once per page on a clone of the
        // caller's query; appending would send every cursor it had ever seen.
        let q = Query::new().cursor("c1").cursor("c2").cursor("c3");
        assert_eq!(q.to_query_string(), "cursor=c3");
    }

    #[test]
    fn encodes_reserved_characters_and_utf8() {
        assert_eq!(encode_component("a b"), "a%20b");
        assert_eq!(encode_component("a+b"), "a%2Bb");
        assert_eq!(encode_component("a/b"), "a%2Fb");
        assert_eq!(encode_component("a&b=c"), "a%26b%3Dc");
        assert_eq!(encode_component("héllo"), "h%C3%A9llo");
        assert_eq!(encode_component("-_.~"), "-_.~");
    }

    #[test]
    fn path_segments_are_encoded() {
        assert_eq!(encode_path_segment("vip customer"), "vip%20customer");
        assert_eq!(encode_path_segment("a/../b"), "a%2F..%2Fb");
    }

    #[test]
    fn push_opt_skips_none() {
        let q = Query::new()
            .push_opt("limit", Some(10))
            .push_opt("cursor", None::<String>);
        assert_eq!(q.to_query_string(), "limit=10");
    }

    #[test]
    fn empty_query_encodes_to_nothing() {
        assert!(Query::new().is_empty());
        assert_eq!(Query::new().to_query_string(), "");
    }

    #[test]
    fn from_vec_preserves_order() {
        let q = Query::from(vec![("b", "2"), ("a", "1")]);
        assert_eq!(q.to_query_string(), "b=2&a=1");
    }

    #[test]
    fn get_and_remove() {
        let q = Query::new().push("a", "1").push("a", "2");
        assert_eq!(q.get("a"), Some("1"));
        assert!(q.remove("a").is_empty());
    }
}
