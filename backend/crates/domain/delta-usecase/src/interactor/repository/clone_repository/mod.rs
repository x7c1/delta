//! `clone_repository`: fetch a repository the user has no local clone of into
//! one of their registered clone roots.
//!
//! The PR tab lists pull requests from every repository the account can see,
//! including ones that exist nowhere on this machine. Those rows used to be a
//! dead end; this use case is the unblock — Delta already knows `gh` is
//! authenticated (the PR tab is gated on it) and the clone roots already say
//! where clones belong, so "clone it for me" needs no new information.
//!
//! The shape of the operation:
//!
//! 1. `clone_root` must be one of the registered roots. An unregistered
//!    directory is refused ([`Error::CloneRootNotRegistered`]) before anything
//!    happens: Delta writes clones only where the user said clones go.
//! 2. The destination is exactly `<clone_root>/<repo_name>` — no fallback
//!    naming. An existing path there is refused
//!    ([`Error::CloneDestinationExists`]) rather than merged into or renamed
//!    around.
//! 3. The clone itself is assembled in a **temporary sibling** directory inside
//!    the same root and renamed onto the destination on success. A rename within
//!    one directory is atomic, so the destination never exists half-cloned: it
//!    either is not there or is a finished clone. On failure the temporary
//!    directory is removed.
//! 4. The work runs on a spawned task and announces itself on the event seam,
//!    because a clone takes far longer than a request should. The caller gets
//!    `Ok(())` (the transport answers `202`) as soon as the job is claimed.
//!
//! The job registry ([`CloneJobs`]) is in-memory and keyed by destination path.
//! It exists for one reason: a second request for a destination that is already
//! being cloned **joins** the running job instead of starting a second `gh`
//! process on the same directory. It is deliberately not persisted — a server
//! restart forgets in-flight jobs, and the stale temporary directory such a
//! death leaves behind is removed when the next job for that destination starts.
//!
//! Nothing here touches a session: no lock, no registry, no actor. A clone
//! running does not delay starting a session, and starting a session does not
//! delay a clone.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::{Error, Result};
use crate::interactor::InteractorCore;
use crate::ports::{
    AsyncEventSink, GhCli, GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript,
    Workspace,
};

/// Prefix of the temporary directory a clone is assembled in before being
/// renamed onto its destination.
///
/// A sibling *inside the clone root* rather than a system temp dir, and
/// deterministic rather than randomised, for two reasons: the rename onto the
/// destination has to stay within one filesystem to be atomic, and a fixed name
/// is what lets the next job identify — and remove — the leftovers of a job
/// whose server process died mid-clone. The leading dot keeps it out of the
/// clone-root scan, which ignores dot-directories.
const TEMP_PREFIX: &str = ".delta-clone-tmp-";

/// The in-flight clone jobs of this process, keyed by destination path.
///
/// Cheap to clone (the set is behind an [`Arc`]) so a spawned job can hold the
/// same registry the interactor does and retire its own entry when it finishes.
/// Purely in-memory: see the module docs for what that deliberately costs.
#[derive(Clone, Default)]
pub(in crate::interactor) struct CloneJobs {
    in_flight: Arc<Mutex<HashSet<String>>>,
}

impl CloneJobs {
    /// Claim `destination` for a new job, reporting whether the claim was won.
    ///
    /// `false` means a job for that exact destination is already running, which
    /// is the join case: the caller must NOT start a second `gh` process, and
    /// the running job's completion event serves both requests.
    async fn claim(&self, destination: &str) -> bool {
        self.in_flight.lock().await.insert(destination.to_owned())
    }

    /// Retire a finished job's claim, so a retry after a failure can start a
    /// fresh one. Called before the outcome is announced: an event that arrived
    /// while the claim still stood would invite a retry that silently joined the
    /// job it was retrying.
    async fn release(&self, destination: &str) {
        self.in_flight.lock().await.remove(destination);
    }
}

/// Everything one clone job needs, owned outright so the job can outlive the
/// call that started it.
struct CloneJob {
    gh_cli: Arc<dyn GhCli>,
    jobs: CloneJobs,
    /// The seam the outcome is announced on. `None` in a configuration that
    /// wired no sink (the domain tests that do not observe events), where the
    /// clone still runs and the outcome is only logged.
    event_sink: Option<AsyncEventSink>,
    repo_owner: String,
    repo_name: String,
    clone_root: String,
    destination: String,
    temp: String,
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Start cloning `repo_owner/repo_name` into `clone_root`, or join the job
    /// already cloning into the same destination.
    ///
    /// Returns as soon as the request is accepted — the clone itself runs on a
    /// spawned task and reports through
    /// [`SessionEvent::RepositoryCloneCompleted`] /
    /// [`SessionEvent::RepositoryCloneFailed`]. Rejections happen here, before
    /// any job exists: an unregistered `clone_root`
    /// ([`Error::CloneRootNotRegistered`]), an owner/name that cannot be part of
    /// a path ([`Error::InvalidRepositoryRef`]), or an existing destination
    /// ([`Error::CloneDestinationExists`]).
    pub async fn clone_repository(
        &self,
        repo_owner: &str,
        repo_name: &str,
        clone_root: &str,
    ) -> Result<()> {
        check_path_segment(repo_owner)?;
        check_path_segment(repo_name)?;

        let registered = self
            .store
            .list_clone_roots()
            .await?
            .into_iter()
            .any(|root| root.path == clone_root);
        if !registered {
            return Err(Error::CloneRootNotRegistered(clone_root.to_owned()));
        }

        let destination = join(clone_root, repo_name);
        if path_exists(&destination).await {
            return Err(Error::CloneDestinationExists(destination));
        }

        if !self.clone_jobs.claim(&destination).await {
            // Joining, not starting: the running job's event answers this
            // request too. This is the double-click and the two-tabs case, and
            // the reason the registry exists at all.
            tracing::info!(
                destination,
                "a clone job for this destination is already running; joining it"
            );
            return Ok(());
        }

        tokio::spawn(run(CloneJob {
            gh_cli: Arc::clone(&self.gh_cli),
            jobs: self.clone_jobs.clone(),
            event_sink: self.event_sink.clone(),
            repo_owner: repo_owner.to_owned(),
            repo_name: repo_name.to_owned(),
            clone_root: clone_root.to_owned(),
            temp: join(clone_root, &format!("{TEMP_PREFIX}{repo_name}")),
            destination,
        }));
        Ok(())
    }
}

/// Run one clone job to completion and announce the outcome.
async fn run(job: CloneJob) {
    let outcome = clone_into_place(&job).await;

    if let Err(reason) = &outcome {
        // The temporary directory is the only trace a failed clone leaves, and
        // leaving it would make the next attempt's cleanup the thing that
        // rescues it. Remove it here, where the failure is known.
        if let Err(err) = remove_dir_all_if_present(&job.temp).await {
            tracing::warn!(
                temp = job.temp,
                error = %err,
                reason,
                "could not remove the temporary directory of a failed clone",
            );
        }
    }

    // Retire the claim *before* announcing: a client that reacts to the failure
    // by retrying immediately must start a new job, not silently join the dead
    // one and then wait forever for an event that will never come.
    job.jobs.release(&job.destination).await;

    let event = match outcome {
        Ok(()) => {
            tracing::info!(
                destination = job.destination,
                "cloned {}/{}",
                job.repo_owner,
                job.repo_name,
            );
            SessionEvent::RepositoryCloneCompleted {
                repo_owner: job.repo_owner,
                repo_name: job.repo_name,
                clone_root: job.clone_root,
                destination_path: job.destination,
            }
        }
        Err(message) => {
            tracing::warn!(
                destination = job.destination,
                message,
                "cloning {}/{} failed",
                job.repo_owner,
                job.repo_name,
            );
            SessionEvent::RepositoryCloneFailed {
                repo_owner: job.repo_owner,
                repo_name: job.repo_name,
                clone_root: job.clone_root,
                destination_path: job.destination,
                message,
            }
        }
    };

    match &job.event_sink {
        Some(sink) => sink.emit(event),
        None => tracing::debug!(
            ?event,
            "no event sink is wired; the clone outcome is not announced",
        ),
    }
}

/// Clone into the temporary directory and move it onto the destination.
///
/// The failure type is the message the browser will be shown, so each step
/// reports in terms the user can act on.
async fn clone_into_place(job: &CloneJob) -> std::result::Result<(), String> {
    // A temporary directory that already exists belongs to a job this process
    // does not know about — one whose server died mid-clone, since a live job
    // holds the destination's claim and would have sent us down the join path.
    // Its contents are a partial clone of exactly this repository, so removing
    // it is the only correct move: `gh` refuses a non-empty target anyway.
    remove_dir_all_if_present(&job.temp)
        .await
        .map_err(|err| format!("could not clear the temporary clone directory: {err}"))?;

    job.gh_cli
        .clone_repo(&job.repo_owner, &job.repo_name, &job.temp)
        .await
        .map_err(|err| err.to_string())?;

    // The atomic step: within one directory a rename either happens or does
    // not, so the destination is never observed half-cloned.
    tokio::fs::rename(&job.temp, &job.destination)
        .await
        .map_err(|err| format!("could not move the finished clone into place: {err}"))
}

/// Whether `path` exists, following the same "anything at this path counts"
/// rule the destination check needs: a dangling symlink or a plain file at the
/// destination is just as much a reason to refuse as a directory, so this stats
/// the link itself rather than its target.
async fn path_exists(path: &str) -> bool {
    tokio::fs::symlink_metadata(path).await.is_ok()
}

/// Remove `path` and everything under it, treating "it was not there" as
/// success. Any other I/O error is reported, because silently continuing would
/// hand `gh` a directory it will refuse.
async fn remove_dir_all_if_present(path: &str) -> std::io::Result<()> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        // A plain file (or symlink) sitting where the temporary directory goes
        // is not a directory tree, so `remove_dir_all` refuses it. Remove it as
        // a file instead: either way the path has to be free.
        Err(_) => match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        },
    }
}

/// Join `segment` onto `root` as a single path component.
///
/// Trailing slashes on `root` are trimmed first so a bare `/` root yields
/// `/<segment>` rather than `//<segment>`; the registration endpoint already
/// canonicalises stored roots the same way.
fn join(root: &str, segment: &str) -> String {
    format!("{}/{segment}", root.trim_end_matches('/'))
}

/// Reject an owner or repository name that cannot be one path component.
///
/// The destination is built by joining the repository name onto a clone root, so
/// a name containing `/`, a `..` component, or a NUL would let a request write
/// outside the root it named. A leading `-` is refused too: the slug reaches
/// `gh repo clone` as a positional argument, so `-x`/`--flag` could be parsed as
/// an option (argument injection) — the `--` separator at the call site is the
/// belt, this is the suspenders. The API surface never produces one (these come
/// from `gh`'s own PR rows), which is exactly why the check belongs here rather
/// than in the transport: it holds for every caller, including a future one.
fn check_path_segment(segment: &str) -> Result<()> {
    let invalid = segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.starts_with('-')
        || segment.contains('/')
        || segment.contains('\\')
        || segment.contains('\0');
    if invalid {
        return Err(Error::InvalidRepositoryRef(segment.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
