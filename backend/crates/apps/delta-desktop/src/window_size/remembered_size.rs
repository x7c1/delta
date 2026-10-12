use std::sync::{Mutex, PoisonError};

use tauri::LogicalSize;

use super::{is_usable, SavedSize, WindowMode};

/// The window's last normal content size, in logical pixels, with the screen
/// it had that size on: what [`save`](fn@super::save) writes as the app
/// exits. Managed as app state.
#[derive(Debug, Default)]
pub struct RememberedSize(Mutex<Option<SavedSize>>);

impl RememberedSize {
    /// Record the window's content size `size` while it is in `mode`, on a
    /// monitor whose work area is `screen` logical pixels (`None` when it
    /// cannot be read). Only a normal window's size is kept, so maximizing,
    /// minimizing or going fullscreen keeps the size it had before; an empty
    /// size is ignored.
    pub fn observe(
        &self,
        size: LogicalSize<f64>,
        mode: WindowMode,
        screen: Option<LogicalSize<f64>>,
    ) {
        if mode != WindowMode::Normal || !is_usable(size) {
            return;
        }
        let saved = SavedSize {
            width: size.width,
            height: size.height,
            screen: screen.filter(|screen| is_usable(*screen)),
        };
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(saved);
    }

    /// The last normal size and its screen, if the window has had one.
    pub fn last_normal(&self) -> Option<SavedSize> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{saved_on, size};
    use super::super::{read_remembered, write_remembered, STATE_FILE_NAME};
    use super::*;

    #[test]
    fn a_maximized_or_minimized_window_does_not_overwrite_the_last_normal_size() {
        let remembered = RememberedSize::default();
        let screen = Some(size(2560.0, 1440.0));
        remembered.observe(size(1400.0, 900.0), WindowMode::Normal, screen);
        remembered.observe(size(2560.0, 1400.0), WindowMode::Maximized, screen);
        remembered.observe(size(1.0, 1.0), WindowMode::Minimized, screen);
        remembered.observe(size(2560.0, 1440.0), WindowMode::Fullscreen, screen);
        remembered.observe(size(0.0, 0.0), WindowMode::Normal, screen);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STATE_FILE_NAME);
        write_remembered(&path, remembered.last_normal().unwrap()).unwrap();
        assert_eq!(
            read_remembered(&path),
            Some(saved_on(1400.0, 900.0, size(2560.0, 1440.0)))
        );
    }

    #[test]
    fn a_normal_resize_replaces_the_last_normal_size() {
        let remembered = RememberedSize::default();
        assert_eq!(remembered.last_normal(), None);
        remembered.observe(size(1400.0, 900.0), WindowMode::Normal, None);
        remembered.observe(
            size(1500.0, 950.0),
            WindowMode::Normal,
            Some(size(1920.0, 1080.0)),
        );
        assert_eq!(
            remembered.last_normal(),
            Some(saved_on(1500.0, 950.0, size(1920.0, 1080.0)))
        );
    }

    #[test]
    fn an_unusable_screen_is_remembered_as_unknown() {
        let remembered = RememberedSize::default();
        remembered.observe(
            size(1400.0, 900.0),
            WindowMode::Normal,
            Some(size(0.0, 0.0)),
        );
        assert_eq!(remembered.last_normal().unwrap().screen, None);
    }
}
