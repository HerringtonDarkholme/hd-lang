//! A stack that always holds its base frame.
//!
//! The checker's scope stack and return-type stack start with the body's
//! own frame and every nested frame is pushed and popped around it, so the
//! top always exists. Holding the base outside the vector makes that a
//! fact of the type: `top` and `base` need no `Option`, and `pop` cannot
//! remove the base.

/// A non-empty stack of frames: `base`, then `inner` from outermost to
/// innermost. A frame's depth is its place from the base, which is 0.
#[derive(Clone, Debug)]
pub struct Frames<T> {
    base: T,
    inner: Vec<T>,
}

impl<T> Frames<T> {
    pub fn new(base: T) -> Self {
        Self {
            base,
            inner: Vec::new(),
        }
    }

    pub fn push(&mut self, frame: T) {
        self.inner.push(frame);
    }

    /// Pops the innermost frame; the base frame is never popped.
    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop()
    }

    /// The number of frames, the base included.
    #[cfg_attr(
        feature = "testing",
        expect(
            clippy::len_without_is_empty,
            reason = "the base frame is never popped"
        )
    )]
    pub fn len(&self) -> usize {
        self.inner.len() + 1
    }

    /// The innermost frame's depth.
    pub fn depth(&self) -> usize {
        self.inner.len()
    }

    /// Pops frames until `len` remain, keeping the base.
    pub fn truncate(&mut self, len: usize) {
        self.inner.truncate(len.saturating_sub(1));
    }

    pub fn base(&self) -> &T {
        &self.base
    }

    pub fn base_mut(&mut self) -> &mut T {
        &mut self.base
    }

    pub fn top(&self) -> &T {
        self.inner.last().unwrap_or(&self.base)
    }

    pub fn top_mut(&mut self) -> &mut T {
        match self.inner.last_mut() {
            Some(frame) => frame,
            None => &mut self.base,
        }
    }

    /// The frame at `depth`, which must be open.
    pub(crate) fn at_mut(&mut self, depth: usize) -> &mut T {
        match depth.checked_sub(1) {
            None => &mut self.base,
            Some(i) => &mut self.inner[i],
        }
    }

    /// Every frame, outermost first.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.base).chain(&self.inner)
    }

    /// Every frame with its depth, innermost first.
    pub fn iter_rev(&self) -> impl Iterator<Item = (usize, &T)> {
        self.inner
            .iter()
            .enumerate()
            .rev()
            .map(|(i, frame)| (i + 1, frame))
            .chain(std::iter::once((0, &self.base)))
    }
}
