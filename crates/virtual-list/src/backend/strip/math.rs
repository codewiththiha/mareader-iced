//! The strip's sub-pixel core: the `StripBackend` impl every generic windowing
//! path in `super` runs.

use crate::units::to_sub;

use super::{Strip, StripBackend};

impl StripBackend for Strip {
    #[inline]
    fn len(&self) -> usize {
        self.len()
    }
    fn gap_sub(&self) -> i64 {
        self.gap
    }
    fn offset_sub(&self, index: usize) -> i64 {
        match self.starts.get(index) {
            Some(&v) => v,
            None => self.total_sub(),
        }
    }
    fn size_sub(&self, index: usize) -> i64 {
        let len = self.len();
        if index >= len {
            return 0;
        }
        let end = if index + 1 == len {
            self.starts[len]
        } else {
            self.starts[index + 1].saturating_sub(self.gap)
        };
        end.saturating_sub(self.starts[index]).max(0)
    }
    fn total_sub(&self) -> i64 {
        self.starts.last().copied().unwrap_or(0)
    }
    fn index_at_sub(&self, p: i64) -> usize {
        let len = self.len();
        if len == 0 || p <= 0 {
            return 0;
        }
        let idx = self.starts[..len]
            .partition_point(|&s| s <= p)
            .saturating_sub(1);
        if self.starts[idx].saturating_add(self.size_sub(idx)) <= p && idx + 1 < len {
            idx + 1
        } else {
            idx
        }
    }

    /// The hinted leading-edge search: neighbour first, then a galloping
    /// bracket. This is the override every generic hinted windowing path
    /// over a [`Strip`] runs — the f64 entry point
    /// [`Strip::index_at_hinted`] delegates here.
    fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        let len = self.len();
        if len == 0 || pos <= 0.0 {
            *hint = 0;
            return 0;
        }
        let p = to_sub(pos);

        // Clamp hint to a valid item index.
        if *hint >= len {
            *hint = len - 1;
        }
        let h = *hint;

        // 1) O(1): still inside the same item?
        let h_start = self.starts[h];
        let h_end = h_start.saturating_add(self.size_sub(h));
        if p >= h_start && p < h_end {
            return h;
        }
        // 2) O(1): did we step into the next / previous item?
        if h + 1 < len {
            let n_start = self.starts[h + 1];
            let n_end = n_start.saturating_add(self.size_sub(h + 1));
            if p >= n_start && p < n_end {
                *hint = h + 1;
                return h + 1;
            }
        }
        if h > 0 {
            let p_start = self.starts[h - 1];
            let p_end = p_start.saturating_add(self.size_sub(h - 1));
            if p >= p_start && p < p_end {
                *hint = h - 1;
                return h - 1;
            }
        }

        // 3) Galloping search: bracket the answer, then binary search inside.
        let target = if p < h_start {
            // We jumped UPWARDS (back towards 0). Find the largest index `i`
            // with `starts[i] <= p` and `i <= h`.
            let mut lo = 0usize;
            let mut step = 1usize;
            // Probe 1, 2, 4, ... below h until we find an index whose start is
            // > p (so the answer is below it).
            let mut probe = h;
            loop {
                let next = probe.saturating_sub(step);
                if next == probe {
                    break;
                }
                if self.starts[next] <= p {
                    probe = next;
                    // We found a lower bound; binary search [probe, h].
                    lo = probe;
                    break;
                }
                probe = next;
                if probe == 0 {
                    break;
                }
                step <<= 1;
            }
            // Binary search in [lo, h] for the largest index whose start <= p.
            self.starts[lo..=h]
                .partition_point(|&s| s <= p)
                .saturating_sub(1)
                + lo
        } else {
            // We jumped DOWNWARDS (forward). Find the largest index `i` with
            // `starts[i] <= p` and `i >= h`.
            let mut hi = h;
            let mut step = 1usize;
            let mut probe = h;
            loop {
                let next = probe.saturating_add(step).min(len - 1);
                if next == probe {
                    break;
                }
                if self.starts[next] > p {
                    hi = next;
                    break;
                }
                probe = next;
                if probe == len - 1 {
                    // Reached the end; the answer is len-1 (or its
                    // neighbour, resolved below).
                    hi = len - 1;
                    break;
                }
                step <<= 1;
            }
            // Binary search in [h, hi].
            h + self.starts[h..=hi]
                .partition_point(|&s| s <= p)
                .saturating_sub(1)
        };

        // Apply the same boundary rule as `index_at`: if pos is at or past the
        // end of the candidate, the next item leads.
        let idx =
            if self.starts[target].saturating_add(self.size_sub(target)) <= p && target + 1 < len {
                target + 1
            } else {
                target
            };
        *hint = idx;
        idx
    }
    fn set_size_sub(&mut self, index: usize, new_sub: i64) -> i64 {
        let len = self.len();
        if index >= len {
            return 0;
        }
        let old_sub = self.size_sub(index);
        if new_sub == old_sub {
            return 0;
        }
        let delta = new_sub.saturating_sub(old_sub);
        // O(n) suffix walk, run once per MEASURED page (each measurement
        // lands in its own flush, not in a loop over `n`): a 2 000-page book
        // pays ~2 000 cache-friendly i64 adds per measured page, faster than
        // a Fenwick tree at this scale. If counts ever reach tens of
        // thousands, switch `starts` to a Fenwick tree for O(log n).
        for i in (index + 1)..=len {
            self.starts[i] = self.starts[i].saturating_add(delta);
        }
        delta
    }
}
