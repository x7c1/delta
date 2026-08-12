//! PR-tab use cases: pull PR search results through the `gh` driver
//! and stamp each row with whether Delta has a local clone of the PR's
//! repository registered.

mod list_pull_requests;

#[cfg(test)]
mod tests;
