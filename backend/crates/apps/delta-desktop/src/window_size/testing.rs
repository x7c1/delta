//! Helpers shared by the tests of `window_size`'s modules.

use tauri::{LogicalSize, PhysicalSize};

use super::{SavedSize, WorkArea};

pub fn size(width: f64, height: f64) -> LogicalSize<f64> {
    LogicalSize { width, height }
}

/// A remembered size whose screen is not known.
pub fn saved(width: f64, height: f64) -> SavedSize {
    SavedSize {
        width,
        height,
        screen: None,
    }
}

/// A remembered size made on a screen whose work area is `screen`.
pub fn saved_on(width: f64, height: f64, screen: LogicalSize<f64>) -> SavedSize {
    SavedSize {
        width,
        height,
        screen: Some(screen),
    }
}

pub fn work_area(width: u32, height: u32, scale_factor: f64) -> Option<WorkArea> {
    Some(WorkArea {
        size: PhysicalSize { width, height },
        scale_factor,
    })
}
