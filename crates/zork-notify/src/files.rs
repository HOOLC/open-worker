//! Kernel change sources, including atomic directory replacement. Successful
//! watches have no timer; only failed registrations use bounded retry backoff.
use notify::{RecommendedWatcher, RecursiveMode, Watcher, WatcherKind};
#[cfg(target_os = "macos")]
mod vnode;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

const CHANGED: u8 = 1;
const RESCAN: u8 = 2;
const REARM: u8 = 4;

/// An exact set of files, registered through their parents before the initial
/// read. Atomic replacement and initially absent files retain the same source.
pub struct Source {
    #[cfg(not(target_os = "macos"))]
    _watch: FileWatch,
    #[cfg(target_os = "macos")]
    _watch: vnode::Watch,
    signal: crate::Notifier,
}
impl Source {
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> notify::Result<Self> {
        let files: std::collections::BTreeSet<_> = paths.into_iter().collect();
        #[cfg(not(target_os = "macos"))]
        let roots: std::collections::BTreeSet<_> = files
            .iter()
            .cloned()
            .chain(
                files
                    .iter()
                    .filter_map(|path| path.parent().map(Path::to_owned)),
            )
            .collect();
        let signal = crate::Notifier::default();
        let publisher = signal.clone();
        #[cfg(target_os = "macos")]
        let watch = vnode::Watch::new(files, publisher).map_err(notify::Error::io)?;
        #[cfg(not(target_os = "macos"))]
        let watch = watch_paths(
            roots.into_iter().map(|path| (path, false)).collect(),
            move |path| files.contains(path),
            move |_| publisher.notify(),
        )?;
        Ok(Self {
            _watch: watch,
            signal,
        })
    }
    pub fn subscribe(&self) -> crate::Changes {
        self.signal.subscribe()
    }
}

pub struct FileWatch {
    control: mpsc::SyncSender<()>,
    stopping: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for FileWatch {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        let _ = self.control.try_send(());
        if let Some(worker) = self.worker.take() {
            // A callback can release the last owner of this watch.
            if worker.thread().id() != std::thread::current().id() {
                let _ = worker.join();
            }
        }
    }
}

pub fn watch(
    root: &Path,
    relevant: impl Fn(&Path) -> bool + Send + 'static,
    changed: impl FnMut(bool) + Send + 'static,
) -> notify::Result<FileWatch> {
    watch_paths(vec![(root.to_owned(), true)], relevant, changed)
}

fn canonical(path: &Path) -> notify::Result<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| notify::Error::io(error))?;
            Ok(canonical(parent)?.join(
                path.file_name()
                    .ok_or_else(|| notify::Error::generic("watch path has no name"))?,
            ))
        }
        Err(error) => Err(notify::Error::io(error)),
    }
}

fn registrations(paths: &[(PathBuf, bool)]) -> notify::Result<BTreeMap<PathBuf, bool>> {
    let mut result = BTreeMap::new();
    for (path, recursive) in paths {
        // A stable parent announces creation/replacement of a watched directory.
        let mut parent = path.parent();
        while let Some(directory) = parent {
            if directory.is_dir() {
                result.entry(directory.to_owned()).or_insert(false);
                break;
            }
            parent = directory.parent();
        }
        match std::fs::metadata(path) {
            Ok(_) => {
                result
                    .entry(path.clone())
                    .and_modify(|r| *r |= *recursive)
                    .or_insert(*recursive);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(notify::Error::io(error)),
        }
    }
    Ok(result)
}
fn register(
    watcher: &mut RecommendedWatcher,
    paths: &BTreeMap<PathBuf, bool>,
) -> notify::Result<()> {
    for (path, recursive) in paths {
        watcher.watch(
            path,
            if *recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            },
        )?;
    }
    Ok(())
}

/// Watch only the required roots. A nonrecursive root does not scan repositories,
/// transcripts or workspaces. Callback paths preserve the caller's root spelling,
/// including symlinked /tmp. Missing/replaced directories are rearmed by parents.
pub fn watch_paths(
    paths: Vec<(PathBuf, bool)>,
    relevant: impl Fn(&Path) -> bool + Send + 'static,
    mut changed: impl FnMut(bool) + Send + 'static,
) -> notify::Result<FileWatch> {
    if RecommendedWatcher::kind() == WatcherKind::PollWatcher {
        return Err(notify::Error::generic(
            "kernel file notifications unavailable",
        ));
    }
    if paths.is_empty() {
        return Err(notify::Error::generic("watch paths empty"));
    }
    let mut aliases = paths
        .iter()
        .map(|(path, _)| Ok((canonical(path)?, path.clone())))
        .collect::<notify::Result<Vec<_>>>()?;
    aliases.sort_by_key(|(canonical, _)| std::cmp::Reverse(canonical.components().count()));
    let selected_roots: Vec<_> = paths.iter().map(|(path, _)| path.clone()).collect();
    // One wake token plus a coalesced flag set, never an unbounded OS-event queue.
    let (control, receiver) = mpsc::sync_channel(1);
    let pending = Arc::new(AtomicU8::new(0));
    let writes = pending.clone();
    let stopping = Arc::new(AtomicBool::new(false));
    let stopped = stopping.clone();
    let sender = control.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let (rescan, rearm) = match event {
            // Subscriber reads cannot excite another read/notification cycle.
            Ok(event)
                if event.kind.is_access()
                    && !matches!(
                        event.kind,
                        notify::EventKind::Access(notify::event::AccessKind::Close(
                            notify::event::AccessMode::Write
                        ))
                    ) =>
            {
                return;
            }
            Ok(event) if event.need_rescan() => (true, true),
            Ok(event) => {
                let paths: Vec<_> = event
                    .paths
                    .iter()
                    .map(|path| {
                        aliases
                            .iter()
                            .find_map(|(canonical, requested)| {
                                path.strip_prefix(canonical)
                                    .ok()
                                    .map(|suffix| requested.join(suffix))
                            })
                            .unwrap_or_else(|| path.clone())
                    })
                    .collect();
                let rearm = paths.iter().any(|path| selected_roots.contains(path));
                if !rearm && !paths.iter().any(|path| relevant(path)) {
                    return;
                }
                (false, rearm)
            }
            Err(_) => (true, true),
        };
        writes.fetch_or(
            CHANGED | if rescan { RESCAN } else { 0 } | if rearm { REARM } else { 0 },
            Ordering::Release,
        );
        let _ = sender.try_send(());
    })?;
    let mut watched = registrations(&paths)?;
    register(&mut watcher, &watched)?;
    let worker = std::thread::spawn(move || {
        let mut retry: Option<Duration> = None;
        loop {
            if stopped.load(Ordering::Acquire) {
                return;
            }
            let forced = match retry {
                Some(delay) => match receiver.recv_timeout(delay) {
                    Ok(()) => 0,
                    Err(mpsc::RecvTimeoutError::Timeout) => CHANGED | RESCAN | REARM,
                    Err(_) => return,
                },
                None => match receiver.recv() {
                    Ok(()) => 0,
                    Err(_) => return,
                },
            };
            if stopped.load(Ordering::Acquire) {
                return;
            }
            let flags = pending.swap(0, Ordering::AcqRel) | forced;
            if flags == 0 {
                continue;
            }
            let rescan = flags & RESCAN != 0;
            let rearm = flags & REARM != 0;
            if rearm {
                for path in watched.keys() {
                    let _ = watcher.unwatch(path);
                }
                let result = registrations(&paths).and_then(|next| {
                    watched = next;
                    register(&mut watcher, &watched)
                });
                retry = if result.is_err() {
                    Some(retry.map_or(Duration::from_secs(1), |delay| {
                        (delay * 2).min(Duration::from_secs(30))
                    }))
                } else {
                    None
                };
            }
            // Register before the authority reads; a replacement never leaves a
            // gap between the catch-up snapshot and the next kernel observation.
            changed(rescan || retry.is_some());
        }
    });
    Ok(FileWatch {
        control,
        stopping,
        worker: Some(worker),
    })
}
