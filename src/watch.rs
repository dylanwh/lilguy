use std::{
    collections::{hash_map::Entry, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use eyre::{eyre, Result};
use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventHandler, DebounceEventResult, DebouncedEvent};
use parking_lot::Mutex;
use tokio::{
    sync::mpsc::{Receiver, Sender},
    task::JoinHandle,
};
use xxhash_rust::xxh3::Xxh3;

type Checksums = Arc<Mutex<HashMap<PathBuf, u64>>>;

pub struct EventHandler {
    checksums: Checksums,
    changed_tx: Sender<Vec<PathBuf>>,
}

impl EventHandler {
    fn handle_event_failable(&mut self, event: DebounceEventResult) -> Result<()> {
        let mut checksums = self.checksums.lock();
        match event {
            Ok(events) => {
                let mut changed = vec![];
                for file in files(events) {
                    if !file.is_file() {
                        continue;
                    }
                    let new_checksum = checksum_file(&file)?;
                    let old_checksum = checksums.entry(file.clone()).or_insert(new_checksum);
                    if new_checksum != *old_checksum {
                        changed.push(file);
                    }
                }

                if !changed.is_empty() {
                    self.changed_tx.blocking_send(changed)?;
                }
            }
            Err(ref errors) if errors.len() == 1 => {
                return Err(eyre!("{}", errors[0]));
            }
            Err(errors) => {
                let err = errors
                    .into_iter()
                    .fold(eyre!("multiple errors: "), |acc, e| acc.wrap_err(e));
                return Err(err);
            }
        }
        Ok(())
    }
}

impl DebounceEventHandler for EventHandler {
    fn handle_event(&mut self, event: DebounceEventResult) {
        if let Err(e) = self.handle_event_failable(event) {
            tracing::error!("error in file watch event handler: {e}");
        }
    }
}

fn files(events: Vec<DebouncedEvent>) -> Vec<PathBuf> {
    let mut files = vec![];
    for event in events {
        for path in &event.paths {
            files.push(path.to_owned());
        }
    }

    files
}

type Debouncer = notify_debouncer_full::Debouncer<
    notify::RecommendedWatcher,
    notify_debouncer_full::RecommendedCache,
>;

pub enum Message {
    Watch(PathBuf, RecursiveMode),
    Unwatch(PathBuf),
}

pub struct Watch {
    msg_tx: Sender<Message>,
    changed_rx: Receiver<Vec<PathBuf>>,
    task: JoinHandle<()>,
}

impl Watch {
    fn new() -> Result<Self> {
        let checksums = Arc::new(Mutex::new(HashMap::new()));

        let (changed_tx, changed_rx) = tokio::sync::mpsc::channel(1);
        let debouncer = notify_debouncer_full::new_debouncer(
            Duration::from_millis(250),
            None,
            EventHandler {
                checksums: checksums.clone(),
                changed_tx,
            },
        )?;

        let (msg_tx, msg_rx) = tokio::sync::mpsc::channel(1);

        let task = tokio::task::spawn_blocking(move || {
            if let Err(e) = watch_actor(checksums, debouncer, msg_rx) {
                tracing::error!(?e, "error in watcher actor");
            }
        });

        Ok(Watch {
            msg_tx,
            changed_rx,
            task,
        })
    }

    async fn watch<P>(&self, path: P, recursive: bool) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().to_path_buf();
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        self.msg_tx.send(Message::Watch(path, mode)).await?;
        Ok(())
    }

    async fn unwatch<P>(&self, path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref().to_path_buf();
        self.msg_tx.send(Message::Unwatch(path)).await?;
        Ok(())
    }
}

fn watch_actor(
    checksums: Checksums,
    mut debouncer: Debouncer,
    mut rx: Receiver<Message>,
) -> Result<(), eyre::Error> {
    while let Some(message) = rx.blocking_recv() {
        match message {
            Message::Watch(path, RecursiveMode::NonRecursive) => {
                checksums.lock().insert(path.clone(), checksum_file(&path)?);
                debouncer.watch(path, RecursiveMode::NonRecursive)?;
            }
            Message::Watch(path, RecursiveMode::Recursive) => {
                checksums.lock().extend(checksum_dir(&path)?);
                debouncer.watch(path, RecursiveMode::Recursive)?;
            }
            Message::Unwatch(path) => {
                debouncer.unwatch(&path)?;
            }
        }
    }

    Ok(())
}

fn checksum_file<P>(path: P) -> Result<u64, std::io::Error>
where
    P: AsRef<Path>,
{
    let contents = std::fs::read(path)?;
    // using xxhash for a faster checksum
    let mut hasher = Xxh3::new();
    hasher.update(&contents);
    Ok(hasher.digest())
}

fn checksum_dir<P>(path: P) -> Result<impl Iterator<Item = (PathBuf, u64)>, std::io::Error>
where
    P: AsRef<Path>,
{
    Ok(ignore::Walk::new(path)
        .into_iter()
        .filter_map(|entry| {
            if let Ok(entry) = entry {
                if entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
                    let path = entry.path().to_path_buf();
                    let checksum = checksum_file(&path).ok()?;
                    Some((path, checksum))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .into_iter())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_watch() -> Result<()> {
        let mut watch = Watch::new()?;
        let temp_dir = tempfile::tempdir()?;
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, b"Hello, world!")?;

        watch.watch(&file_path, false).await?;
        tokio::time::sleep(Duration::from_secs(10)).await;

        // Simulate a file change
        std::fs::write(&file_path, b"Hello, Rust!")?;

        // Wait for the event to be processed
        if let Ok(Some(t)) =
            tokio::time::timeout(Duration::from_secs(10), watch.changed_rx.recv()).await
        {
            assert_eq!(t[0], file_path);
        } else {
            panic!("test failed");
        }

        watch.unwatch(&file_path).await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_watch_dir() -> Result<()> {
        let mut watch = Watch::new()?;
        let temp_dir = tempfile::tempdir()?;
        let dir_path = temp_dir.path().join("subdir");
        std::fs::create_dir(&dir_path)?;
        let file_path = dir_path.join("test.txt");
        std::fs::write(&file_path, b"Hello, world!")?;

        watch.watch(&temp_dir.path(), true).await?;
        tokio::time::sleep(Duration::from_secs(10)).await;

        // Simulate a file change
        std::fs::write(&file_path, b"Hello, Rust!")?;

        // Wait for the event to be processed
        if let Ok(Some(t)) =
            tokio::time::timeout(Duration::from_secs(10), watch.changed_rx.recv()).await
        {
            assert_eq!(t[0], file_path);
        } else {
            panic!("test failed");
        }

        watch.unwatch(&temp_dir.path()).await?;
        Ok(())
    }

    // multiple changed files
    #[tokio::test]
    async fn test_watch_multiple() -> Result<()> {
        let mut watch = Watch::new()?;
        let temp_dir = tempfile::tempdir()?;
        let dir_path = temp_dir.path().join("subdir");
        std::fs::create_dir(&dir_path)?;
        let file_path1 = dir_path.join("test1.txt");
        let file_path2 = dir_path.join("test2.txt");
        std::fs::write(&file_path1, b"Hello, world!")?;
        std::fs::write(&file_path2, b"Hello, world!")?;

        watch.watch(&temp_dir.path(), true).await?;
        tokio::time::sleep(Duration::from_secs(10)).await;

        // Simulate file changes
        std::fs::write(&file_path1, b"Hello, Rust!")?;
        std::fs::write(&file_path2, b"Hello, Rust!")?;

        // Wait for the event to be processed
        if let Ok(Some(t)) =
            tokio::time::timeout(Duration::from_secs(10), watch.changed_rx.recv()).await
        {
            assert!(t.contains(&file_path1));
            assert!(t.contains(&file_path2));
        } else {
            panic!("test failed");
        }

        watch.unwatch(&temp_dir.path()).await?;
        Ok(())
    }
}
