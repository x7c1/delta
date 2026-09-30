use std::ffi::OsString;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use super::{parse_printed_env, MARKER};

/// How long the login shell may take to print its environment.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// The arguments that make a shell print its environment between the markers,
/// as an interactive login shell.
///
/// `printf` and `env` with `;` between them behave the same in `sh`, `bash`,
/// `zsh` and `fish`, and `env` prints `PATH` `:`-joined in every one of them.
fn login_shell_args() -> [String; 4] {
    [
        "-i".to_owned(),
        "-l".to_owned(),
        "-c".to_owned(),
        format!("printf '%s\\n' {MARKER}; env; printf '%s' {MARKER}"),
    ]
}

pub fn read_login_shell_env(shell: &OsString) -> Result<Vec<(String, String)>, String> {
    let mut child = Command::new(shell)
        .args(login_shell_args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("spawn failed: {err}"))?;
    let mut stdout = child.stdout.take().ok_or("no stdout pipe")?;

    // Read on a helper thread so the wait can time out: a process the rc files
    // start in the background may hold the pipe open after the shell exits, so
    // neither the read nor the wait is guaranteed to return.
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = String::new();
        let result = stdout.read_to_string(&mut output).map(|_| output);
        let _ = sender.send(result);
    });
    let received = receiver.recv_timeout(TIMEOUT);
    if received.is_err() {
        let _ = child.kill();
    }
    // Reap the shell off the main thread; it has exited or been killed.
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    match received {
        Ok(Ok(output)) => parse_printed_env(&output)
            .ok_or_else(|| "the shell did not print its environment".to_owned()),
        Ok(Err(err)) => Err(format!("reading its output failed: {err}")),
        Err(_) => Err(format!("no answer within {}s", TIMEOUT.as_secs())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End to end against `/bin/sh`: the command the shell is given prints the
    /// environment it was started with between the markers.
    #[test]
    fn a_real_shell_prints_its_path() {
        let vars = read_login_shell_env(&OsString::from("/bin/sh")).expect("sh answers");
        assert!(vars
            .iter()
            .any(|(name, value)| name == "PATH" && !value.is_empty()));
    }
}
