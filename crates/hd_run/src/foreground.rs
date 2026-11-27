//! Foreground terminal ownership for inherited-stdio children in owned groups.
//! Follows shell job control (fish `src/proc.rs`): hand off, continue a child
//! that raced the handoff, then restore the parent's foreground group.

use std::fs::File;
use std::io;

use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
use rustix::process::{
    Pid, getpgid, getpgrp, getppid, kill_process_group, test_kill_process_group,
};
use rustix::termios::{tcgetpgrp, tcsetpgrp};

/// Restores the terminal when an inherited child completes or is cancelled.
pub struct Foreground {
    terminal: File,
    parent: Pid,
    child: Pid,
}

impl Foreground {
    /// Gives a controlling terminal to `pid` only while this process owns it.
    /// A non-interactive process has no controlling terminal and needs no handoff.
    pub fn give_to(pid: u32) -> io::Result<Option<Self>> {
        Self::from_group(pid, getpgrp())
    }

    /// A helper in its own group borrows foreground ownership from its parent.
    pub fn from_parent(pid: u32) -> io::Result<Option<Self>> {
        let Some(parent) = getppid() else {
            return Ok(None);
        };
        Self::from_group(pid, getpgid(Some(parent))?)
    }

    fn from_group(pid: u32, parent: Pid) -> io::Result<Option<Self>> {
        let Ok(terminal) = File::options().read(true).write(true).open("/dev/tty") else {
            return Ok(None);
        };
        if tcgetpgrp(&terminal)? != parent {
            return Ok(None);
        }
        let child = i32::try_from(pid)
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| io::Error::other("invalid child process group"))?;
        set_group(&terminal, child)?;
        let guard = Self {
            terminal,
            parent,
            child,
        };
        // The child may have tried reading before tcsetpgrp and received SIGTTIN.
        let _ = kill_process_group(child, rustix::process::Signal::CONT);
        Ok(Some(guard))
    }
}

impl Drop for Foreground {
    fn drop(&mut self) {
        // A later foreground child must not lose its terminal to this guard.
        if let Ok(current) = tcgetpgrp(&self.terminal)
            && (current == self.child
                || test_kill_process_group(current) == Err(rustix::io::Errno::SRCH))
        {
            let _ = set_group(&self.terminal, self.parent);
        }
    }
}

fn set_group(terminal: &File, group: Pid) -> io::Result<()> {
    // Only this thread blocks SIGTTOU, and only around the ioctl; no global
    // signal disposition changes. Restore still works while the parent is background.
    let mut blocked = SigSet::empty();
    blocked.add(Signal::SIGTTOU);
    let mut previous = SigSet::empty();
    pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&blocked), Some(&mut previous))?;
    let result = tcsetpgrp(terminal, group).map_err(io::Error::from);
    pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&previous), None)?;
    result
}
