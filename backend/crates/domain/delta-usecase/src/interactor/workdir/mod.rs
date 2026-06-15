//! Working-directory use cases for the directory picker.

mod browse_workdir;
mod git_repo;
mod home_dir;
mod recent_workdirs;

#[cfg(test)]
mod tests;

pub(in crate::interactor::workdir) use home_dir::home_dir;

/// How many recently-used working directories the picker's "recent" list returns.
const RECENT_WORKDIRS_LIMIT: u32 = 20;
