//! Repository-tab use cases: aggregate the session history into recency-ordered
//! repositories, each bundling one or more local clones, for the new-session
//! Repository tab — plus the two commands that maintain that set: registering
//! the clone roots it probes, and cloning a repository into one of them.

mod clone_repository;
mod clone_roots;
mod list_repositories;
mod scan;

pub(in crate::interactor) use clone_repository::CloneJobs;

#[cfg(test)]
mod tests;
