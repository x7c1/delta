use super::{PromptPart, Submission};

/// Where one buffered byte came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Typed,
    /// Inside the paste with this sequence number.
    Paste(u64),
}

/// The fake TUI's input box: the pending bytes and where each came from.
#[derive(Default)]
pub(super) struct LineBuffer {
    bytes: Vec<u8>,
    origins: Vec<Origin>,
    /// The sequence number of the latest paste; its bytes are stamped with it.
    current_paste: u64,
}

impl LineBuffer {
    pub(super) fn push_typed(&mut self, byte: u8) {
        self.bytes.push(byte);
        self.origins.push(Origin::Typed);
    }

    /// A paste start marker arrived: the bytes that follow belong to a new
    /// paste, distinct from any earlier one.
    pub(super) fn start_paste(&mut self) {
        self.current_paste += 1;
    }

    pub(super) fn push_pasted(&mut self, byte: u8) {
        self.bytes.push(byte);
        self.origins.push(Origin::Paste(self.current_paste));
    }

    pub(super) fn pop(&mut self) {
        self.bytes.pop();
        self.origins.pop();
    }

    pub(super) fn clear(&mut self) {
        self.bytes.clear();
        self.origins.clear();
    }

    pub(super) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Empty the buffer into a [`Submission`], one part per run of bytes
    /// sharing an origin.
    pub(super) fn take(&mut self) -> Submission {
        let mut parts = Vec::new();
        let mut start = 0;
        while start < self.bytes.len() {
            let origin = self.origins[start];
            let len = self.origins[start..]
                .iter()
                .take_while(|&&o| o == origin)
                .count();
            parts.push(PromptPart {
                text: String::from_utf8_lossy(&self.bytes[start..start + len]).into_owned(),
                pasted: matches!(origin, Origin::Paste(_)),
            });
            start += len;
        }
        self.clear();
        Submission { parts }
    }
}
