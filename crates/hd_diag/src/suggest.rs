//! The "did you mean" of a misspelt name, shared by every unknown-name,
//! -type, -field, -variant and -method report (#2332). It follows rustc's
//! `rustc_span::edit_distance::find_best_match_for_name`: a match that
//! differs only in case, else the least restricted Damerau-Levenshtein
//! distance within `max(len, 3) / 3`, else the same `_`-separated words in
//! another order. Unlike rustc, which takes the first of equal candidates,
//! a suggestion is made only when one candidate is best, since a fix-it
//! that applies it must be certain; a tie suggests nothing.

use hd_base::SliceSort as _;
use hd_base::Span;

use crate::{DiagBuf, FixSafety};

/// The optimal-string-alignment distance from `a` to `b` in characters, or
/// `None` past `limit`: rustc's `edit_distance`, with its early exit on the
/// lengths and its stripping of a common prefix and suffix. O(|a|·|b|)
/// time, three rows of the shorter name's length in space.
#[must_use]
pub fn edit_distance(a: &str, b: &str, limit: usize) -> Option<usize> {
    let mut a: &[char] = &a.chars().collect::<Vec<_>>();
    let mut b: &[char] = &b.chars().collect::<Vec<_>>();
    if a.len() < b.len() {
        std::mem::swap(&mut a, &mut b);
    }
    let min = a.len() - b.len();
    if min > limit {
        return None;
    }
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    (a, b) = (&a[prefix..], &b[prefix..]);
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    (a, b) = (&a[..a.len() - suffix], &b[..b.len() - suffix]);
    let mut before = vec![usize::MAX; b.len() + 1];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let sub = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + sub);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                cur[j] = cur[j].min(before[j - 2] + 1);
            }
        }
        [before, prev, cur] = [prev, cur, before];
    }
    let d = prev[b.len()];
    (d <= limit).then_some(d)
}

/// The words of a `_`-separated name, sorted.
fn sorted_words(name: &str) -> Vec<&str> {
    let mut words: Vec<&str> = name.split('_').collect();
    words.sort_stable();
    words
}

/// The one candidate `lookup` most likely misspells, among those `checks`
/// accepts: ranked by case-only difference, then edit distance, then
/// reordered words, and taken only when it alone holds the best rank that
/// any accepted candidate holds. `checks` runs on the best ranks only, and
/// stops at a second accepted candidate. Cost: O(n·k²) for n candidates of
/// length about k, on an error only, plus a sort of the near ones.
pub fn best_match<'a>(
    lookup: &str,
    candidates: impl IntoIterator<Item = &'a str>,
    mut checks: impl FnMut(&str) -> bool,
) -> Option<&'a str> {
    let lower = lookup.to_lowercase();
    let limit = lookup.chars().count().max(3) / 3;
    let words = sorted_words(lookup);
    let mut near: Vec<(usize, &str)> = candidates
        .into_iter()
        .filter(|c| *c != lookup)
        .filter_map(|c| {
            let rank = if c.to_lowercase() == lower {
                0
            } else if let Some(d) = edit_distance(lookup, c, limit) {
                1 + d
            } else if words.len() > 1 && sorted_words(c) == words {
                2 + limit
            } else {
                return None;
            };
            Some((rank, c))
        })
        .collect();
    near.sort_stable();
    near.dedup();
    let mut found = None;
    for &(rank, c) in &near {
        if found.is_some_and(|(r, _)| r < rank) {
            break;
        }
        if checks(c) {
            if found.is_some() {
                return None;
            }
            found = Some((rank, c));
        }
    }
    found.map(|(_, c)| c)
}

/// The end of a message naming the best match, or nothing.
#[must_use]
pub fn did_you_mean(best: Option<&str>) -> String {
    best.map_or_else(String::new, |b| format!("; did you mean `{b}`?"))
}

impl DiagBuf {
    /// Attaches to the diagnostic pushed last the exact fix-it that writes
    /// `best` over the misspelt name at `name`, when there is a best match.
    pub fn add_typo_fix(&mut self, name: Span, best: Option<&str>) {
        if let Some(b) = best {
            self.add_fix(&format!("write `{b}`"), FixSafety::Exact, &[(name, b)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{best_match, edit_distance};

    #[test]
    fn distance_counts_a_transposition_as_one_edit() {
        assert_eq!(edit_distance("cuont", "count", 1), Some(1));
        assert_eq!(edit_distance("kitten", "sitting", 3), Some(3));
        assert_eq!(edit_distance("kitten", "sitting", 2), None);
        assert_eq!(edit_distance("abc", "abcdef", 2), None);
        assert_eq!(edit_distance("", "ab", 2), Some(2));
        assert_eq!(edit_distance("τιμή", "τημή", 1), Some(1));
    }

    #[test]
    fn one_best_candidate_is_taken_and_a_tie_is_not() {
        let all = |_: &str| true;
        assert_eq!(
            best_match("cuont", ["count", "mount2", "x"], all),
            Some("count")
        );
        // `nope` is 4 from `cmp`, past `max(4, 3) / 3 = 1`.
        assert_eq!(best_match("nope", ["cmp", "iter"], all), None);
        assert_eq!(best_match("Strng", ["String", "Strong"], all), None);
        assert_eq!(
            best_match("String", ["string", "Strings"], all),
            Some("string")
        );
        assert_eq!(
            best_match("max_count", ["count_max"], all),
            Some("count_max")
        );
        // A rejected candidate leaves the next rank to the others.
        assert_eq!(
            best_match("lenn", ["len", "lent"], |c| c == "lent"),
            Some("lent")
        );
        assert_eq!(best_match("ab", ["ab"], all), None);
    }
}
