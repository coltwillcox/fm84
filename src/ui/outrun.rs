//! F11's Outrun progress bar: a job drawn as a car driving down a neon grid
//! towards the sun - the further it has got, the more of the job is done,
//! and the bigger the sun. Esc swerves it off the road.

use super::*;

/// The fewest rows the scene is drawn in; fewer, and the plain bar is.
pub(super) const SCENE_ROWS: usize = 7;

/// One cell of the scene.
type Cell = (char, Style);

/// The scene, `width` by `height`: `fraction` of the way to the sun, at
/// `frame` - a tenth of a second each - for the grid's movement, and with
/// the car off the road when the job is being stopped.
pub(super) fn outrun_scene(width: usize, height: usize, fraction: f64, frame: u64, swerving: bool) -> Vec<Line<'static>> {
    let fraction = fraction.clamp(0.0, 1.0);
    let horizon = (height * 2 / 5).max(2);
    let mut cells = grid_cells(width, height, horizon, fraction, frame);
    put_car(&mut cells, width, horizon, fraction, swerving);
    lines_of(cells)
}

/// The night drive, `width` by `height` at `frame`: the whole screen, the
/// sun big on the horizon with the logo over it, palms going by on either
/// side of the road, and the car on it for good. The logo is out when `lit`
/// is false - Turbo blinks it, as an attract screen's.
pub(super) fn night_drive_scene(width: usize, height: usize, frame: u64, lit: bool) -> Vec<Line<'static>> {
    // Half the screen sky: room for the sun and the logo both.
    let horizon = (height / 2).max(2);
    // The sun up to the logo's last row, where the logo has room; the logo
    // is then the first row down.
    let logo_rows = super::help::LOGO.len();
    let sun = if horizon >= logo_rows + 2 { ((horizon - logo_rows) as f64 - 1.5) / (horizon as f64 - 1.0) } else { 0.8 };
    let mut cells = grid_cells(width, height, horizon, sun, frame);
    put_palms(&mut cells, width, horizon, frame);
    put_car(&mut cells, width, horizon, 0.0, false);
    put_logo(&mut cells, width, horizon, lit);
    lines_of(cells)
}

/// Half the road's width, at `depth` from the horizon (0) to the bottom (1).
fn road_half(width: usize, depth: f64) -> f64 {
    1.0 + width as f64 / 6.0 * depth
}

/// Put `cell` at `x`, `y`, if that is in the scene.
fn put(cells: &mut [Vec<Cell>], x: isize, y: isize, cell: Cell) {
    if let Some(row) = usize::try_from(y).ok().and_then(|y| cells.get_mut(y))
        && let Some(slot) = usize::try_from(x).ok().and_then(|x| row.get_mut(x))
    {
        *slot = cell;
    }
}

/// The sun on the horizon, `sun` of the way to its biggest, and the ground
/// under it at `frame`.
fn grid_cells(width: usize, height: usize, horizon: usize, sun: f64, frame: u64) -> Vec<Vec<Cell>> {
    let colours = palette();
    let (grid, road, sun_top, sun_bottom) = (Style::new().fg(colours.directory), Style::new().fg(colours.columns), Style::new().fg(colours.selected_marker), Style::new().fg(colours.rename_background));
    let mut cells: Vec<Vec<Cell>> = vec![vec![(' ', Style::new()); width]; height];
    let ground = height - horizon - 1;
    let centre = (width / 2) as isize;
    // A cell is about twice as tall as it is wide.
    let time = frame as f64 * 0.15;

    // The sun, sitting on the horizon, the nearer the bigger; its lower half
    // cut into slats, as the eighties drew it.
    let radius = 1.5 + sun * (horizon as f64 - 1.0);
    for y in 0..horizon {
        let above = (horizon - y) as f64 - 0.5;
        if above > radius {
            continue;
        }
        let half = (radius * 2.0 * (1.0 - (above / radius).powi(2)).sqrt()).round() as isize;
        let lower = above < radius * 0.5;
        if lower && (horizon - y).is_multiple_of(2) {
            continue;
        }
        for x in centre - half..=centre + half {
            put(&mut cells, x, y as isize, ('█', if lower { sun_bottom } else { sun_top }));
        }
    }
    for x in 0..width {
        put(&mut cells, x as isize, horizon as isize, ('─', grid));
    }

    // The ground: lines across that come on towards the viewer, lines that
    // run out from the vanishing point, and the road between its edges.
    for row in 1..=ground {
        let y = (horizon + row) as isize;
        let depth = row as f64 / ground as f64;
        if (1.0 / depth - time).rem_euclid(1.0) < 0.22 {
            for x in 0..width {
                put(&mut cells, x as isize, y, ('─', grid));
            }
        }
        // Fanning out from the road's edges, not alongside them.
        let spacing = width as f64 / 7.0;
        // Too near the horizon to tell the lines apart, there are none.
        for k in (1..=10).filter(|_| spacing * depth * 1.2 >= 2.0) {
            let reach = (road_half(width, depth) + k as f64 * spacing * depth * 1.2).round() as isize;
            put(&mut cells, centre - reach, y, ('/', grid));
            put(&mut cells, centre + reach, y, ('\\', grid));
        }
        let half = road_half(width, depth).round() as isize;
        for x in centre - half + 1..centre + half {
            put(&mut cells, x, y, (' ', Style::new()));
        }
        put(&mut cells, centre - half, y, ('/', road));
        put(&mut cells, centre + half, y, ('\\', road));
        if (1.5 / depth - time * 1.5).rem_euclid(1.0) < 0.5 {
            put(&mut cells, centre, y, ('│', road));
        }
    }
    cells
}

/// The car: at the bottom when nothing is done, at the horizon when all of
/// it is - or, stopping, off the road to the right with its tracks.
fn put_car(cells: &mut [Vec<Cell>], width: usize, horizon: usize, fraction: f64, swerving: bool) {
    let colours = palette();
    let (road, car, lights) = (Style::new().fg(colours.columns), Style::new().fg(colours.selected_foreground), Style::new().fg(colours.rename_background).add_modifier(Modifier::BOLD));
    let height = cells.len();
    let ground = height - horizon - 1;
    if ground < 3 {
        return;
    }
    let centre = (width / 2) as isize;
    let travel = ground - 3;
    let bottom = height - 1 - (fraction * travel as f64).round() as usize;
    let depth = (bottom - horizon) as f64 / ground as f64;
    let x = if swerving { centre + road_half(width, depth).round() as isize + 3 } else { centre - 2 };
    let bottom = bottom as isize;
    for (offset, glyph) in "▄▄▄".chars().enumerate() {
        put(cells, x + 1 + offset as isize, bottom - 1, (glyph, car));
    }
    for (offset, glyph) in "▟▀▀▀▙".chars().enumerate() {
        let style = if offset == 0 || offset == 4 { lights } else { car };
        put(cells, x + offset as isize, bottom, (glyph, style));
    }
    if swerving {
        put(cells, x - 2, bottom, ('~', road));
        put(cells, x - 1, bottom, ('~', road));
    }
}

/// A palm, from far to near: the rows of each, top to bottom. A space is
/// the scene showing through.
const PALMS: [&[&str]; 4] = [&["Y"], &[r"\|/", " | "], &[r"\\|//", "  (  ", "  )  "], &[r"_\\|//_", r" /\|/\ ", "   (   ", "   )   ", "   (   ", "   )   "]];

/// Palms on either side of the road at `frame`, coming on from the horizon
/// as fast as the ground does, and growing as they come.
fn put_palms(cells: &mut [Vec<Cell>], width: usize, horizon: usize, frame: u64) {
    let colours = palette();
    let (fronds, trunk) = (Style::new().fg(colours.title), Style::new().fg(colours.columns));
    let ground = (cells.len() - horizon - 1) as f64;
    let centre = (width / 2) as isize;
    let time = frame as f64 * 0.15;
    // How far off each is, in the ground lines' measure, where 1 is the
    // bottom of the screen; three to a side, the right half a gap behind.
    const FARTHEST: f64 = 6.0;
    let mut palms: Vec<(f64, isize)> = (0..6)
        .map(|k| {
            let side = if k % 2 == 0 { -1 } else { 1 };
            let distance = (k as f64 - time).rem_euclid(FARTHEST) + 0.8;
            (distance, side)
        })
        .collect();
    // The far ones first, for the near ones to stand in front of them.
    palms.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (distance, side) in palms {
        let depth = 1.0 / distance;
        let y = horizon as isize + (depth * ground).round() as isize;
        let reach = (road_half(width, depth) + width as f64 * 0.1 * depth + 2.0).round() as isize;
        let sprite = PALMS[match depth {
            depth if depth < 0.25 => 0,
            depth if depth < 0.45 => 1,
            depth if depth < 0.7 => 2,
            _ => 3,
        }];
        let rows = sprite.len() as isize;
        for (row, text) in sprite.iter().enumerate() {
            let left = centre + side * reach - (text.chars().count() / 2) as isize;
            for (offset, glyph) in text.chars().enumerate().filter(|(_, glyph)| *glyph != ' ') {
                let style = if matches!(glyph, '(' | ')') || (rows > 1 && glyph == '|' && row > 0) { trunk } else { fronds };
                put(cells, left + offset as isize, y - rows + 1 + row as isize, (glyph, style));
            }
        }
    }
}

/// The logo a row down from the top, over the top of the sun - if the sky
/// has the room. Drawn whole, spaces and all, for the sun's blocks not to
/// run into its own.
fn put_logo(cells: &mut [Vec<Cell>], width: usize, horizon: usize, lit: bool) {
    let rows = super::help::LOGO.len();
    let used = super::help::LOGO[0].chars().count();
    if horizon < rows + 2 || width < used + 2 {
        return;
    }
    let shade = Style::new().fg(palette().border);
    let solid = if lit { Style::new().fg(palette().directory).add_modifier(Modifier::BOLD) } else { shade };
    let left = ((width - used) / 2) as isize;
    for (row, text) in super::help::LOGO.iter().enumerate() {
        for (offset, glyph) in text.chars().enumerate() {
            put(cells, left + offset as isize, 1 + row as isize, (glyph, if glyph == '█' { solid } else { shade }));
        }
    }
}

/// The cells as lines, a span for each run of one style.
fn lines_of(cells: Vec<Vec<Cell>>) -> Vec<Line<'static>> {
    cells
        .into_iter()
        .map(|row| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut run = String::new();
            let mut style = None;
            for (glyph, cell) in row {
                if style != Some(cell) && !run.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut run), style.unwrap_or_default()));
                }
                style = Some(cell);
                run.push(glyph);
            }
            spans.push(Span::styled(run, style.unwrap_or_default()));
            Line::from(spans)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect()).collect()
    }

    /// Where the car is: the row its wheels are on, and its first column.
    fn car(lines: &[Line<'_>]) -> (usize, usize) {
        text(lines).iter().enumerate().find_map(|(y, row)| row.chars().position(|c| c == '▟').map(|x| (y, x))).expect("no car")
    }

    #[test]
    fn the_car_drives_towards_the_sun_and_swerves_off_when_stopped() {
        let (width, height) = (60, 14);
        let start = outrun_scene(width, height, 0.0, 0, false);
        let half = outrun_scene(width, height, 0.5, 0, false);
        let end = outrun_scene(width, height, 1.0, 0, false);
        assert_eq!(start.len(), height);
        assert!(text(&start).iter().all(|row| row.chars().count() == width));
        assert_eq!(car(&start).0, height - 1, "at the bottom to start with");
        assert!(car(&half).0 < car(&start).0 && car(&end).0 < car(&half).0, "further up the road as it goes");
        // The sun is bigger the nearer it is.
        let sun = |lines: &[Line<'_>]| text(lines).iter().map(|row| row.matches('█').count()).sum::<usize>();
        assert!(sun(&end) > sun(&start));
        // Off the road, and to the right of where it was.
        let swerved = outrun_scene(width, height, 0.5, 0, true);
        assert!(car(&swerved).1 > car(&half).1 + 3);
        assert!(text(&swerved).iter().any(|row| row.contains("~~")));
        // The grid moves from one frame to the next.
        assert_ne!(text(&outrun_scene(width, height, 0.5, 0, false)), text(&outrun_scene(width, height, 0.5, 3, false)));
    }

    #[test]
    fn the_night_drive_has_the_logo_over_the_sun_and_palms_going_by() {
        let (width, height) = (80, 24);
        let scene = night_drive_scene(width, height, 0, true);
        assert_eq!(scene.len(), height);
        assert!(text(&scene).iter().all(|row| row.chars().count() == width));
        let rows = text(&scene);
        let logo_bottom = rows.iter().position(|row| row.contains("╚═╝     ╚═╝     ╚═╝")).expect("the logo");
        assert!(logo_bottom < height / 2, "in the sky");
        assert_eq!(car(&scene).0, height - 1, "the car on the road");
        // Palms either side of the road, and somewhere else a moment on.
        let palms = |lines: &[Line<'_>]| text(lines).iter().skip(height / 2).map(|row| row.chars().filter(|c| matches!(c, 'Y' | '(' | ')')).count()).sum::<usize>();
        assert!(palms(&scene) > 0);
        assert_ne!(text(&scene), text(&night_drive_scene(width, height, 4, true)));
        // Out, the logo's blocks are drawn as its shading is; the sun's stay.
        let bold = |lines: &[Line<'_>]| lines.iter().flat_map(|line| &line.spans).any(|span| span.style.add_modifier.contains(Modifier::BOLD) && span.content.contains('█'));
        assert!(bold(&scene) && !bold(&night_drive_scene(width, height, 0, false)));
        // No room in the sky for it, no logo.
        assert!(!text(&night_drive_scene(width, 12, 0, true)).iter().any(|row| row.contains('╚')));
    }
}
