//! Host capabilities shared by the supervisor, installer and client core.
use anyhow::Result;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use zork_notify::{files::FileWatch, process::ProcessWatch, Changes, Notifier};

pub struct Events {
    root: PathBuf,
    signal: Notifier,
    _files: FileWatch,
    supervisor: Option<(u32, Option<std::time::SystemTime>, ProcessWatch)>,
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        time::{Duration, Instant},
    };
    #[test]
    fn exact_file_source_observes_writes_while_open_and_replacement() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("open.wal");
        let mut writer = std::fs::File::create(&path).unwrap();
        let source = zork_notify::files::Source::new([path.clone()]).unwrap();
        let mut changes = source.subscribe();
        for value in [b"first", b"later"] {
            changes.checkpoint();
            writer.write_all(value).unwrap();
            writer.sync_data().unwrap();
            assert!(changes
                .blocking_changed(Instant::now() + Duration::from_secs(2))
                .unwrap());
        }
        changes.checkpoint();
        let temporary = root.path().join("replacement");
        std::fs::write(&temporary, "new").unwrap();
        std::fs::rename(&temporary, &path).unwrap();
        assert!(changes
            .blocking_changed(Instant::now() + Duration::from_secs(2))
            .unwrap());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "new");
    }
}
impl Events {
    /// Register before reading any readiness or service state. Only these files
    /// matter; repository changes and transcript writes never wake the host.
    pub fn new(root: &Path) -> Result<Self> {
        let socket = crate::zork_sock_path(root);
        let files = BTreeSet::from([
            root.join("service.json"),
            crate::zork_pid_path(root),
            socket.clone(),
            crate::ready_pid_path(root, "zork-station"),
            crate::ready_pid_path(root, "zork-mesh"),
            root.join("run/update.json"),
        ]);
        let mut roots = BTreeSet::from([root.to_owned(), root.join("run")]);
        if let Some(parent) = socket.parent() {
            roots.insert(parent.to_owned());
        }
        let signal = Notifier::default();
        let publisher = signal.clone();
        let watcher = zork_notify::files::watch_paths(
            roots.into_iter().map(|path| (path, false)).collect(),
            move |path| files.contains(path),
            move |_| publisher.notify(),
        )?;
        let mut events = Self {
            root: root.to_owned(),
            signal,
            _files: watcher,
            supervisor: None,
        };
        events.refresh()?;
        Ok(events)
    }
    pub fn subscribe(&self) -> Changes {
        self.signal.subscribe()
    }
    /// Follow a new process generation only after the source changed. Kernel
    /// exit notification also catches a crash leaving stale PID/socket files.
    pub fn refresh(&mut self) -> Result<()> {
        let pid = std::fs::read_to_string(crate::zork_pid_path(&self.root))
            .ok()
            .and_then(|pid| pid.trim().parse::<u32>().ok())
            .filter(|pid| *pid > 0);
        let modified = std::fs::metadata(crate::zork_pid_path(&self.root))
            .ok()
            .and_then(|meta| meta.modified().ok());
        if self.supervisor.as_ref().map(|(pid, at, _)| (*pid, *at))
            != pid.map(|pid| (pid, modified))
        {
            self.supervisor = pid
                .map(|pid| self.watch_process(pid).map(|watch| (pid, modified, watch)))
                .transpose()?;
        }
        Ok(())
    }
    pub fn watch_process(&self, pid: u32) -> Result<ProcessWatch> {
        Ok(ProcessWatch::with_notifier(pid, self.signal.clone())?)
    }
    pub fn supervisor_exited(&self) -> Result<bool> {
        self.supervisor
            .as_ref()
            .map_or(Ok(true), |(_, _, watch)| Ok(watch.exited()?))
    }
}
