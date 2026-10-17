use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};

use super::instance_lock::InstanceLock;

/// What a second launch writes to the running copy before it closes the
/// connection. The running copy ignores where it was started from and with
/// what arguments, so nothing else is sent.
const NOTIFICATION: &[u8] = b"delta: launched again\n";

/// How long the running copy waits for a connection to send [`NOTIFICATION`]
/// and close. A connection that takes longer is dropped without effect.
const READ_LIMIT: Duration = Duration::from_secs(2);

/// How long the accept loop pauses after a failed accept, so an error that
/// repeats (such as running out of file descriptors) does not spin.
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

/// Called on the running copy for each later launch.
type OnSecondLaunch = Arc<dyn Fn() + Send + Sync + 'static>;

/// The outcome of [`claim`].
pub enum Claim {
    /// Another copy holds the lock and was told about this launch; this one
    /// exits.
    AlreadyRunning,
    /// This copy holds the lock and is listening for later launches.
    Acquired(InstanceLock),
}

/// Take the single-instance lock at the socket `path`, or tell the copy that
/// holds it about this launch.
///
/// A copy that is listening there is sent a notification, and this launch is
/// [`Claim::AlreadyRunning`]. When nothing is there, or only the socket of a
/// copy that ended without removing it (nothing listening), this copy binds the
/// path and calls `on_second_launch`, on a thread of its own, each time a later
/// launch notifies it.
///
/// An error means the lock could not be set up: the path holds something other
/// than a socket, connecting failed in another way, or the bind failed.
pub fn claim(
    path: &Path,
    on_second_launch: impl Fn() + Send + Sync + 'static,
) -> anyhow::Result<Claim> {
    claim_with_limit(path, READ_LIMIT, Arc::new(on_second_launch))
}

fn claim_with_limit(
    path: &Path,
    read_limit: Duration,
    on_second_launch: OnSecondLaunch,
) -> anyhow::Result<Claim> {
    match notify(path) {
        Ok(()) => return Ok(Claim::AlreadyRunning),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) if err.kind() == ErrorKind::ConnectionRefused => remove_stale_socket(path)?,
        Err(err) => {
            return Err(err)
                .with_context(|| format!("could not reach the running copy at {}", path.display()))
        }
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("could not bind {}", path.display()))?;
    thread::Builder::new()
        .name("single-instance".to_owned())
        .spawn(move || accept_launches(&listener, read_limit, &on_second_launch))
        .context("could not start the single-instance listener thread")?;
    Ok(Claim::Acquired(InstanceLock::new(path.to_owned())))
}

/// Tell the copy listening at `path` about this launch.
fn notify(path: &Path) -> io::Result<()> {
    // The notification fits in a fresh socket's buffer, so the write does not
    // wait for the running copy to read it.
    UnixStream::connect(path)?.write_all(NOTIFICATION)
}

/// Remove the socket at `path` that nothing listens on, left by a copy that
/// ended without removing it. Anything other than a socket is left alone.
fn remove_stale_socket(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("could not inspect {}", path.display()))?;
    if !metadata.file_type().is_socket() {
        bail!("{} is not a socket", path.display());
    }
    tracing::info!(
        socket = %path.display(),
        "taking over the single-instance socket a previous copy left"
    );
    fs::remove_file(path).with_context(|| format!("could not remove {}", path.display()))
}

/// Accept later launches for the rest of the process's life, reading each on a
/// thread of its own so that one slow connection holds up no other.
fn accept_launches(
    listener: &UnixListener,
    read_limit: Duration,
    on_second_launch: &OnSecondLaunch,
) {
    loop {
        let stream = match listener.accept() {
            Ok((stream, _addr)) => stream,
            Err(err) => {
                tracing::warn!("the single-instance socket could not accept a connection: {err}");
                thread::sleep(ACCEPT_RETRY_DELAY);
                continue;
            }
        };
        let on_second_launch = Arc::clone(on_second_launch);
        let spawned = thread::Builder::new()
            .name("single-instance-read".to_owned())
            .spawn(move || match read_notification(stream, read_limit) {
                Ok(()) => on_second_launch(),
                Err(err) => tracing::warn!(
                    "a connection to the single-instance socket was not a launch: {err}"
                ),
            });
        if let Err(err) = spawned {
            tracing::warn!("could not read a connection to the single-instance socket: {err}");
        }
    }
}

/// Read [`NOTIFICATION`] from `stream` up to its end, within `limit` in all.
fn read_notification(mut stream: UnixStream, limit: Duration) -> io::Result<()> {
    let deadline = Instant::now() + limit;
    let mut received = Vec::with_capacity(NOTIFICATION.len());
    let mut buf = [0u8; 64];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                ErrorKind::TimedOut,
                format!("nothing complete within {}ms", limit.as_millis()),
            ));
        }
        if !wait_readable(&stream, left)? {
            continue;
        }
        let read = match stream.read(&mut buf) {
            Ok(read) => read,
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        if read == 0 {
            break;
        }
        received.extend_from_slice(&buf[..read]);
        if received.len() > NOTIFICATION.len() {
            break;
        }
    }
    if received != NOTIFICATION {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "{} bytes that are not a launch notification",
                received.len()
            ),
        ));
    }
    Ok(())
}

/// Wait at most `timeout` for `stream` to have data or its end to read; `false`
/// when it has neither yet.
///
/// `poll` rather than a read timeout: macOS refuses to set `SO_RCVTIMEO` on a
/// socket whose peer has already closed it (`EINVAL`), which is what a second
/// launch has done by the time its connection is read.
fn wait_readable(stream: &UnixStream, timeout: Duration) -> io::Result<bool> {
    let mut poll_fd = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let millis =
        libc::c_int::try_from(timeout.as_micros().div_ceil(1000)).unwrap_or(libc::c_int::MAX);
    // SAFETY: `poll_fd` is one valid `pollfd`, and its descriptor stays open
    // for the call, as `stream` owns it.
    let ready = unsafe { libc::poll(&mut poll_fd, 1, millis) };
    if ready < 0 {
        let err = io::Error::last_os_error();
        return match err.kind() {
            ErrorKind::Interrupted => Ok(false),
            _ => Err(err),
        };
    }
    Ok(ready > 0)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;

    use super::*;

    /// Longer than any wait a passing test needs, short enough that a failing
    /// one ends.
    const WAIT: Duration = Duration::from_secs(5);

    /// `claim` with a callback that sends on the returned channel.
    fn claim_counting(path: &Path, read_limit: Duration) -> (Claim, mpsc::Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        let sender = std::sync::Mutex::new(sender);
        let claim = claim_with_limit(
            path,
            read_limit,
            Arc::new(move || sender.lock().unwrap().send(()).unwrap()),
        )
        .unwrap();
        (claim, receiver)
    }

    fn socket_in(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("io_github_x7c1_delta_si.sock")
    }

    /// Leave a socket file at `path` that nothing listens on, as a copy that
    /// crashed does.
    ///
    /// The socket is bound and closed without ever listening. Binding a
    /// `UnixListener` and dropping it would also listen, and a process another
    /// test forks in between inherits the descriptor until it execs: a connect
    /// in that window is accepted into the backlog instead of refused.
    fn leave_stale_socket(path: &Path) {
        use std::os::fd::{FromRawFd, OwnedFd};
        use std::os::unix::ffi::OsStrExt;

        // SAFETY: `socket` has no preconditions; the result is checked.
        let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        assert!(raw >= 0, "socket: {}", io::Error::last_os_error());
        // SAFETY: `raw` is a descriptor just opened and owned by nothing else.
        let socket = unsafe { OwnedFd::from_raw_fd(raw) };

        // SAFETY: an all-zero `sockaddr_un` is a valid value of the type.
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let bytes = path.as_os_str().as_bytes();
        assert!(bytes.len() < addr.sun_path.len(), "{}", path.display());
        for (dst, src) in addr.sun_path.iter_mut().zip(bytes) {
            *dst = libc::c_char::from_ne_bytes([*src]);
        }
        let len = libc::socklen_t::try_from(std::mem::size_of::<libc::sockaddr_un>()).unwrap();
        // SAFETY: `addr` is a valid `sockaddr_un` of `len` bytes, and `socket`
        // stays open for the call.
        let bound = unsafe {
            libc::bind(
                socket.as_raw_fd(),
                std::ptr::addr_of!(addr).cast::<libc::sockaddr>(),
                len,
            )
        };
        assert_eq!(bound, 0, "bind: {}", io::Error::last_os_error());
    }

    #[test]
    fn a_second_launch_reaches_the_running_copy_and_is_told_to_exit() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let (first, launches) = claim_counting(&path, READ_LIMIT);
        assert!(matches!(first, Claim::Acquired(_)));
        assert!(path.exists());

        let second = claim(&path, || panic!("the second launch was notified")).unwrap();
        assert!(matches!(second, Claim::AlreadyRunning));
        launches
            .recv_timeout(WAIT)
            .expect("the running copy was not told about the second launch");
    }

    #[test]
    fn a_stale_socket_is_removed_and_taken_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        leave_stale_socket(&path);
        assert!(path.exists());

        let (claimed, launches) = claim_counting(&path, READ_LIMIT);
        assert!(matches!(claimed, Claim::Acquired(_)));
        assert!(matches!(
            claim(&path, || {}).unwrap(),
            Claim::AlreadyRunning
        ));
        launches
            .recv_timeout(WAIT)
            .expect("the copy that took the socket over is not listening on it");
    }

    #[test]
    fn something_other_than_a_socket_at_the_path_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        fs::write(&path, "not a socket").unwrap();

        assert!(claim(&path, || {}).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "not a socket");
    }

    #[test]
    fn a_client_that_never_sends_or_closes_does_not_hold_up_later_launches() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let (_lock, launches) = claim_counting(&path, READ_LIMIT);

        let _silent = UnixStream::connect(&path).unwrap();
        let started = Instant::now();
        assert!(matches!(
            claim(&path, || {}).unwrap(),
            Claim::AlreadyRunning
        ));
        launches
            .recv_timeout(WAIT)
            .expect("the launch after a silent client was not forwarded");
        assert!(
            started.elapsed() < READ_LIMIT,
            "the launch waited for the silent client's read limit"
        );
    }

    #[test]
    fn a_client_that_never_closes_is_dropped_after_the_read_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let (_lock, launches) = claim_counting(&path, Duration::from_millis(200));

        let mut silent = UnixStream::connect(&path).unwrap();
        silent.set_read_timeout(Some(WAIT)).unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(
            silent.read(&mut buf).unwrap(),
            0,
            "the running copy did not close the connection"
        );
        assert!(
            launches.try_recv().is_err(),
            "a silent client counted as a launch"
        );
    }

    #[test]
    fn a_connection_that_does_not_send_the_notification_is_no_launch() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let (_lock, launches) = claim_counting(&path, READ_LIMIT);

        let mut other = UnixStream::connect(&path).unwrap();
        other.write_all(b"something else").unwrap();
        drop(other);
        assert!(launches.recv_timeout(Duration::from_millis(300)).is_err());

        assert!(matches!(
            claim(&path, || {}).unwrap(),
            Claim::AlreadyRunning
        ));
        launches.recv_timeout(WAIT).unwrap();
    }

    #[test]
    fn releasing_the_lock_removes_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let Claim::Acquired(lock) = claim(&path, || {}).unwrap() else {
            panic!("the lock was not acquired");
        };
        lock.release();
        assert!(!path.exists());
        lock.release();
    }
}
