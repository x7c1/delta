//! Which way one recorded agent frame travelled.

/// Which way one recorded frame travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommsDirection {
    /// Delta → agent: a frame Delta wrote.
    ToAgent,
    /// Agent → Delta: a frame Delta read.
    FromAgent,
}
