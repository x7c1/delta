use tauri::LogicalSize;

use super::{FALLBACK_SIZE, MIN_SIZE};

/// The share of the monitor's work area the first window takes, per side.
const WORK_AREA_SHARE: f64 = 0.8;

/// The widest the first window gets, in logical pixels: the navigator's 288px
/// (`w-72`) and the terminal's default 384px leave the conversation about
/// 930px. The conversation has no maximum width of its own, so on a large or
/// ultrawide screen 80 % of the width would only stretch its lines. A screen
/// whose 80 % is narrower (1920 wide or less) is not affected.
const MAX_WIDTH: f64 = 1600.0;

/// The first window's size for a monitor whose work area is `work_area`
/// logical pixels: 80 % of it per side, no wider than [`MAX_WIDTH`], and never
/// below [`MIN_SIZE`]. Without a monitor, [`FALLBACK_SIZE`].
pub fn initial_size(work_area: Option<LogicalSize<f64>>) -> LogicalSize<f64> {
    match work_area {
        Some(area) => LogicalSize {
            width: (area.width * WORK_AREA_SHARE)
                .min(MAX_WIDTH)
                .round()
                .max(MIN_SIZE.width),
            height: (area.height * WORK_AREA_SHARE).round().max(MIN_SIZE.height),
        },
        None => FALLBACK_SIZE,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::size;
    use super::*;

    #[test]
    fn a_full_hd_work_area_gives_80_percent_of_it() {
        assert_eq!(
            initial_size(Some(size(1920.0, 1080.0))),
            size(1536.0, 864.0)
        );
    }

    #[test]
    fn the_size_is_rounded_to_whole_logical_pixels() {
        assert_eq!(initial_size(Some(size(1512.0, 945.0))), size(1210.0, 756.0));
    }

    #[test]
    fn a_tiny_work_area_is_clamped_to_the_minimum() {
        assert_eq!(initial_size(Some(size(800.0, 600.0))), MIN_SIZE);
    }

    #[test]
    fn each_side_is_clamped_on_its_own() {
        assert_eq!(
            initial_size(Some(size(1100.0, 1000.0))),
            size(1024.0, 800.0)
        );
    }

    #[test]
    fn an_ultrawide_work_area_is_narrowed_to_the_maximum_width() {
        assert_eq!(
            initial_size(Some(size(3440.0, 1400.0))),
            size(1600.0, 1120.0)
        );
    }

    #[test]
    fn a_large_work_area_keeps_80_percent_of_its_height() {
        assert_eq!(
            initial_size(Some(size(3840.0, 2100.0))),
            size(1600.0, 1680.0)
        );
    }

    #[test]
    fn without_a_monitor_the_window_keeps_the_fallback_size() {
        assert_eq!(initial_size(None), FALLBACK_SIZE);
    }
}
