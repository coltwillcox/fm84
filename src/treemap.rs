//! The space map screen's layout: a directory's entries as blocks, each as
//! big as the room it takes, laid out squarified - as near square as they
//! can be made - so the large ones read at a glance.

use ratatui::layout::Rect;

/// A block: the entry it is, by its place in the panel, or the rest of them
/// together, too small to have one each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Entry(usize),
    /// How many entries it holds, and their bytes.
    Rest(usize, u64),
}

/// Cells a block is given at the least before what is smaller still goes
/// into the rest: room for a name, if a short one.
const SMALLEST: f64 = 12.0;

/// At most this many blocks of their own, however big the screen.
const MOST: usize = 400;

/// A terminal's cell is about twice as tall as it is wide, so a block looks
/// square at twice as many columns as rows.
const CELL_ASPECT: f64 = 2.0;

/// Each of `sizes` - an entry's place and its bytes - as a block of `area`,
/// the largest first. Nothing of no size is given one; those too small to
/// show share one block at the end, as the rest.
pub fn layout(sizes: &[(usize, u64)], area: Rect) -> Vec<(Tile, Rect)> {
    let cells = f64::from(area.width) * f64::from(area.height);
    let mut sorted: Vec<(usize, u64)> = sizes.iter().copied().filter(|&(_, size)| size > 0).collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let total: f64 = sorted.iter().map(|&(_, size)| size as f64).sum();
    if total == 0.0 || cells == 0.0 {
        return Vec::new();
    }
    // Those too small for a block of their own, from the first of them on.
    let kept = sorted.iter().take(MOST).take_while(|&&(_, size)| size as f64 / total * cells >= SMALLEST).count();
    let mut tiles: Vec<(Tile, f64)> = sorted[..kept].iter().map(|&(index, size)| (Tile::Entry(index), size as f64)).collect();
    match &sorted[kept..] {
        [] => {}
        // One alone is itself, small as it is.
        [(index, size)] => tiles.push((Tile::Entry(*index), *size as f64)),
        rest => {
            let bytes = rest.iter().fold(0u64, |total, &(_, size)| total.saturating_add(size));
            tiles.push((Tile::Rest(rest.len(), bytes), bytes as f64));
        }
    }
    tiles.sort_by(|a, b| b.1.total_cmp(&a.1));

    // Laid out where a cell is as wide as it is tall, then each block's
    // edges rounded to cells: blocks side by side share an edge, so they
    // tile the area with no gap and no overlap.
    let (width, height) = (f64::from(area.width), f64::from(area.height) * CELL_ASPECT);
    let scale = width * height / total;
    let areas: Vec<f64> = tiles.iter().map(|(_, size)| size * scale).collect();
    let mut placed = Vec::with_capacity(tiles.len());
    squarify(&areas, Frame { x: 0.0, y: 0.0, width, height }, &mut placed);
    tiles
        .iter()
        .zip(placed)
        .filter_map(|((tile, _), frame)| {
            let column = |x: f64| area.x + (x.round() as u16).min(area.width);
            let row = |y: f64| area.y + ((y / CELL_ASPECT).round() as u16).min(area.height);
            let (left, right) = (column(frame.x), column(frame.x + frame.width));
            let (top, bottom) = (row(frame.y), row(frame.y + frame.height));
            (right > left && bottom > top).then(|| (*tile, Rect::new(left, top, right - left, bottom - top)))
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Bruls, Huizing and van Wijk's squarified treemap: `areas`, largest first,
/// laid in rows along the shorter side of what is left, each row taking
/// another block for as long as that makes its worst block squarer.
fn squarify(areas: &[f64], mut frame: Frame, placed: &mut Vec<Frame>) {
    let mut start = 0;
    while start < areas.len() {
        let short = frame.width.min(frame.height);
        let mut end = start + 1;
        while end < areas.len() && worst(&areas[start..=end], short) <= worst(&areas[start..end], short) {
            end += 1;
        }
        let row = &areas[start..end];
        let sum: f64 = row.iter().sum();
        if frame.width >= frame.height {
            // A column down the left.
            let width = if frame.height > 0.0 { (sum / frame.height).min(frame.width) } else { 0.0 };
            let mut y = frame.y;
            for &area in row {
                let height = if width > 0.0 { area / width } else { 0.0 };
                placed.push(Frame { x: frame.x, y, width, height });
                y += height;
            }
            frame = Frame {
                x: frame.x + width,
                width: frame.width - width,
                ..frame
            };
        } else {
            // A row along the top.
            let height = if frame.width > 0.0 { (sum / frame.width).min(frame.height) } else { 0.0 };
            let mut x = frame.x;
            for &area in row {
                let width = if height > 0.0 { area / height } else { 0.0 };
                placed.push(Frame { x, y: frame.y, width, height });
                x += width;
            }
            frame = Frame {
                y: frame.y + height,
                height: frame.height - height,
                ..frame
            };
        }
        start = end;
    }
}

/// How far from square the least square of `row` would be, laid along a
/// side `short` long: 1 for square, more the longer and thinner.
fn worst(row: &[f64], short: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    let (largest, smallest) = row.iter().fold((0.0f64, f64::MAX), |(largest, smallest), &area| (largest.max(area), smallest.min(area)));
    if sum <= 0.0 || smallest <= 0.0 || short <= 0.0 {
        return f64::MAX;
    }
    let (short, sum) = (short * short, sum * sum);
    (short * largest / sum).max(sum / (short * smallest))
}

/// The arrow keys' way across the blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// The block an arrow goes to from `from`: of those wholly that way, the
/// nearest, and of those as near, the one most in line with it.
pub fn neighbour(blocks: &[(Tile, Rect)], from: usize, direction: Direction) -> Option<usize> {
    let here = blocks.get(from)?.1;
    // Twice the centre, to keep to whole numbers.
    let centre = |rect: Rect| (2 * i32::from(rect.x) + i32::from(rect.width), 2 * i32::from(rect.y) + i32::from(rect.height));
    let (x, y) = centre(here);
    blocks
        .iter()
        .enumerate()
        .filter(|&(index, _)| index != from)
        .filter_map(|(index, &(_, rect))| {
            let (gap, across) = match direction {
                Direction::Right if rect.x >= here.right() => (rect.x - here.right(), (centre(rect).1 - y).abs()),
                Direction::Left if rect.right() <= here.x => (here.x - rect.right(), (centre(rect).1 - y).abs()),
                Direction::Down if rect.y >= here.bottom() => (rect.y - here.bottom(), (centre(rect).0 - x).abs()),
                Direction::Up if rect.bottom() <= here.y => (here.y - rect.bottom(), (centre(rect).0 - x).abs()),
                _ => return None,
            };
            // Overlapping it across the way it goes, ahead of one only
            // diagonally there.
            let beside = match direction {
                Direction::Left | Direction::Right => rect.y < here.bottom() && here.y < rect.bottom(),
                Direction::Up | Direction::Down => rect.x < here.right() && here.x < rect.right(),
            };
            Some(((!beside, gap, across), index))
        })
        .min()
        .map(|(_, index)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(blocks: &[(Tile, Rect)]) -> u32 {
        blocks.iter().map(|(_, rect)| u32::from(rect.width) * u32::from(rect.height)).sum()
    }

    /// Every cell is some block's and no two blocks share one.
    fn tiles_exactly(blocks: &[(Tile, Rect)], whole: Rect) {
        assert_eq!(area(blocks), u32::from(whole.width) * u32::from(whole.height), "{blocks:?}");
        for (index, (_, a)) in blocks.iter().enumerate() {
            assert!(whole.contains(a.as_position()) && a.right() <= whole.right() && a.bottom() <= whole.bottom(), "{a:?} outside");
            for (_, b) in &blocks[index + 1..] {
                assert!(a.intersection(*b).is_empty(), "{a:?} and {b:?} overlap");
            }
        }
    }

    #[test]
    fn blocks_are_as_big_as_what_they_hold_and_fill_the_area() {
        let whole = Rect::new(3, 2, 80, 20);
        let blocks = layout(&[(0, 0), (1, 600), (2, 300), (3, 100)], whole);
        tiles_exactly(&blocks, whole);
        assert_eq!(blocks.iter().map(|(tile, _)| *tile).collect::<Vec<_>>(), [Tile::Entry(1), Tile::Entry(2), Tile::Entry(3)], "nothing for no size");
        let cells = |at: usize| f64::from(blocks[at].1.width) * f64::from(blocks[at].1.height);
        assert!((cells(0) / 1600.0 - 0.6).abs() < 0.05, "{blocks:?}");
        assert!((cells(1) / 1600.0 - 0.3).abs() < 0.05, "{blocks:?}");
        assert!(layout(&[(0, 0)], whole).is_empty());
        assert!(layout(&[(0, 5)], Rect::new(0, 0, 0, 10)).is_empty());
    }

    #[test]
    fn the_largest_is_near_square_and_the_tiny_share_a_block() {
        let whole = Rect::new(0, 0, 100, 30);
        // One big, a few middling, and a thousand crumbs.
        let mut sizes = vec![(0, 1_000_000), (1, 300_000), (2, 200_000), (3, 100_000)];
        sizes.extend((4..1004).map(|index| (index, 10)));
        let blocks = layout(&sizes, whole);
        tiles_exactly(&blocks, whole);
        assert!(blocks.iter().any(|(tile, _)| *tile == Tile::Rest(1000, 10_000)), "{blocks:?}");
        let big = blocks[0].1;
        // As seen: columns are half as tall as rows.
        let ratio = f64::from(big.width) / (2.0 * f64::from(big.height));
        assert!((0.4..2.5).contains(&ratio), "{big:?}");
    }

    #[test]
    fn arrows_go_to_the_nearest_block_that_way() {
        //  0 0 | 1
        //  0 0 | 2
        //  3 3   3
        let blocks = vec![
            (Tile::Entry(0), Rect::new(0, 0, 4, 2)),
            (Tile::Entry(1), Rect::new(4, 0, 2, 1)),
            (Tile::Entry(2), Rect::new(4, 1, 2, 1)),
            (Tile::Entry(3), Rect::new(0, 2, 6, 1)),
        ];
        assert_eq!(neighbour(&blocks, 0, Direction::Right), Some(1));
        assert_eq!(neighbour(&blocks, 2, Direction::Left), Some(0));
        assert_eq!(neighbour(&blocks, 1, Direction::Down), Some(2));
        assert_eq!(neighbour(&blocks, 2, Direction::Down), Some(3));
        assert_eq!(neighbour(&blocks, 3, Direction::Up), Some(0));
        assert_eq!(neighbour(&blocks, 0, Direction::Left), None);
        assert_eq!(neighbour(&blocks, 9, Direction::Left), None);
    }
}
