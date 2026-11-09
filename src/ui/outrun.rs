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
    let colours = palette();
    let (grid, road, sun_top, sun_bottom, car, lights) = (
        Style::new().fg(colours.directory),
        Style::new().fg(colours.columns),
        Style::new().fg(colours.selected_marker),
        Style::new().fg(colours.rename_background),
        Style::new().fg(colours.selected_foreground),
        Style::new().fg(colours.rename_background).add_modifier(Modifier::BOLD),
    );
    let mut cells: Vec<Vec<Cell>> = vec![vec![(' ', Style::new()); width]; height];
    let put = |cells: &mut Vec<Vec<Cell>>, x: isize, y: usize, cell: Cell| {
        if let Some(row) = cells.get_mut(y)
            && let Some(slot) = usize::try_from(x).ok().and_then(|x| row.get_mut(x))
        {
            *slot = cell;
        }
    };
    let horizon = (height * 2 / 5).max(2);
    let ground = height - horizon - 1;
    let centre = (width / 2) as isize;
    // A cell is about twice as tall as it is wide.
    let time = frame as f64 * 0.15;

    // The sun, sitting on the horizon, the nearer the bigger; its lower half
    // cut into slats, as the eighties drew it.
    let radius = 1.5 + fraction * (horizon as f64 - 1.0);
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
            put(&mut cells, x, y, ('█', if lower { sun_bottom } else { sun_top }));
        }
    }
    for x in 0..width {
        put(&mut cells, x as isize, horizon, ('─', grid));
    }

    // The ground: lines across that come on towards the viewer, lines that
    // run out from the vanishing point, and the road between its edges.
    let road_half = |depth: f64| 1.0 + width as f64 / 6.0 * depth;
    for row in 1..=ground {
        let y = horizon + row;
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
            let reach = (road_half(depth) + k as f64 * spacing * depth * 1.2).round() as isize;
            put(&mut cells, centre - reach, y, ('/', grid));
            put(&mut cells, centre + reach, y, ('\\', grid));
        }
        let half = road_half(depth).round() as isize;
        for x in centre - half + 1..centre + half {
            put(&mut cells, x, y, (' ', Style::new()));
        }
        put(&mut cells, centre - half, y, ('/', road));
        put(&mut cells, centre + half, y, ('\\', road));
        if (1.5 / depth - time * 1.5).rem_euclid(1.0) < 0.5 {
            put(&mut cells, centre, y, ('│', road));
        }
    }

    // The car: at the bottom when nothing is done, at the horizon when all
    // of it is - or, stopping, off the road to the right with its tracks.
    if ground >= 3 {
        let travel = ground - 3;
        let bottom = height - 1 - (fraction * travel as f64).round() as usize;
        let depth = (bottom - horizon) as f64 / ground as f64;
        let x = if swerving { centre + road_half(depth).round() as isize + 3 } else { centre - 2 };
        for (offset, glyph) in "▄▄▄".chars().enumerate() {
            put(&mut cells, x + 1 + offset as isize, bottom - 1, (glyph, car));
        }
        for (offset, glyph) in "▟▀▀▀▙".chars().enumerate() {
            let style = if offset == 0 || offset == 4 { lights } else { car };
            put(&mut cells, x + offset as isize, bottom, (glyph, style));
        }
        if swerving {
            put(&mut cells, x - 2, bottom, ('~', road));
            put(&mut cells, x - 1, bottom, ('~', road));
        }
    }

    cells
        .into_iter()
        .map(|row| {
            // Runs of one style as one span each.
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
}
