//! FSEvents can defer writes to a file held open (notably SQLite WAL). Exact
//! file sources use vnode events; directory sources keep the scalable backend.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{fs::MetadataExt, net::UnixStream},
    },
    path::PathBuf,
};

pub(super) struct Watch {
    cancel: UnixStream,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.cancel.write_all(&[1]);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct Entry {
    file: File,
    identity: (u64, u64),
    content: bool,
}

fn register(queue: &OwnedFd, fd: i32, filter: i16, flags: u32) -> io::Result<()> {
    let event = libc::kevent {
        ident: fd as _,
        filter,
        flags: libc::EV_ADD | libc::EV_CLEAR,
        fflags: flags,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    let result = unsafe {
        libc::kevent(
            queue.as_raw_fd(),
            &event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn desired(files: &BTreeSet<PathBuf>) -> BTreeMap<PathBuf, bool> {
    let mut result = BTreeMap::new();
    for file in files {
        result.insert(file.clone(), true);
        let mut parent = file.parent();
        while let Some(path) = parent {
            result.entry(path.to_owned()).or_insert(false);
            // Also keep the containing directory's parent for atomic replacement.
            if path.exists() {
                if let Some(parent) = path.parent() {
                    result.entry(parent.to_owned()).or_insert(false);
                }
                break;
            }
            parent = path.parent();
        }
    }
    result
}

fn rearm(
    queue: &OwnedFd,
    files: &BTreeSet<PathBuf>,
    entries: &mut BTreeMap<PathBuf, Entry>,
) -> io::Result<bool> {
    let desired = desired(files);
    let mut changed = false;
    entries.retain(|path, entry| {
        let keep = desired.contains_key(path)
            && std::fs::metadata(path).is_ok_and(|meta| (meta.dev(), meta.ino()) == entry.identity);
        changed |= !keep && entry.content;
        keep
    });
    for (path, content) in desired {
        if entries.contains_key(&path) {
            continue;
        }
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        register(
            queue,
            file.as_raw_fd(),
            libc::EVFILT_VNODE,
            libc::NOTE_WRITE
                | libc::NOTE_EXTEND
                | libc::NOTE_ATTRIB
                | libc::NOTE_RENAME
                | libc::NOTE_DELETE
                | libc::NOTE_REVOKE,
        )?;
        entries.insert(
            path,
            Entry {
                file,
                identity: (metadata.dev(), metadata.ino()),
                content,
            },
        );
        changed |= content;
    }
    Ok(changed)
}

impl Watch {
    pub(super) fn new(files: BTreeSet<PathBuf>, signal: crate::Notifier) -> io::Result<Self> {
        if files.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "watch files empty",
            ));
        }
        let raw = unsafe { libc::kqueue() };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let queue = unsafe { OwnedFd::from_raw_fd(raw) };
        let (cancel, stopped) = UnixStream::pair()?;
        register(&queue, stopped.as_raw_fd(), libc::EVFILT_READ, 0)?;
        let mut entries = BTreeMap::new();
        rearm(&queue, &files, &mut entries)?;
        let worker = std::thread::spawn(move || {
            let mut retry = None;
            loop {
                let mut events: [libc::kevent; 16] = unsafe { std::mem::zeroed() };
                let timeout = retry.map(|seconds| libc::timespec {
                    tv_sec: seconds,
                    tv_nsec: 0,
                });
                let count = unsafe {
                    libc::kevent(
                        queue.as_raw_fd(),
                        std::ptr::null(),
                        0,
                        events.as_mut_ptr(),
                        events.len() as _,
                        timeout.as_ref().map_or(std::ptr::null(), |value| value),
                    )
                };
                if count < 0 {
                    if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    signal.notify();
                    return;
                }
                let events = &events[..count as usize];
                if events
                    .iter()
                    .any(|event| event.ident == stopped.as_raw_fd() as usize)
                {
                    return;
                }
                let content_changed = events.iter().any(|event| {
                    entries.values().any(|entry| {
                        entry.content && entry.file.as_raw_fd() as usize == event.ident
                    })
                });
                // Register replacements before publishing so the subsequent read
                // cannot race a gap between the old and new inode observations.
                match rearm(&queue, &files, &mut entries) {
                    Ok(replaced) => {
                        retry = None;
                        if replaced || content_changed {
                            signal.notify();
                        }
                    }
                    Err(_) => {
                        retry = Some(retry.map_or(1, |delay| (delay * 2).min(30)));
                        signal.notify();
                    }
                }
            }
        });
        Ok(Self {
            cancel,
            worker: Some(worker),
        })
    }
}
