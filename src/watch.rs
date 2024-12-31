use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventHandler, DebounceEventResult};
use regex::{Match, Regex};
use std::{
    collections::{HashMap, HashSet},
    io,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::block_in_place,
};
use tokio_util::sync::{CancellationToken, DropGuard};
use walkdir::WalkDir;

struct EventHandler {
    checksums: HashMap<&'static str, HashMap<PathBuf, u32>>,
    matchers: Matchers,
    tx: Sender<(&'static str, HashSet<PathBuf>)>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("notify error: {0}")]
    Notify(#[from] notify::Error),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("walkdir error: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("send error")]
    Send,
}

pub trait Matcher: Send + Sync + 'static {
    fn is_match(&self, path: &Path) -> bool;
}

impl Matcher for Regex {
    fn is_match(&self, path: &Path) -> bool {
        self.is_match(path.to_str().expect("path to str"))
    }
}

pub struct MatchParent(pub PathBuf);
pub struct MatchExtension(pub String);

impl Matcher for MatchParent {
    fn is_match(&self, path: &Path) -> bool {
        path.starts_with(&self.0)
    }
}

impl Matcher for MatchExtension {
    fn is_match(&self, path: &Path) -> bool {
        path.extension().map_or(false, |ext| ext == self.0.as_str())
    }
}

impl From<MatchParent> for Box<dyn Matcher> {
    fn from(parent: MatchParent) -> Self {
        Box::new(parent)
    }
}

impl From<MatchExtension> for Box<dyn Matcher> {
    fn from(ext: MatchExtension) -> Self {
        Box::new(ext)
    }
}

pub struct Matchers(Vec<(&'static str, Box<dyn Matcher>)>);

impl Matchers {
    fn find(&self, path: &Path) -> Option<(&'static str, &Box<dyn Matcher>)> {
        self.0
            .iter()
            .map(|(n, r)| (*n, r))
            .find(|(_, matcher)| matcher.is_match(path))
    }

    fn find_name(&self, path: &Path) -> Option<&'static str> {
        self.find(path).map(|(name, _)| name)
    }
}

pub async fn watch(directory: PathBuf, matchers: Vec<(&'static str, Box<dyn Matcher>)>) -> (Receiver<(&'static str, HashSet<PathBuf>)>, DropGuard) {
    let matchers = Matchers(matchers);
    let (tx, rx) = channel(5);
    let token = CancellationToken::new();

    let guard = token.clone().drop_guard();
    tokio::spawn(async move {
        let watch_directory = directory.clone();

        let debouncer = block_in_place(move || {
            let checksums = initial_checksums(&matchers, directory).expect("initial checksums");
            let mut debouncer = new_debouncer(
                Duration::from_secs(2),
                None,
                EventHandler {
                    checksums,
                    matchers,
                    tx,
                },
            )
            .unwrap();
            debouncer
                .watch(watch_directory, RecursiveMode::Recursive)
                .unwrap();

            debouncer
        });

        tracing::info!("watcher started");
        token.cancelled().await;
        drop(debouncer);
        tracing::info!("watcher stopped");
    });

    ( rx, guard )
}

type Changed = HashMap<&'static str, HashSet<PathBuf>>;

type Checksums = HashMap<&'static str, HashMap<PathBuf, u32>>;

fn initial_checksums(matcher: &Matchers, directory: PathBuf) -> Result<Checksums, Error> {
    let mut checksums = Checksums::new();

    for entry in WalkDir::new(&directory).into_iter() {
        let entry = entry?;
        let path = entry.path();

        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(name) = matcher.find_name(path) {
            let checksum = checksum_file(path)?;
            checksums
                .entry(name)
                .or_default()
                .insert(path.into(), checksum);
        }
    }

    Ok(checksums)
}

fn report_errors(errors: Vec<notify::Error>) {
    for error in errors {
        tracing::error!(?error, "error watching files");
    }
}

impl DebounceEventHandler for EventHandler {
    fn handle_event(&mut self, event: DebounceEventResult) {
        match event {
            Ok(events) => {
                let paths = events.iter().flat_map(|event| event.paths.iter());
                let mut changes = Changed::new();

                for path in paths {
                    tracing::debug!(?path, "file changed");
                    if let Some(name) = self.matchers.find_name(path) {
                        tracing::debug!(?name, "matched");
                        let checksum = checksum_file(path).expect("checksum file");
                        let previous = self
                            .checksums
                            .get_mut(name)
                            .and_then(|map| map.get_mut(path));
                        if let Some(previous) = previous {
                            if *previous == checksum {
                                continue;
                            }
                            *previous = checksum;
                        }
                        changes.entry(name).or_default().insert(path.into());
                    }
                }

                for change in changes {
                    self.tx.blocking_send(change).expect("send");
                }
            }
            Err(errors) => report_errors(errors),
        }
    }
}

fn checksum_file(path: &Path) -> Result<u32, io::Error> {
    let contents = std::fs::read(path)?;
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&contents);
    Ok(hasher.finalize())
}
