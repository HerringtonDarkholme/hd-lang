//! Unbounded origin sets. A machine word stays inline; wider sets keep
//! only their nonzero words, shared until a union changes them. Thus an
//! independent reference costs one word even in a body with many owners.

use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Origins {
    Small(u64),
    Wide(Rc<[(usize, u64)]>),
}

impl Default for Origins {
    fn default() -> Self {
        Self::Small(0)
    }
}

impl Origins {
    pub(super) fn bit(k: usize) -> Self {
        let word = 1 << (k % 64);
        if k < 64 {
            Self::Small(word)
        } else {
            Self::Wide(Rc::from([(k / 64, word)]))
        }
    }

    pub(super) fn first_word(&self) -> u64 {
        self.words()
            .next()
            .filter(|&(k, _)| k == 0)
            .map_or(0, |(_, w)| w)
    }

    pub(super) fn is_empty(&self) -> bool {
        matches!(self, Self::Small(0))
    }

    fn words(&self) -> impl Iterator<Item = (usize, u64)> + '_ {
        let (small, wide) = match self {
            Self::Small(0) => (None, &[][..]),
            Self::Small(word) => (Some((0, *word)), &[][..]),
            Self::Wide(words) => (None, words.as_ref()),
        };
        small.into_iter().chain(wide.iter().copied())
    }

    pub(super) fn bits(&self) -> impl Iterator<Item = usize> + '_ {
        self.words()
            .flat_map(|(k, word)| super::bits(word).map(move |bit| 64 * k + bit))
    }

    fn packed(words: Vec<(usize, u64)>) -> Self {
        match words.as_slice() {
            [] => Self::default(),
            [(0, word)] => Self::Small(*word),
            _ => Self::Wide(Rc::from(words)),
        }
    }

    fn same(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (Self::Small(a), Self::Small(b)) => a == b,
            (Self::Wide(a), Self::Wide(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Sparse word merge: neither the domain width nor zero gaps are walked.
    fn merge(&self, rhs: &Self, union: bool) -> Self {
        if self.same(rhs) {
            return self.clone();
        }
        if let (Self::Small(a), Self::Small(b)) = (self, rhs) {
            return Self::Small(if union { a | b } else { a & b });
        }
        if self.is_empty() {
            return if union { rhs.clone() } else { Self::default() };
        }
        if rhs.is_empty() {
            return if union { self.clone() } else { Self::default() };
        }
        let (mut a, mut b) = (self.words().peekable(), rhs.words().peekable());
        let mut out = Vec::new();
        while let (Some(&(ka, wa)), Some(&(kb, wb))) = (a.peek(), b.peek()) {
            match ka.cmp(&kb) {
                std::cmp::Ordering::Equal => {
                    let word = if union { wa | wb } else { wa & wb };
                    if word != 0 {
                        out.push((ka, word));
                    }
                    a.next();
                    b.next();
                }
                std::cmp::Ordering::Less => {
                    if union {
                        out.push((ka, wa));
                    }
                    a.next();
                }
                std::cmp::Ordering::Greater => {
                    if union {
                        out.push((kb, wb));
                    }
                    b.next();
                }
            }
        }
        if union {
            out.extend(a);
            out.extend(b);
        }
        let merged = Self::packed(out);
        if merged == *self {
            self.clone()
        } else if merged == *rhs {
            rhs.clone()
        } else {
            merged
        }
    }

    pub(super) fn without(&self, rhs: &Self) -> Self {
        if let (Self::Small(a), Self::Small(b)) = (self, rhs) {
            return Self::Small(a & !b);
        }
        if rhs.is_empty() {
            return self.clone();
        }
        if self.same(rhs) {
            return Self::default();
        }
        let mut b = rhs.words().peekable();
        let mut out = Vec::new();
        for (k, word) in self.words() {
            while b.peek().is_some_and(|&(other, _)| other < k) {
                b.next();
            }
            let word = word
                & !b.peek()
                    .filter(|&&(other, _)| other == k)
                    .map_or(0, |&(_, w)| w);
            if word != 0 {
                out.push((k, word));
            }
        }
        Self::packed(out)
    }
}

impl BitOr for Origins {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.merge(&rhs, true)
    }
}
impl BitAnd for Origins {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.merge(&rhs, false)
    }
}
impl BitOrAssign for Origins {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.merge(&rhs, true);
    }
}
impl BitAndAssign for Origins {
    fn bitand_assign(&mut self, rhs: Self) {
        *self = self.merge(&rhs, false);
    }
}

#[cfg(test)]
mod tests {
    use super::Origins;

    #[test]
    fn unions_and_differences_cross_words_without_a_domain_limit() {
        let mut all = Origins::default();
        let mut odds = Origins::default();
        for k in 0..2000 {
            all |= Origins::bit(k);
            if k % 2 == 1 {
                odds |= Origins::bit(k);
            }
        }
        assert_eq!(
            all.bits().collect::<Vec<_>>(),
            (0..2000).collect::<Vec<_>>()
        );
        assert_eq!((all.clone() & odds.clone()), odds);
        assert_eq!(
            all.without(&odds).bits().collect::<Vec<_>>(),
            (0..2000).step_by(2).collect::<Vec<_>>()
        );
        assert!(all.without(&all).is_empty());
        // A high-numbered singleton neither allocates nor visits zero words.
        assert_eq!(Origins::bit(1_000_000).words().count(), 1);
    }
}

impl PartialEq<u64> for Origins {
    fn eq(&self, rhs: &u64) -> bool {
        matches!(self, Self::Small(w) if w == rhs)
    }
}
impl PartialEq<u64> for &Origins {
    fn eq(&self, rhs: &u64) -> bool {
        matches!(self, Origins::Small(w) if w == rhs)
    }
}

/// The four monotone columns read by an origin transfer.
#[derive(Clone, Copy)]
pub(super) enum Column {
    Value,
    Held,
    Dependency,
    LocalDependency,
}

/// A transfer subscribes to the facts it reads. Growing a fact schedules
/// only its readers; subscriptions include dynamically discovered pointees.
/// Every fact/transfer edge is registered once, even in a reference cycle.
#[derive(Default)]
pub(super) struct Worklist {
    n: usize,
    locals: usize,
    current: Option<usize>,
    readers: Vec<Vec<usize>>,
    edges: hd_base::HashSet<(usize, usize)>,
    queue: std::collections::VecDeque<usize>,
    pending: Vec<bool>,
}

impl Worklist {
    pub(super) fn new(n: usize, locals: usize) -> Self {
        Self {
            n,
            locals,
            readers: vec![Vec::new(); 2 * (n + locals)],
            queue: (0..n).collect(),
            pending: vec![true; n],
            ..Self::default()
        }
    }

    fn key(&self, kind: Column, index: usize) -> usize {
        match kind {
            Column::Value => index,
            Column::Held => self.n + index,
            Column::Dependency => self.n + self.locals + index,
            Column::LocalDependency => 2 * self.n + self.locals + index,
        }
    }

    pub(super) fn read(&mut self, kind: Column, index: usize) {
        if let Some(i) = self.current {
            let key = self.key(kind, index);
            if self.edges.insert((key, i)) {
                self.readers[key].push(i);
            }
        }
    }

    pub(super) fn grew(&mut self, kind: Column, index: usize) {
        let key = self.key(kind, index);
        for &i in &self.readers[key] {
            if !self.pending[i] {
                self.pending[i] = true;
                self.queue.push_back(i);
            }
        }
    }

    pub(super) fn next(&mut self) -> Option<usize> {
        self.current = self.queue.pop_front();
        if let Some(i) = self.current {
            self.pending[i] = false;
        }
        self.current
    }
}
