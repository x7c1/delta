use tauri::LogicalSize;

use super::{SavedSize, WorkArea, MIN_SIZE};

/// What a remembered content size cut to a smaller monitor's work area leaves
/// free of it, per side, in logical pixels: room for GNOME's header bar and
/// its shadow (about 40 and 50 logical pixels) and for the desktop's own bars.
/// GTK on Wayland reports the whole monitor as the work area, so GNOME's top
/// bar (about 32) and a dock are not taken off it.
#[cfg(not(target_os = "macos"))]
const SCREEN_MARGIN: LogicalSize<f64> = LogicalSize {
    width: 64.0,
    height: 128.0,
};

/// None on macOS: the work area already leaves out the menu bar and the Dock,
/// and the title bar is drawn over the content (see `macos_title_bar`), so a
/// window filling the work area is restored at that size rather than shrunk.
#[cfg(target_os = "macos")]
const SCREEN_MARGIN: LogicalSize<f64> = LogicalSize {
    width: 0.0,
    height: 0.0,
};

/// The remembered content size `saved` (logical pixels) fitted to the monitor
/// the window opens on, whose work area is converted to logical pixels at the
/// monitor's own scale. Each side is fitted on its own:
///
/// - where the work area is at least as large as the screen the size was made
///   on, the side is restored exactly, so a window the user stretched across
///   this screen comes back the same;
/// - where it is smaller, or the size's screen is not known, the side is cut
///   to the work area less [`SCREEN_MARGIN`].
///
/// Never below [`MIN_SIZE`], which wins on a screen too small for both.
/// Without a monitor only the minimum applies.
pub fn fit_to_screen(saved: SavedSize, work_area: Option<WorkArea>) -> LogicalSize<f64> {
    let fit = |side: f64, available: Option<f64>, minimum: f64| {
        let side = available.map_or(side, |available| side.min(available));
        side.round().max(minimum)
    };
    // The room for a side: none to cut when this screen is at least as large
    // on that side as the one the size was made on.
    let room = |area: f64, screen: Option<f64>, margin: f64| {
        let as_large = screen.is_some_and(|screen| area >= screen);
        (!as_large).then_some(area - margin)
    };
    let area = work_area.map(WorkArea::logical);
    LogicalSize {
        width: fit(
            saved.width,
            area.and_then(|area| {
                room(
                    area.width,
                    saved.screen.map(|screen| screen.width),
                    SCREEN_MARGIN.width,
                )
            }),
            MIN_SIZE.width,
        ),
        height: fit(
            saved.height,
            area.and_then(|area| {
                room(
                    area.height,
                    saved.screen.map(|screen| screen.height),
                    SCREEN_MARGIN.height,
                )
            }),
            MIN_SIZE.height,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{saved, saved_on, size, work_area};
    use super::*;

    #[test]
    fn a_size_larger_than_the_monitor_is_reduced_to_fit_it_with_the_margin() {
        // 5120×2160 at scale 2 is 2560×1080 logical.
        assert_eq!(
            fit_to_screen(saved(3044.0, 3852.0), work_area(5120, 2160, 2.0)),
            size(2560.0 - SCREEN_MARGIN.width, 1080.0 - SCREEN_MARGIN.height)
        );
    }

    #[test]
    fn a_size_within_the_monitor_is_kept() {
        assert_eq!(
            fit_to_screen(saved(1600.0, 900.0), work_area(2560, 1440, 1.0)),
            size(1600.0, 900.0)
        );
    }

    #[test]
    fn each_side_is_fitted_on_its_own() {
        let area = work_area(1920, 1080, 1.0);
        assert_eq!(
            fit_to_screen(saved(3000.0, 700.0), area),
            size(1920.0 - SCREEN_MARGIN.width, 700.0)
        );
        assert_eq!(
            fit_to_screen(saved(1200.0, 3000.0), area),
            size(1200.0, 1080.0 - SCREEN_MARGIN.height)
        );
    }

    #[test]
    fn a_fitted_size_never_goes_below_the_minimum() {
        assert_eq!(
            fit_to_screen(saved(1600.0, 1000.0), work_area(800, 600, 1.0)),
            MIN_SIZE
        );
        assert_eq!(fit_to_screen(saved(500.0, 300.0), None), MIN_SIZE);
        assert_eq!(
            fit_to_screen(saved(500.0, 300.0), work_area(2560, 1440, 1.0)),
            MIN_SIZE
        );
        assert_eq!(
            fit_to_screen(
                saved_on(500.0, 300.0, size(2560.0, 1440.0)),
                work_area(2560, 1440, 1.0)
            ),
            MIN_SIZE
        );
    }

    #[test]
    fn a_size_saved_at_another_scale_is_fitted_in_the_new_monitors_logical_pixels() {
        // Saved on a 2560×1440 screen at scale 1, opened on a 2560×1600 one at
        // scale 2: the new work area is 1280×800 logical, not 2560×1600.
        assert_eq!(
            fit_to_screen(
                saved_on(2400.0, 1300.0, size(2560.0, 1440.0)),
                work_area(2560, 1600, 2.0)
            ),
            size(1280.0 - SCREEN_MARGIN.width, 800.0 - SCREEN_MARGIN.height)
        );
        // The other way round, a size saved at scale 2 keeps its logical size
        // on a scale-1 screen with room for it.
        assert_eq!(
            fit_to_screen(
                saved_on(1200.0, 700.0, size(1280.0, 800.0)),
                work_area(2560, 1440, 1.0)
            ),
            size(1200.0, 700.0)
        );
    }

    #[test]
    fn a_size_made_on_the_same_screen_is_restored_exactly() {
        // A window stretched to the full height of a 4096×1728 logical screen
        // keeps that height, with no margin taken off.
        let screen = size(4096.0, 1728.0);
        assert_eq!(
            fit_to_screen(saved_on(1366.0, 1659.0, screen), work_area(4096, 1728, 1.0)),
            size(1366.0, 1659.0)
        );
        // Filling the screen entirely, too.
        assert_eq!(
            fit_to_screen(saved_on(4096.0, 1728.0, screen), work_area(4096, 1728, 1.0)),
            screen
        );
    }

    #[test]
    fn a_size_made_on_a_smaller_screen_is_restored_exactly() {
        assert_eq!(
            fit_to_screen(
                saved_on(1366.0, 1659.0, size(4096.0, 1728.0)),
                work_area(5120, 2160, 1.0)
            ),
            size(1366.0, 1659.0)
        );
    }

    #[test]
    fn a_size_made_on_a_larger_screen_is_cut_with_the_margin() {
        assert_eq!(
            fit_to_screen(
                saved_on(1366.0, 1659.0, size(4096.0, 1728.0)),
                work_area(2560, 1440, 1.0)
            ),
            size(1366.0, 1440.0 - SCREEN_MARGIN.height)
        );
    }

    #[test]
    fn only_the_side_the_screen_is_smaller_on_is_cut() {
        // Made on a 1920×1200 screen, opened on a wider but shorter one: the
        // width, which this screen has room for, is kept to the pixel; the
        // height is cut with the margin.
        assert_eq!(
            fit_to_screen(
                saved_on(1920.0, 1200.0, size(1920.0, 1200.0)),
                work_area(2560, 1080, 1.0)
            ),
            size(1920.0, 1080.0 - SCREEN_MARGIN.height)
        );
    }

    #[test]
    fn a_size_from_an_unknown_screen_is_cut_with_the_margin() {
        // A file written without the screen: fitted as from a larger screen.
        assert_eq!(
            fit_to_screen(saved(1366.0, 1728.0), work_area(4096, 1728, 1.0)),
            size(1366.0, 1728.0 - SCREEN_MARGIN.height)
        );
    }

    #[test]
    fn a_fitted_size_is_rounded_to_whole_logical_pixels() {
        // 2560×1440 at scale 1.5 is 1706.67×960 logical.
        assert_eq!(
            fit_to_screen(saved(4000.0, 900.4), work_area(2560, 1440, 1.5)),
            size(
                (2560.0 / 1.5 - SCREEN_MARGIN.width).round(),
                (900.4_f64).min(960.0 - SCREEN_MARGIN.height).round()
            )
        );
    }

    #[test]
    fn without_a_monitor_a_saved_size_is_kept() {
        assert_eq!(
            fit_to_screen(saved(3000.0, 2000.0), None),
            size(3000.0, 2000.0)
        );
        assert_eq!(
            fit_to_screen(saved_on(3000.0, 2000.0, size(3840.0, 2160.0)), None),
            size(3000.0, 2000.0)
        );
    }
}
