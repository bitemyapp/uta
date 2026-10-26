use std::{os::unix::process::CommandExt, process::Command};

/// Do not leave audio or a credential-bearing helper behind if Uta crashes or
/// its thread pool exits while an external request is still in progress.
pub(crate) fn kill_with_parent(command: &mut Command) {
    let parent = std::process::id() as libc::pid_t;
    // SAFETY: after fork the closure uses only the prctl/getppid system calls
    // and constructs an errno. It takes no locks and calls no allocator.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
            }
            Ok(())
        });
    }
}
