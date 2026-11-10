//! F11's night drive: left alone a few minutes on the panels, fm84 fills
//! the screen with the Outrun grid, and the next key or touch of the mouse
//! brings the panels back as they were - taking nothing else from it.

use super::*;

impl AppState {
    /// Start the drive, once nothing has come in for as many minutes as F11
    /// says - none, and it never does - and only over the panels with
    /// nothing open on them: a job, a question or a file being read is not
    /// something left alone.
    pub fn poll_night_drive(&mut self) {
        let minutes = self.options.night_drive_minutes;
        if self.night_drive.is_none() && minutes > 0 && self.last_input.elapsed() >= Duration::from_secs(minutes * 60) && self.dialog.is_none() && !self.is_modal_open() {
            self.night_drive = Some(Instant::now());
        }
    }

    /// Something has come in. True when it was only waking fm84 from the
    /// drive, and is to go no further.
    pub fn wake(&mut self) -> bool {
        self.last_input = Instant::now();
        self.night_drive.take().is_some()
    }
}
