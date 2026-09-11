//! Kernel process-exit observation. Cancellation wakes the wait immediately;
//! observing a process neither kills it nor reaps a child owned by the caller.
use std::{
    io::{self, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    sync::{Arc, Mutex},
};

pub struct ProcessWatch {
    state: Arc<Mutex<Option<Result<(), String>>>>,
    signal: crate::Notifier,
    cancel: UnixStream,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for ProcessWatch {
    fn drop(&mut self) {
        let _ = self.cancel.write_all(&[1]);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl ProcessWatch {
    pub fn new(pid: u32) -> io::Result<Self> {
        Self::with_notifier(pid, crate::Notifier::default())
    }
    /// Merge this capability's wakeups into an existing host change source.
    pub fn with_notifier(pid: u32, signal: crate::Notifier) -> io::Result<Self> {
        let handle = exit_handle(pid)?;
        let (cancel, stopped) = UnixStream::pair()?;
        let state = Arc::new(Mutex::new(handle.is_none().then_some(Ok(()))));
        let worker = handle.map(|handle| {
            let state = state.clone();
            let signal = signal.clone();
            std::thread::spawn(move || {
                let mut descriptors = [
                    libc::pollfd {
                        fd: stopped.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: handle.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                let result = loop {
                    // poll(-1) is a kernel event wait, with no sampling timer.
                    let result = unsafe { libc::poll(descriptors.as_mut_ptr(), 2, -1) };
                    if result < 0 {
                        let error = io::Error::last_os_error();
                        if error.kind() == io::ErrorKind::Interrupted {
                            continue;
                        }
                        break Err(error.to_string());
                    }
                    if descriptors[0].revents != 0 {
                        return;
                    }
                    if descriptors[1].revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                        break Err("process observation failed".into());
                    }
                    if descriptors[1].revents != 0 {
                        break Ok(());
                    }
                };
                *state.lock().unwrap() = Some(result);
                signal.notify();
            })
        });
        Ok(Self {
            state,
            signal,
            cancel,
            worker,
        })
    }
    pub fn subscribe(&self) -> crate::Changes {
        self.signal.subscribe()
    }
    pub fn exited(&self) -> io::Result<bool> {
        match self.state.lock().unwrap().as_ref() {
            None => Ok(false),
            Some(Ok(())) => Ok(true),
            Some(Err(error)) => Err(io::Error::other(error.clone())),
        }
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
pub(crate) fn exit_handle(pid: u32) -> io::Result<Option<OwnedFd>> {
    let descriptor = unsafe { libc::kqueue() };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
    let event = libc::kevent {
        ident: pid as _,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    let result = unsafe {
        libc::kevent(
            descriptor.as_raw_fd(),
            &event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        return Err(error);
    }
    Ok(Some(descriptor))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn exit_handle(pid: u32) -> io::Result<Option<OwnedFd>> {
    let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
    if descriptor < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        return Err(error);
    }
    Ok(Some(unsafe { OwnedFd::from_raw_fd(descriptor as _) }))
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
    target_os = "linux",
    target_os = "android"
)))]
pub(crate) fn exit_handle(_: u32) -> io::Result<Option<OwnedFd>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "kernel process observation unavailable",
    ))
}

/// Observe then reap an owned child, with a single deadline and no try-wait loop.
pub fn wait(
    child: &mut std::process::Child,
    deadline: std::time::Instant,
) -> io::Result<Option<std::process::ExitStatus>> {
    if let Some(status) = child.try_wait()? {
        return Ok(Some(status));
    }
    let watch = ProcessWatch::new(child.id())?;
    let mut changes = watch.subscribe();
    loop {
        changes.checkpoint();
        if watch.exited()? {
            return child.wait().map(Some);
        }
        if !changes
            .blocking_changed(deadline)
            .map_err(io::Error::other)?
        {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    #[test]
    fn exit_notification_and_cancellation_do_not_own_the_process() {
        let mut child = Command::new("sh")
            .args(["-c", "read value"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let watch = ProcessWatch::new(child.id()).unwrap();
        let mut changes = watch.subscribe();
        changes.checkpoint();
        assert!(!watch.exited().unwrap());
        child.stdin.take();
        if !watch.exited().unwrap() {
            assert!(changes
                .blocking_changed(Instant::now() + Duration::from_secs(3))
                .unwrap());
        }
        assert!(watch.exited().unwrap());
        child.wait().unwrap();
        let mut child = Command::new("sh")
            .args(["-c", "read value"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let watch = ProcessWatch::new(child.id()).unwrap();
        let started = Instant::now();
        drop(watch);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(child.try_wait().unwrap().is_none());
        child.stdin.take();
        child.wait().unwrap();
    }
}
