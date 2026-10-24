use super::*;
use crate::constants::{IMAGE_MAX_DECODED, IMAGE_ZOOM_NORMAL, IMAGE_ZOOM_STEPS};
use image::{Rgba, RgbaImage};

/// Sizes as an ordinary 2:1 cell gives them. The tests below are written
/// around that ratio, and the terminal a test run happens to be watched
/// from is no business of theirs.
fn image_size_for(image: &DynamicImage, width: usize, height: usize, fill: bool, zoom: u16) -> (usize, usize) {
    image_size_on(image, width, height, fill, zoom, CELL_ASPECT_FALLBACK)
}

fn solid(width: u32, height: u32, pixel: [u8; 4]) -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, Rgba(pixel)))
}

#[test]
fn colours_share_bands_and_stay_in_range() {
    // Neighbouring values collapse into one band, centred within it.
    let band = 1u8 << IMAGE_COLOR_DROP_BITS;
    assert_eq!(coarse(0), band / 2);
    assert_eq!(coarse(band - 1), band / 2);
    assert_eq!(coarse(band), band + band / 2);
    assert!((0..=255u8).all(|value| coarse(value).abs_diff(value) <= band / 2));
}

#[test]
fn ramp_ends_map_to_black_and_white() {
    assert_eq!(image_to_ascii_on(&solid(4, 4, [0, 0, 0, 255]), 4, 2, false, false).0[0], "    ");
    assert_eq!(image_to_ascii_on(&solid(4, 4, [255, 255, 255, 255]), 4, 2, false, false).0[0], "@@@@");
}

#[test]
fn a_light_background_turns_the_ramp_round() {
    assert_eq!(image_to_ascii_on(&solid(4, 4, [0, 0, 0, 255]), 4, 2, true, false).0[0], "@@@@");
    assert_eq!(image_to_ascii_on(&solid(4, 4, [255, 255, 255, 255]), 4, 2, true, false).0[0], "    ");
}

#[test]
fn transparent_is_left_blank() {
    for light in [false, true] {
        assert_eq!(image_to_ascii_on(&solid(4, 4, [255, 255, 255, 0]), 4, 2, light, false).0[0], "    ");
        assert_eq!(image_to_ascii_on(&solid(4, 4, [0, 0, 0, 0]), 4, 2, light, false).0[0], "    ");
    }
}

#[test]
fn backgrounds_carry_the_cell_colour() {
    let red = [200u8, 40, 60, 255];
    let want = Color::Rgb(coarse(200), coarse(40), coarse(60));

    // Off: nothing behind the characters, and the character itself carries
    // the colour, exactly as before.
    let (_, inks, backs) = image_to_ascii_on(&solid(4, 4, red), 4, 2, false, false);
    assert!(backs.is_empty());
    assert_eq!(inks[0][0], want);

    // On: the cell's own colour goes behind it, so the colour is right
    // whatever the glyph happens to cover.
    let (_, inks, backs) = image_to_ascii_on(&solid(4, 4, red), 4, 2, false, true);
    assert_eq!(backs.len(), inks.len());
    assert_eq!(backs[0].len(), inks[0].len());
    assert_eq!(backs[0][0], want);
    // And the glyph is moved off it, or there would be nothing to see.
    assert_ne!(inks[0][0], backs[0][0]);
}

#[test]
fn a_glyph_lifts_off_a_dark_cell_and_sinks_into_a_bright_one() {
    let level = |colour: Color| match colour {
        Color::Rgb(red, green, blue) => red as u32 + green as u32 + blue as u32,
        _ => unreachable!("image cells are always true colour"),
    };

    let (_, inks, backs) = image_to_ascii_on(&solid(2, 2, [20, 20, 20, 255]), 2, 1, false, true);
    assert!(level(inks[0][0]) > level(backs[0][0]), "a dark cell needs a lighter glyph");

    let (_, inks, backs) = image_to_ascii_on(&solid(2, 2, [240, 240, 240, 255]), 2, 1, false, true);
    assert!(level(inks[0][0]) < level(backs[0][0]), "a bright cell needs a darker glyph");
}

#[test]
fn rows_are_halved_for_the_cell_aspect() {
    let square = solid(100, 100, [128, 128, 128, 255]);
    let (columns, rows) = image_size_for(&square, 40, 40, false, IMAGE_ZOOM_NORMAL);
    assert_eq!((columns, rows), (40, 20));

    let (lines, colors, _) = image_to_ascii_on(&square, columns, rows, false, false);
    assert_eq!(lines.len(), 20);
    assert!(lines.iter().all(|line| line.chars().count() == 40));
    assert_eq!(colors.len(), 20);
    assert!(colors.iter().flatten().all(|color| *color == Color::Rgb(coarse(128), coarse(128), coarse(128))));
    // A very wide image still keeps one row.
    assert_eq!(image_size_for(&solid(1000, 1, [0, 0, 0, 255]), 10, 10, false, IMAGE_ZOOM_NORMAL).1, 1);
}

/// A drawing keeps the picture's shape on whatever cell the terminal has,
/// not only on the 2:1 one fm84 used to assume. Measured as the drawing's
/// shape on screen - columns of cells that wide, rows of cells that tall -
/// against the picture's own.
#[test]
fn keeps_proportions_on_any_cell() {
    for cell in [1.0, 1.6, 2.0, 2.4, 2.78, 3.5] {
        for (width, height) in [(400u32, 300u32), (100, 100), (192, 108), (60, 90)] {
            let image = solid(width, height, [128, 128, 128, 255]);
            let (columns, rows) = image_size_on(&image, 120, 40, false, IMAGE_ZOOM_NORMAL, cell);
            let drawn = columns as f64 / (rows as f64 * cell);
            let wanted = f64::from(width) / f64::from(height);
            // A row is a coarse unit: at 40 rows one of them is 2.5% of the
            // height, and the count is rounded to a whole one.
            assert!((drawn / wanted - 1.0).abs() < 0.05, "{width}x{height} on a {cell} cell drew {columns}x{rows}, shape {drawn:.3} against {wanted:.3}");
        }
    }
}

/// A terminal that says nothing useful about its cells leaves the drawing
/// where it always was.
#[test]
fn cell_aspect_is_believable() {
    let ratio = cell_aspect();
    assert!(CELL_ASPECT_RANGE.contains(&ratio), "{ratio}");
}

#[test]
fn decodes_by_content_and_rejects_the_rest() {
    let mut png = Vec::new();
    solid(3, 2, [255, 0, 0, 255]).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let (image, label) = load_image(&png).unwrap();
    assert_eq!((image.width(), image.height()), (3, 2));
    assert_eq!(label, "PNG 3x2");
    assert!(load_image(b"\0\x01\x02 not an image").is_none());
}

#[test]
fn fit_stays_inside_and_fill_covers() {
    // Cell aspect halves rows, so a square image is twice as wide as tall.
    let square = solid(100, 100, [0, 0, 0, 255]);
    assert_eq!(image_size_for(&square, 80, 20, false, IMAGE_ZOOM_NORMAL).0, 40);
    assert_eq!(image_size_for(&square, 80, 20, true, IMAGE_ZOOM_NORMAL).0, 80);
    assert_eq!(image_size_for(&square, 30, 20, false, IMAGE_ZOOM_NORMAL).0, 30);
    assert_eq!(image_size_for(&square, 30, 20, true, IMAGE_ZOOM_NORMAL).0, 40);

    // Fit is inside the viewer for every shape, including the slivers that
    // have to be squashed to get there.
    for (image_width, image_height) in [(100, 100), (1024, 576), (1, 1024), (4, 1024), (1024, 1)] {
        let image = solid(image_width, image_height, [0, 0, 0, 255]);
        for (width, height) in [(80, 20), (30, 20), (200, 50), (7, 3)] {
            let (columns, rows) = image_size_for(&image, width, height, false, IMAGE_ZOOM_NORMAL);
            assert!(columns <= width && rows <= height, "{image_width}x{image_height} fit in {width}x{height}: {columns}x{rows}");
            let drawn = image_to_ascii_on(&image, columns, rows, false, false).0;
            assert_eq!(drawn.len(), rows);
        }
    }

    // Fill covers the viewer, except where the cap in the test below stops it.
    for (width, height) in [(80, 20), (30, 20), (200, 50), (7, 3)] {
        let (columns, rows) = image_size_for(&square, width, height, true, IMAGE_ZOOM_NORMAL);
        assert!(columns >= width && rows >= height);
    }
}

#[test]
fn extreme_aspects_stay_cheap_to_draw() {
    const GREY: [u8; 4] = [128, 128, 128, 255];
    let (width, height) = (200, 50);
    // Both caps at once is the worst the sizing can produce.
    let budget = width * height * IMAGE_MAX_OVERFLOW * IMAGE_MAX_OVERFLOW;
    for (image_width, image_height) in [(1, 1024), (2, 1024), (4, 1024), (1024, 1), (1024, 2), (1024, 576)] {
        let image = solid(image_width, image_height, GREY);
        for fill in [false, true] {
            // Every rung of the zoom ladder, since zooming in is the one
            // thing that could put the blow-up back.
            for zoom in IMAGE_ZOOM_STEPS {
                let (columns, rows) = image_size_for(&image, width, height, fill, zoom);
                let (lines, colors, _) = image_to_ascii_on(&image, columns, rows, false, false);
                let cells = columns * lines.len();
                assert!(cells <= budget, "{image_width}x{image_height} fill={fill} zoom={zoom}: {columns}x{} is {cells} cells", lines.len());
                assert_eq!(colors.len(), lines.len());
            }
        }
    }

    // A photo is untouched by the cap: fill still covers the pane exactly.
    assert_eq!(image_size_for(&solid(1024, 576, GREY), width, height, true, IMAGE_ZOOM_NORMAL).0, width);
    assert_eq!(image_size_for(&solid(1024, 576, GREY), width, height, false, IMAGE_ZOOM_NORMAL).0, 177);
}

#[test]
fn cost_comes_from_the_header_not_the_file_size() {
    let dir = std::env::temp_dir().join(format!("fm84-image-cost-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let png = dir.join("picture.png");
    solid(600, 400, [1, 2, 3, 255]).save(&png).unwrap();
    assert_eq!(image_cost(&png).unwrap(), ((600, 400), 600 * 400 * 4));
    // The gap between the two is the whole point: this one is 164x.
    assert!(std::fs::metadata(&png).unwrap().len() * 100 < 600 * 400 * 4);

    let text = dir.join("notes.txt");
    std::fs::write(&text, "hello\n").unwrap();
    assert!(image_cost(&text).is_none());
    assert!(image_cost(&dir.join("absent.png")).is_none());

    // Where the prompt falls: a 24 MP photo opens straight away, while the
    // 12000x12000 picture that unpacks to 549 MiB is asked about first.
    const { assert!(6000 * 4000 * 4 < IMAGE_MAX_DECODED) };
    const { assert!(12000 * 12000 * 4 > IMAGE_MAX_DECODED) };

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn zoom_scales_the_fitted_size_and_stops_at_the_cap() {
    let (width, height) = (200, 50);
    let photo = solid(1024, 576, [128, 128, 128, 255]);
    let fitted = image_size_for(&photo, width, height, false, IMAGE_ZOOM_NORMAL).0;

    // Each rung scales the fitted width by its own percentage.
    for zoom in IMAGE_ZOOM_STEPS {
        let (columns, _) = image_size_for(&photo, width, height, false, zoom);
        let wanted = fitted * zoom as usize / 100;
        assert!(columns.abs_diff(wanted) <= 1, "zoom {zoom}%: {columns} columns, wanted about {wanted}");
    }

    // Zooming out shrinks it, zooming in overflows the pane and scrolls.
    assert!(image_size_for(&photo, width, height, false, 25).0 < fitted);
    assert!(image_size_for(&photo, width, height, false, 400).1 > height);

    // The ladder tops out where the drawing is capped, so the last rung is
    // the last one that changes anything.
    assert_eq!(*IMAGE_ZOOM_STEPS.last().unwrap() as usize, IMAGE_MAX_OVERFLOW * 100);
    assert!(IMAGE_ZOOM_STEPS.contains(&IMAGE_ZOOM_NORMAL));
    assert!(IMAGE_ZOOM_STEPS.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn x_cycles_image_text_hex_and_text_hex() {
    let dir = std::env::temp_dir().join(format!("fm84-view-modes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("picture.png");
    solid(3, 2, [255, 0, 0, 255]).save(&png).unwrap();
    let text = dir.join("notes.txt");
    std::fs::write(&text, "hello\n").unwrap();

    let mut state = load_file_content(&png, None).unwrap();
    let mut seen = vec![state.mode];
    for _ in 0..3 {
        state.mode = state.next_mode();
        seen.push(state.mode);
    }
    assert_eq!(seen, [ViewMode::Image, ViewMode::Text, ViewMode::Hex, ViewMode::Image]);

    let mut state = load_file_content(&text, None).unwrap();
    let mut seen = vec![state.mode];
    for _ in 0..2 {
        state.mode = state.next_mode();
        seen.push(state.mode);
    }
    assert_eq!(seen, [ViewMode::Text, ViewMode::Hex, ViewMode::Text]);

    std::fs::remove_dir_all(&dir).unwrap();
}
