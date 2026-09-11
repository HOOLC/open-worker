//! Readiness for blocking platform adapters. All waits park in the kernel;
//! process death, user cancellation and the operation deadline remain distinct.
use std::{
    io::{self, Write},
    os::{
        fd::{AsRawFd, BorrowedFd},
        unix::net::UnixStream,
    },
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Instant,
};

pub struct Cancellation {
    cancelled: AtomicBool,
    reader: UnixStream,
    writer: Mutex<UnixStream>,
}
impl Cancellation {
    pub fn new() -> io::Result<Self> {
        let (writer, reader) = UnixStream::pair()?;
        Ok(Self {
            cancelled: AtomicBool::new(false),
            reader,
            writer: Mutex::new(writer),
        })
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            let _ = self.writer.lock().unwrap().write_all(&[1]);
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Ready {
    Readable,
    ProcessExited,
    Cancelled,
    TimedOut,
}

pub fn readable(
    fd: BorrowedFd<'_>,
    process: Option<u32>,
    cancel: Option<&Cancellation>,
    deadline: Instant,
) -> io::Result<Ready> {
    let exit = process
        .map(crate::process::exit_handle)
        .transpose()?
        .flatten();
    if process.is_some() && exit.is_none() {
        return Ok(Ready::ProcessExited);
    }
    let mut descriptors = [
        libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: exit.as_ref().map_or(-1, AsRawFd::as_raw_fd),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: cancel.map_or(-1, |cancel| cancel.reader.as_raw_fd()),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        if cancel.is_some_and(Cancellation::is_cancelled) {
            return Ok(Ready::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(Ready::TimedOut);
        }
        let timeout = remaining
            .as_millis()
            .saturating_add(1)
            .min(i32::MAX as u128) as i32;
        let count =
            unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, timeout) };
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if count == 0 {
            continue;
        } // Only a deadline beyond i32 milliseconds needs another wait.
        if descriptors[2].revents != 0 {
            return Ok(Ready::Cancelled);
        }
        if descriptors[1].revents != 0 {
            return Ok(Ready::ProcessExited);
        }
        if descriptors[0].revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(io::Error::other("readiness descriptor unavailable"));
        }
        if descriptors[0].revents != 0 {
            return Ok(Ready::Readable);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::fd::AsFd, sync::Arc, time::Duration};
    #[test]
    fn cancellation_interrupts_a_parked_listener_without_a_periodic_wakeup() {
        let (reader, _writer) = UnixStream::pair().unwrap();
        let cancel = Arc::new(Cancellation::new().unwrap());
        let token = cancel.clone();
        let task = std::thread::spawn(move || {
            readable(
                reader.as_fd(),
                None,
                Some(&token),
                Instant::now() + Duration::from_secs(30),
            )
            .unwrap()
        });
        cancel.cancel();
        assert_eq!(task.join().unwrap(), Ready::Cancelled);
    }
}
