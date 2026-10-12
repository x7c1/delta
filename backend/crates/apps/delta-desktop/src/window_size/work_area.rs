use tauri::{LogicalSize, Monitor, PhysicalSize, Runtime, WebviewWindow};

/// A monitor's work area (the screen less the menu bar, the Dock or the
/// panels), as tao reports it: in physical pixels, with the monitor's scale
/// factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkArea {
    pub size: PhysicalSize<u32>,
    pub scale_factor: f64,
}

impl WorkArea {
    /// The work area of the monitor `window` is on, or of the primary one;
    /// `None` when neither can be read.
    pub(super) fn of_window<R: Runtime>(window: &WebviewWindow<R>) -> Option<Self> {
        let current = window.current_monitor().unwrap_or_else(|err| {
            tracing::warn!("could not read the window's monitor: {err}");
            None
        });
        let monitor = current.or_else(|| {
            window.primary_monitor().unwrap_or_else(|err| {
                tracing::warn!("could not read the primary monitor: {err}");
                None
            })
        })?;
        Some(Self::of(&monitor))
    }

    fn of(monitor: &Monitor) -> Self {
        Self {
            size: monitor.work_area().size,
            scale_factor: monitor.scale_factor(),
        }
    }

    /// The work area in the monitor's logical pixels, the unit the window's
    /// size is kept and set in.
    pub(super) fn logical(self) -> LogicalSize<f64> {
        self.size.to_logical(self.scale_factor)
    }
}
