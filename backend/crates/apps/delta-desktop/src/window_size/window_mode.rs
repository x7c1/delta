/// Whether the window shows at a size of its own, the one to remember.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMode {
    Normal,
    Maximized,
    Minimized,
    Fullscreen,
}
