//! F11's Animation: the cursor sliding from row to row, a directory's rows
//! coming up one after another as a CRT warms, popups opened with a
//! scanline wipe, and the rows a copy or a move made flashing once. Only
//! the timing is here; the frames are drawn by the UI, which keeps what it
//! last drew in `Motion`.

use super::*;

/// How long the cursor takes to slide to its row.
pub const SLIDE: Duration = Duration::from_millis(90);
/// The longest a directory takes to come up, however many rows it has.
pub const BOOT: Duration = Duration::from_millis(220);
/// The most a row waits on the one above it, for a short listing.
const BOOT_ROW: Duration = Duration::from_millis(12);
/// How long a popup takes to wipe open.
pub const WIPE: Duration = Duration::from_millis(140);
/// How long the rows a job made glow before they fade out.
pub const FLASH: Duration = Duration::from_millis(700);
/// How often the screen is drawn while anything moves: about 60 a second.
pub const FRAME: Duration = Duration::from_millis(16);
/// How often it is drawn otherwise, waiting on the keys.
pub const IDLE: Duration = Duration::from_millis(100);
/// The most rows a flash remembers - a copy of a whole tree names more
/// than a panel could show.
const FLASH_MOST: usize = 10_000;

/// How far along something is, from 0 to 1, `started` and lasting `length`.
pub fn progress(started: Instant, length: Duration, now: Instant) -> f64 {
    if length.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(started).as_secs_f64() / length.as_secs_f64()).min(1.0)
}

/// Fast at first and settling at the end, as a thing thrown comes to rest.
pub fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// The cursor on its way to a row.
#[derive(Clone, Copy, Debug)]
pub struct Slide {
    /// Where it set off from, part of the way between rows if it was moving.
    from: f64,
    to: u16,
    started: Instant,
}

impl Slide {
    /// The row it is on at `now`.
    pub fn at(&self, now: Instant) -> f64 {
        let t = ease_out(progress(self.started, SLIDE, now));
        self.from + (f64::from(self.to) - self.from) * t
    }

    fn done(&self, now: Instant) -> bool {
        progress(self.started, SLIDE, now) >= 1.0
    }
}

/// A listing coming up a row at a time.
#[derive(Clone, Copy, Debug)]
pub struct Boot {
    started: Instant,
    rows: usize,
}

impl Boot {
    /// How long each row waits on the one above it.
    fn step(&self) -> Duration {
        let share = BOOT / self.rows.max(1) as u32;
        share.min(BOOT_ROW)
    }

    /// How many rows are up at `now`, counted from the top.
    pub fn shown(&self, now: Instant) -> usize {
        let step = self.step().as_secs_f64();
        if step == 0.0 {
            return self.rows;
        }
        let elapsed = now.saturating_duration_since(self.started).as_secs_f64();
        ((elapsed / step) as usize + 1).min(self.rows)
    }

    fn done(&self, now: Instant) -> bool {
        self.shown(now) >= self.rows
    }
}

/// What the animations last drew: kept by the UI from frame to frame in
/// `Drawn`, as everything it draws is.
#[derive(Default, Debug)]
pub struct Motion {
    /// The row each side's cursor was last drawn on, counted from the top
    /// of its entries.
    cursor_rows: [Option<u16>; 2],
    slides: [Option<Slide>; 2],
    /// The directory each side last came up in, for a new one to boot.
    booted: [Option<PathBuf>; 2],
    boots: [Option<Boot>; 2],
    /// What the popup on show is, and when it began to wipe open.
    popup: Option<(PopupKind, Instant)>,
}

/// One popup told from another, for a new one to wipe in and the same one
/// drawn again - with what is typed into it, or a count going up - not to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PopupKind {
    Dialog(std::mem::Discriminant<Dialog>),
    Job,
    Error,
}

impl Motion {
    /// The row to draw `side`'s cursor on, now it is at `to`: on its way
    /// there from where it was last drawn, part of the way between two rows,
    /// or `to` itself - with nothing drawn before to slide from, as in a
    /// panel new to its directory.
    pub fn cursor_row(&mut self, side: Side, to: u16, now: Instant) -> f64 {
        let last = self.cursor_rows[side].replace(to);
        match (last, self.slides[side]) {
            // Moved again while sliding: off from wherever it has got to.
            (Some(last), Some(slide)) if last != to => {
                self.slides[side] = Some(Slide { from: slide.at(now), to, started: now });
            }
            (Some(last), None) if last != to => {
                self.slides[side] = Some(Slide { from: f64::from(last), to, started: now });
            }
            _ => {}
        }
        match self.slides[side] {
            Some(slide) if !slide.done(now) => slide.at(now),
            _ => {
                self.slides[side] = None;
                f64::from(to)
            }
        }
    }

    /// The cursor drawn somewhere it is not to slide from - another
    /// directory, a tree, a rename - so the next move starts afresh.
    pub fn forget_cursor(&mut self, side: Side) {
        self.cursor_rows[side] = None;
        self.slides[side] = None;
    }

    /// How many of `side`'s `rows` are up, the panel now in `dir`: all of
    /// them, unless it has just come to it and they are still coming up.
    pub fn booted_rows(&mut self, side: Side, dir: &Path, rows: usize, now: Instant) -> usize {
        if self.booted[side].as_deref() != Some(dir) {
            self.booted[side] = Some(dir.to_path_buf());
            self.boots[side] = (rows > 1).then_some(Boot { started: now, rows });
            self.forget_cursor(side);
        }
        match self.boots[side] {
            // The listing grew while coming up, as one still being read does.
            Some(mut boot) if !boot.done(now) => {
                boot.rows = boot.rows.max(rows);
                self.boots[side] = Some(boot);
                boot.shown(now).min(rows)
            }
            _ => {
                self.boots[side] = None;
                rows
            }
        }
    }

    /// How far open the popup on show is, from 0 to 1; a new one, of `kind`,
    /// starts from closed. None with none on show.
    pub fn popup_open(&mut self, kind: Option<PopupKind>, now: Instant) -> Option<f64> {
        let Some(kind) = kind else {
            self.popup = None;
            return None;
        };
        let started = match self.popup {
            Some((was, started)) if was == kind => started,
            _ => {
                self.popup = Some((kind, now));
                now
            }
        };
        Some(progress(started, WIPE, now))
    }

    /// Whether anything is still on its way, and wants the next frame soon.
    pub fn moving(&self, now: Instant) -> bool {
        self.slides.iter().flatten().any(|slide| !slide.done(now)) || self.boots.iter().flatten().any(|boot| !boot.done(now)) || self.popup.is_some_and(|(_, started)| progress(started, WIPE, now) < 1.0)
    }
}

/// The rows a copy or a move made, glowing for a moment once it is done.
pub struct Flash {
    pub paths: HashSet<PathBuf>,
    pub started: Instant,
}

impl Flash {
    /// How bright the glow is at `now`, from 1 to nothing; None once over.
    pub fn glow(&self, now: Instant) -> Option<f64> {
        let t = progress(self.started, FLASH, now);
        (t < 1.0).then(|| 1.0 - ease_out(t))
    }
}

impl AppState {
    /// The rows a finished job made, from what it did, to flash.
    pub(super) fn flash_done(&mut self, did: &[Done]) {
        if !self.options.animation {
            return;
        }
        let paths: HashSet<PathBuf> = did
            .iter()
            .filter_map(|done| match done {
                Done::Copied { to, .. } | Done::Moved { to, .. } => Some(to.clone()),
                Done::Trashed { .. } => None,
            })
            .take(FLASH_MOST)
            .collect();
        if !paths.is_empty() {
            self.flash = Some(Flash { paths, started: Instant::now() });
        }
    }

    /// How long to wait on the keys before drawing again: a frame's worth
    /// while anything moves, so it moves smoothly, and longer otherwise. A
    /// flash over is let go of here.
    pub fn frame_wait(&mut self) -> Duration {
        let now = Instant::now();
        if self.flash.as_ref().is_some_and(|flash| flash.glow(now).is_none()) {
            self.flash = None;
        }
        if self.options.animation && (self.flash.is_some() || self.drawn.motion.moving(now)) { FRAME } else { IDLE }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_slides_to_its_row_and_stays() {
        let mut motion = Motion::default();
        let start = Instant::now();
        assert_eq!(motion.cursor_row(Side::Left, 3, start), 3.0, "nowhere to slide from at first");
        let first = motion.cursor_row(Side::Left, 10, start);
        assert_eq!(first, 3.0, "sets off from where it was");
        let halfway = motion.cursor_row(Side::Left, 10, start + SLIDE / 2);
        assert!(halfway > 3.0 && halfway < 10.0, "between the two: {halfway}");
        assert_eq!(motion.cursor_row(Side::Left, 10, start + SLIDE), 10.0, "there once the slide is over");
        assert!(!motion.moving(start + SLIDE));
        // The other side has a cursor of its own.
        assert_eq!(motion.cursor_row(Side::Right, 5, start), 5.0);
    }

    #[test]
    fn a_move_while_sliding_sets_off_from_where_it_has_got_to() {
        let mut motion = Motion::default();
        let start = Instant::now();
        motion.cursor_row(Side::Left, 0, start);
        motion.cursor_row(Side::Left, 10, start);
        let got_to = motion.cursor_row(Side::Left, 10, start + SLIDE / 3);
        let turned = motion.cursor_row(Side::Left, 0, start + SLIDE / 3);
        assert!((turned - got_to).abs() < 1e-9, "no jump: {got_to} then {turned}");
        assert!(motion.moving(start + SLIDE / 3));
    }

    #[test]
    fn a_new_directory_comes_up_a_row_at_a_time() {
        let mut motion = Motion::default();
        let start = Instant::now();
        let dir = Path::new("/somewhere");
        assert_eq!(motion.booted_rows(Side::Left, dir, 40, start), 1, "one row up to begin with");
        let later = motion.booted_rows(Side::Left, dir, 40, start + BOOT / 2);
        assert!(later > 1 && later < 40, "some way down: {later}");
        assert_eq!(motion.booted_rows(Side::Left, dir, 40, start + BOOT), 40, "all of it in the time a boot takes");
        // The same directory again - read again, a file added - stays up.
        assert_eq!(motion.booted_rows(Side::Left, dir, 41, start + BOOT * 2), 41);
        // A short one is not dragged out to the whole of the time.
        assert_eq!(motion.booted_rows(Side::Left, Path::new("/short"), 3, start), 1);
        assert_eq!(motion.booted_rows(Side::Left, Path::new("/short"), 3, start + BOOT_ROW * 3), 3);
    }

    #[test]
    fn a_new_directory_forgets_where_the_cursor_was() {
        let mut motion = Motion::default();
        let start = Instant::now();
        motion.booted_rows(Side::Left, Path::new("/a"), 1, start);
        motion.cursor_row(Side::Left, 9, start);
        motion.booted_rows(Side::Left, Path::new("/b"), 1, start);
        assert_eq!(motion.cursor_row(Side::Left, 0, start), 0.0, "no slide from a row of another directory");
    }

    #[test]
    fn a_popup_wipes_open_once_and_not_again_for_the_same_one() {
        let mut motion = Motion::default();
        let start = Instant::now();
        assert_eq!(motion.popup_open(None, start), None);
        assert_eq!(motion.popup_open(Some(PopupKind::Job), start), Some(0.0));
        assert_eq!(motion.popup_open(Some(PopupKind::Job), start + WIPE), Some(1.0));
        assert_eq!(motion.popup_open(Some(PopupKind::Job), start + WIPE * 3), Some(1.0), "drawn again, still open");
        assert_eq!(motion.popup_open(Some(PopupKind::Error), start + WIPE * 3), Some(0.0), "another opens afresh");
        motion.popup_open(None, start + WIPE * 4);
        assert_eq!(motion.popup_open(Some(PopupKind::Error), start + WIPE * 4), Some(0.0), "and the same, once closed");
    }

    #[test]
    fn a_flash_fades_to_nothing() {
        let start = Instant::now();
        let flash = Flash { paths: HashSet::new(), started: start };
        assert_eq!(flash.glow(start), Some(1.0));
        let mid = flash.glow(start + FLASH / 2).unwrap();
        assert!(mid > 0.0 && mid < 1.0);
        assert_eq!(flash.glow(start + FLASH), None);
    }

    #[test]
    fn frames_come_quickly_only_while_something_moves() {
        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.flash = Some(Flash { paths: HashSet::new(), started: Instant::now() });
        assert_eq!(app_state.frame_wait(), IDLE, "not with the option off");
        app_state.options.animation = true;
        assert_eq!(app_state.frame_wait(), FRAME);
        app_state.flash.as_mut().unwrap().started -= FLASH;
        assert_eq!(app_state.frame_wait(), IDLE);
        assert!(app_state.flash.is_none(), "let go of once over");
    }

    #[test]
    fn a_copy_flashes_what_it_made_and_the_trash_does_not() {
        let mut app_state = AppState::new();
        app_state.options = Options::default();
        let mark = Mark::of(&std::env::temp_dir(), false).expect("the temporary directory is there");
        let made = vec![Done::Copied { to: PathBuf::from("/x/a"), mark }, Done::Trashed { from: PathBuf::from("/x/b"), since: 0 }];
        app_state.flash_done(&made);
        assert!(app_state.flash.is_none(), "off, no flash");
        app_state.options.animation = true;
        app_state.flash_done(&made);
        let flash = app_state.flash.as_ref().expect("on, a flash");
        assert_eq!(flash.paths, HashSet::from([PathBuf::from("/x/a")]));
        app_state.flash = None;
        app_state.flash_done(&made[1..]);
        assert!(app_state.flash.is_none(), "nothing made, nothing to flash");
    }
}
