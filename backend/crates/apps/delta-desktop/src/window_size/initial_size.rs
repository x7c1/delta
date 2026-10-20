use tauri::LogicalSize;

use super::{FALLBACK_SIZE, MIN_SIZE};

/// The share of the monitor's work area the first window takes, per side.
const WORK_AREA_SHARE: f64 = 0.8;

/// The widest the first window gets for its height: 16:9. A 16:9 or narrower
/// screen is not affected; on an ultrawide one, 80 % of the width would open a
/// window far wider than the conversation and the terminal can use.
const MAX_ASPECT_RATIO: f64 = 16.0 / 9.0;

/// The first window's size for a monitor whose work area is `work_area`
/// logical pixels: 80 % of it per side, no wider than [`MAX_ASPECT_RATIO`]
/// of its height, and never below [`MIN_SIZE`]. Without a monitor,
/// [`FALLBACK_SIZE`].
pub fn initial_size(work_area: Option<LogicalSize<f64>>) -> LogicalSize<f64> {
    match work_area {
        Some(area) => {
            let height = (area.height * WORK_AREA_SHARE).round().max(MIN_SIZE.height);
            let width = (area.width * WORK_AREA_SHARE)
                .min(height * MAX_ASPECT_RATIO)
                .round()
                .max(MIN_SIZE.width);
            LogicalSize { width, height }
        }
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
    fn an_ultrawide_work_area_is_narrowed_to_16_by_9() {
        assert_eq!(
            initial_size(Some(size(3440.0, 1400.0))),
            size(1991.0, 1120.0)
        );
    }

    #[test]
    fn a_short_ultrawide_work_area_is_narrowed_to_16_by_9_of_the_minimum_height() {
        assert_eq!(initial_size(Some(size(2560.0, 700.0))), size(1138.0, 640.0));
    }

    #[test]
    fn a_16_by_10_work_area_keeps_80_percent_of_its_width() {
        assert_eq!(
            initial_size(Some(size(2560.0, 1600.0))),
            size(2048.0, 1280.0)
        );
    }

    #[test]
    fn without_a_monitor_the_window_keeps_the_fallback_size() {
        assert_eq!(initial_size(None), FALLBACK_SIZE);
    }
}
