use std::path::{Path, PathBuf};

use delta_bootstrap::Config;

use super::ExplicitPaths;

/// The database file's name inside the app data directory.
const DATABASE_FILE: &str = "delta.db";

/// The per-spawn working-directory base's name inside the app data directory.
const SESSIONS_DIR: &str = "sessions";

/// `config` with its defaulted database path and session working directory
/// moved under `data_dir`, and the directories that placement needs created
/// before the server opens them.
pub struct Placement {
    pub config: Config,
    pub dirs_to_create: Vec<PathBuf>,
}

/// Move the settings `explicit` does not cover under `data_dir`.
pub fn place_in_app_data_dir(
    mut config: Config,
    data_dir: &Path,
    explicit: ExplicitPaths,
) -> Placement {
    let mut dirs_to_create = Vec::new();
    if !explicit.database_path {
        config.database_path = path_string(&data_dir.join(DATABASE_FILE));
        dirs_to_create.push(data_dir.to_path_buf());
    }
    if !explicit.session_workdir {
        let sessions = data_dir.join(SESSIONS_DIR);
        config.session_workdir_base = path_string(&sessions);
        dirs_to_create.push(sessions);
    }
    Placement {
        config,
        dirs_to_create,
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsString;

    fn base_config() -> Config {
        let vars = [
            ("HOME", "/home/u"),
            ("DELTA_AUTH_TOKEN", "tok"),
            ("DELTA_HOOK_SECRET", "hs"),
        ];
        delta_server::config::config_from_vars(|name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        })
    }

    #[test]
    fn defaults_move_under_the_data_dir() {
        let data_dir = Path::new("/data/io.github.x7c1.delta");
        let placement = place_in_app_data_dir(base_config(), data_dir, ExplicitPaths::default());
        assert_eq!(
            placement.config.database_path,
            "/data/io.github.x7c1.delta/delta.db"
        );
        assert_eq!(
            placement.config.session_workdir_base,
            "/data/io.github.x7c1.delta/sessions"
        );
        assert_eq!(
            placement.dirs_to_create,
            [data_dir.to_path_buf(), data_dir.join("sessions")]
        );
    }

    #[test]
    fn explicit_settings_win() {
        let mut config = base_config();
        config.database_path = "/elsewhere/d.db".to_owned();
        config.session_workdir_base = "/elsewhere/s".to_owned();
        let placement = place_in_app_data_dir(
            config,
            Path::new("/data"),
            ExplicitPaths {
                database_path: true,
                session_workdir: true,
            },
        );
        assert_eq!(placement.config.database_path, "/elsewhere/d.db");
        assert_eq!(placement.config.session_workdir_base, "/elsewhere/s");
        assert!(placement.dirs_to_create.is_empty());
    }

    #[test]
    fn only_the_unset_one_moves() {
        let mut config = base_config();
        config.database_path = "/elsewhere/d.db".to_owned();
        let placement = place_in_app_data_dir(
            config,
            Path::new("/data"),
            ExplicitPaths {
                database_path: true,
                session_workdir: false,
            },
        );
        assert_eq!(placement.config.database_path, "/elsewhere/d.db");
        assert_eq!(placement.config.session_workdir_base, "/data/sessions");
        assert_eq!(placement.dirs_to_create, [PathBuf::from("/data/sessions")]);
    }

    #[test]
    fn home_based_defaults_are_kept() {
        let placement =
            place_in_app_data_dir(base_config(), Path::new("/data"), ExplicitPaths::default());
        assert_eq!(placement.config.worktree_base, "/home/u/.delta/worktrees");
        assert_eq!(placement.config.transcript_root, "/home/u/.claude/projects");
    }
}
