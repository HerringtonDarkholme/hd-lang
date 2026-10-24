//! The bitset states of the ownership passes' dataflow walks (moves and
//! known-null).

/// Bitset states of `words` words each in one growable arena, by start
/// offset, allocated and freed last in, first out.
#[derive(Default)]
pub(crate) struct BitArena {
    pub words: usize,
    cells: Vec<u64>,
}

impl BitArena {
    /// Where the next state goes.
    pub fn top(&self) -> usize {
        self.cells.len()
    }

    /// A new state of all zeros, or a copy of state `from`.
    pub fn alloc(&mut self, from: Option<usize>) -> usize {
        let at = self.cells.len();
        match from {
            Some(f) => self.cells.extend_from_within(f..f + self.words),
            None => self.cells.resize(at + self.words, 0),
        }
        at
    }

    /// Frees state `at` and every state allocated after it.
    pub fn free(&mut self, at: usize) {
        self.cells.truncate(at);
    }

    pub fn copy(&mut self, to: usize, from: usize) {
        self.cells.copy_within(from..from + self.words, to);
    }

    /// Empties state `at`.
    pub fn clear(&mut self, at: usize) {
        self.cells[at..at + self.words].fill(0);
    }

    /// `to |= from`; whether `to` grew.
    pub fn union(&mut self, to: usize, from: usize) -> bool {
        let mut grew = false;
        for k in 0..self.words {
            let v = self.cells[from + k];
            let w = &mut self.cells[to + k];
            grew |= v & !*w != 0;
            *w |= v;
        }
        grew
    }

    /// Joins `from` into `to`: a copy when `to` holds no path yet.
    pub fn join(&mut self, to: usize, any: &mut bool, from: usize) {
        if *any {
            self.union(to, from);
        } else {
            self.copy(to, from);
            *any = true;
        }
    }

    pub fn set(&mut self, at: usize, bit: u32, on: bool) {
        let w = &mut self.cells[at + (bit / 64) as usize];
        if on {
            *w |= 1 << (bit % 64);
        } else {
            *w &= !(1 << (bit % 64));
        }
    }

    pub fn get(&self, at: usize, bit: u32) -> bool {
        self.cells[at + (bit / 64) as usize] & (1 << (bit % 64)) != 0
    }

    /// State `at`'s words.
    pub fn row(&self, at: usize) -> &[u64] {
        &self.cells[at..at + self.words]
    }
}
