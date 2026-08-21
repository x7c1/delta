//! The approval loops, browser → server → `fake-codex`: the allow / deny /
//! allow-for-session matrix over both approval kinds, a parallel fan-out of
//! approvals gating one turn, and the file-change detail a card is built from.

mod decision_matrix;
mod file_change_detail;
mod parallel_approvals;
