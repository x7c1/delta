//! Repository-tab use cases: aggregate the session history into recency-ordered
//! repositories, each bundling one or more local clones, for the new-session
//! Repository tab.

mod clone_roots;
mod list_repositories;
mod scan;

#[cfg(test)]
mod tests;
