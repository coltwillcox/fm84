//! Alt+Shift+Enter's space map: each entry's share of what the directory
//! holds, drawn as a bar behind its row, as ncdu draws it.

use super::*;

/// The bytes behind each entry of a panel with its space map on.
pub struct SpaceMap {
    /// Each entry's, in the panel's order; nothing for `..`, a link, or a
    /// directory not counted.
    pub sizes: Vec<u64>,
    /// The largest of them, which a full bar stands for.
    pub largest: u64,
    /// All of them, which a share is of.
    pub total: u64,
}

impl SpaceMap {
    /// How many of `width` cells the bar of the entry at `index` takes: at
    /// least one for anything at all, so a small file is not taken for an
    /// empty one.
    pub fn bar(&self, index: usize, width: u16) -> u16 {
        let size = self.sizes.get(index).copied().unwrap_or(0);
        if size == 0 || self.largest == 0 {
            return 0;
        }
        let cells = (size as u128 * width as u128 + self.largest as u128 / 2) / self.largest as u128;
        (cells as u16).clamp(1, width)
    }

    /// The share of the whole the entry at `index` is, in whole percent.
    pub fn percent(&self, index: usize) -> Option<u64> {
        let size = *self.sizes.get(index)?;
        (self.total > 0).then(|| (size as u128 * 100 / self.total as u128) as u64)
    }
}

impl AppState {
    /// The space map of `side`, while it is on and the panel is where it was
    /// asked for. A directory still being counted stands at what it has
    /// found so far, so the bars grow as the counting goes.
    pub fn space_map(&self, side: Side) -> Option<SpaceMap> {
        let panel = self.panel(side);
        if panel.space_map.as_ref() != Some(&panel.dir) {
            return None;
        }
        let sizes: Vec<u64> = panel
            .children
            .iter()
            .map(|item| {
                if item.is_parent() || item.is_symlink {
                    0
                } else if item.is_dir {
                    let path = item.path_in(&panel.dir);
                    self.dir_size_so_far(&path).or_else(|| self.dir_sizes.get(&path).copied()).unwrap_or(0)
                } else {
                    item.size_bytes
                }
            })
            .collect();
        let largest = sizes.iter().copied().max().unwrap_or(0);
        let total = sizes.iter().fold(0u64, |total, size| total.saturating_add(*size));
        Some(SpaceMap { sizes, largest, total })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bar is as long against the largest as its size is, and never less
    /// than a cell for something; a share is of the whole.
    #[test]
    fn bars_go_against_the_largest_and_shares_against_the_whole() {
        let map = SpaceMap {
            sizes: vec![0, 400, 100, 1],
            largest: 400,
            total: 501,
        };
        assert_eq!([0, 1, 2, 3].map(|index| map.bar(index, 40)), [0, 40, 10, 1]);
        assert_eq!(map.bar(9, 40), 0);
        assert_eq!(map.percent(1), Some(79));
        assert_eq!(map.percent(0), Some(0));
        let empty = SpaceMap { sizes: vec![0], largest: 0, total: 0 };
        assert_eq!((empty.bar(0, 40), empty.percent(0)), (0, None));
    }
}
