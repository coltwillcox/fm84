//! Deleting for good, with feeling, when F11 says so: the rows of what is
//! going burst into pixels for a moment before the delete takes them away.
//! Never for the trash, which is the safe way, and quietly so.

use super::*;

/// How long the rows take to burst.
pub const EXPLOSION: Duration = Duration::from_millis(600);

/// Rows bursting, and the delete waiting on them.
pub struct Explosion {
    pub side: Side,
    /// The rows, by the names the panel lists them under.
    pub names: HashSet<OsString>,
    pub started: Instant,
    items: Vec<(PathBuf, bool)>,
}

impl Explosion {
    /// How far the burst has got, from 0 to 1.
    pub fn progress(&self) -> f64 {
        (self.started.elapsed().as_secs_f64() / EXPLOSION.as_secs_f64()).min(1.0)
    }
}

impl AppState {
    /// A delete for good of `items`: the rows of those the active panel shows
    /// burst first, and the delete starts once they have.
    pub fn explode_then_delete(&mut self, items: Vec<(PathBuf, bool)>) {
        let side = self.active_side;
        let panel = self.panel(side);
        let names: HashSet<OsString> = panel.children.iter().filter(|item| items.iter().any(|(path, _)| *path == item.path_in(&panel.dir))).map(|item| item.name_os.clone()).collect();
        // Nothing on screen to burst: straight on with the delete.
        if names.is_empty() {
            return self.start_delete(items);
        }
        self.explosion = Some(Explosion { side, names, started: Instant::now(), items });
    }

    /// Once the rows have burst, the delete they were bursting for.
    pub fn poll_explosion(&mut self) {
        if self.explosion.as_ref().is_some_and(|explosion| explosion.started.elapsed() >= EXPLOSION)
            && let Some(explosion) = self.explosion.take()
        {
            self.start_delete(explosion.items);
        }
    }
}

/// A row's cell of `width`, `progress` of the way through bursting: solid
/// at first, then breaking into pixels that thin out to nothing. `seed`
/// keeps each cell and row its own pattern, `frame` moves it along.
pub fn blast(width: usize, progress: f64, seed: u64, frame: u64) -> Vec<(char, u8)> {
    const PIXELS: [char; 6] = ['█', '▓', '▒', '░', '*', '·'];
    (0..width as u64)
        .map(|x| {
            // A cheap hash, as good as a random number for this.
            let mut hash = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ x.wrapping_mul(0xBF58_476D_1CE4_E5B9) ^ frame.wrapping_mul(0x94D0_49BB_1331_11EB);
            hash ^= hash >> 31;
            hash = hash.wrapping_mul(0xD6E8_FEB8_6659_FD93);
            hash ^= hash >> 29;
            let chance = (hash % 1000) as f64 / 1000.0;
            if chance > 1.0 - progress * 0.95 {
                return (' ', 0);
            }
            // The further on, the finer the pixels.
            let fine = ((progress + chance * 0.3) * PIXELS.len() as f64) as usize;
            (PIXELS[fine.min(PIXELS.len() - 1)], (hash >> 40) as u8 % 3)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_bursts_from_solid_to_nothing() {
        let solid = blast(40, 0.0, 7, 0);
        assert!(solid.iter().all(|(glyph, _)| *glyph != ' '), "whole to begin with");
        let late = blast(40, 0.95, 7, 5);
        let left = late.iter().filter(|(glyph, _)| *glyph != ' ').count();
        assert!(left < 10, "mostly gone at the end: {left} left");
        assert_ne!(blast(40, 0.5, 7, 1), blast(40, 0.5, 8, 1), "each row its own");
    }

    #[test]
    fn the_delete_waits_for_the_burst_and_the_trash_never_bursts() {
        let dir = std::env::temp_dir().join(format!("fm84-explosion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("doomed.txt"), "x").unwrap();
        let mut app_state = AppState::new();
        app_state.options = Options::default();
        app_state.active_side = Side::Left;
        app_state.open_dir(Side::Left, dir.clone(), None);

        app_state.explode_then_delete(vec![(dir.join("doomed.txt"), false)]);
        assert!(app_state.explosion.is_some());
        app_state.poll_explosion();
        assert!(app_state.workers.job.is_none(), "not until it has burst");
        assert!(dir.join("doomed.txt").exists());
        if let Some(explosion) = &mut app_state.explosion {
            explosion.started -= EXPLOSION;
        }
        app_state.poll_explosion();
        assert!(app_state.explosion.is_none());
        let started = Instant::now();
        while app_state.workers.job.is_some() {
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(5));
            app_state.poll_transfer();
        }
        assert!(!dir.join("doomed.txt").exists());

        // Not on screen: nothing to burst, and straight on.
        std::fs::write(dir.join("elsewhere.txt"), "x").unwrap();
        app_state.open_dir(Side::Left, std::env::temp_dir(), None);
        app_state.explode_then_delete(vec![(dir.join("elsewhere.txt"), false)]);
        assert!(app_state.explosion.is_none() && app_state.workers.job.is_some());
        while app_state.workers.job.is_some() {
            std::thread::sleep(Duration::from_millis(5));
            app_state.poll_transfer();
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
