// src/lint/similarity.rs
//
// Textual near-identity of SQL predicates, for the mirror-drift check (L6).
//
// This is not a SQL parser and must not become one. Two predicates that mean
// the same thing but read differently are none of lint's business; two that
// read almost the same are the interesting case, because they are what one
// predicate looks like after someone edited a copy of it.
//
// ## Index
// - DEFAULT_THRESHOLD     — similarity at or above which two predicates are "near-identical"
// - [normalize-sql]       normalize_sql()  — case/whitespace/punctuation-insensitive form
// - [similarity]          similarity()     — 0.0..=1.0 normalized edit distance over normalized text

/// Similarity at or above which two predicates are reported as near-identical.
///
/// Tuned to catch a copy someone edited (a changed threshold, an added
/// condition) while ignoring two predicates that merely share SQL keywords.
pub const DEFAULT_THRESHOLD: f64 = 0.85;

// [normalize-sql]
/// Normalize a predicate for comparison: lowercase, collapse whitespace, drop
/// trailing semicolons, and separate punctuation so `count(*)` and `count( * )`
/// compare equal.
///
/// The point is to compare what the predicate *says*, not how it was typed —
/// reformatting one copy of a duplicated predicate must not hide the
/// duplication.
pub fn normalize_sql(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut last_was_space = true;
    for c in sql.trim().trim_end_matches(';').chars() {
        if c.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else if c.is_alphanumeric() || c == '_' || c == '\'' || c == '"' {
            out.extend(c.to_lowercase());
            last_was_space = false;
        } else {
            // Punctuation becomes its own token so spacing around it stops
            // mattering.
            if !last_was_space {
                out.push(' ');
            }
            out.push(c);
            out.push(' ');
            last_was_space = true;
        }
    }
    out.trim().to_string()
}

// [similarity]
/// Similarity of two predicates in `0.0..=1.0`, as `1 - (edit distance / length)`
/// over their normalized forms.
///
/// Token-level rather than character-level: a renamed column should count as
/// one difference, not eight. Two identical predicates score 1.0.
pub fn similarity(a: &str, b: &str) -> f64 {
    let at: Vec<&str> = a.split_whitespace().collect();
    let bt: Vec<&str> = b.split_whitespace().collect();

    if at.is_empty() && bt.is_empty() {
        return 1.0;
    }
    let longest = at.len().max(bt.len());
    if longest == 0 {
        return 1.0;
    }

    let distance = levenshtein(&at, &bt);
    1.0 - (distance as f64 / longest as f64)
}

/// Token-level Levenshtein distance (two-row dynamic programming).
fn levenshtein(a: &[&str], b: &[&str]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];

    for (i, at) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, bt) in b.iter().enumerate() {
            let cost = usize::from(at != bt);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }

    prev[b.len()]
}
