//! The [`Strip`] windowing core. The public f64 API lives here and in
//! [`query`]; the sub-pixel math behind it is [`math`].

use alloc::vec::Vec;

use crate::units::{from_sub, to_sub};

// The windowing/geometry math lives ONCE, in the `StripBackend` impl (see
// `math`) and the generic free functions in `super`: the inherent methods are
// the documented f64 API, each delegating, none re-implementing.
use super::StripBackend;

mod math;
mod query;

#[cfg(test)]
mod tests;

/// A column of variably-sized items separated by a fixed gap.
///
/// Construct one with [`Strip::new`] (explicit sizes) or [`Strip::uniform`]
/// (all items the same size), then query it. Rebuild it when the sizes change,
/// or — for finer-grained updates — call [`Strip::set_size`].
///
/// Internally the prefix-sum is stored as `i64` sub-pixels, so the public `f64`
/// API is exact for every common UI coordinate (multiples of `1/65536` of a
/// CSS pixel). See the crate-level docs for the rationale.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Strip {
    /// `starts[i]` is the offset of item `i`, in sub-pixels; `starts[len]` is
    /// the total extent including the trailing item but no trailing gap.
    /// Always has `len + 1` entries, or is empty when there are no items.
    starts: Vec<i64>,
    /// Gap between adjacent items, in sub-pixels.
    gap: i64,
}

impl Strip {
    /// Build a strip from explicit item sizes. Inputs are trusted as-is.
    pub fn new<I>(sizes: I, gap: f64) -> Self
    where
        I: IntoIterator<Item = f64>,
    {
        let iter = sizes.into_iter();
        let (lower, _) = iter.size_hint();
        let mut starts = Vec::with_capacity(lower + 1);
        let gap_sub = to_sub(gap);
        let mut acc: i64 = 0;
        for size in iter {
            starts.push(acc);
            acc = acc.saturating_add(to_sub(size)).saturating_add(gap_sub);
        }
        if !starts.is_empty() {
            // Total extent excludes the gap after the final item.
            starts.push(acc.saturating_sub(gap_sub));
        }
        Self {
            starts,
            gap: gap_sub,
        }
    }

    /// Build a strip of `count` items that all have the same size.
    ///
    /// The size may be an ESTIMATE the caller refines with [`Strip::set_size`]
    /// as real measurements arrive — the placeholder-height pattern, and what
    /// a strip of not-yet-rendered pages is built with.
    pub fn uniform(count: usize, size: f64, gap: f64) -> Self {
        Self::new(core::iter::repeat_n(size, count), gap)
    }

    /// Number of items.
    #[inline]
    pub fn len(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }

    /// Whether the strip has no items.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// The gap between adjacent items, in CSS pixels.
    #[inline]
    pub fn gap(&self) -> f64 {
        from_sub(self.gap)
    }

    /// Change the size of a single item in `O(n)` time — a measured size
    /// replacing an estimate, or an interactive element resizing. After it,
    /// [`Strip::offset`] and [`Strip::size`] reflect the new size, items
    /// below shift, and the total updates.
    ///
    /// Returns the **delta** (new_size - old_size) in CSS pixels, which the
    /// caller feeds to [`crate::anchor::correct`] to keep the viewport pinned
    /// to whatever the reader was looking at. Does nothing if `index` is out
    /// of range or the size is unchanged.
    pub fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        StripBackend::set_size(self, index, new_size)
    }
}
