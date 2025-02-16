use eyre::Context;
use ignore::Walk;
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventHandler, DebounceEventResult};
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    sync::mpsc::{channel, Receiver, Sender},
    task::spawn_blocking,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::Instrument;

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

    #[error("error scanning files: {0}")]
    Ignore(#[from] ignore::Error),
}

#[derive(Debug)]
pub enum Match {
    StartsWith(PathBuf),
    Extension(String),
}
impl Match {
    fn is_match(&self, path: &Path) -> bool {
        match self {
            Match::StartsWith(prefix) => path.starts_with(prefix),
            Match::Extension(ext) => path.extension().is_some_and(|e| e == ext.as_str()),
        }
    }
}

pub struct Matchers(Vec<(&'static str, Match)>);

impl Matchers {
    fn find(&self, path: &Path) -> Option<(&'static str, &Match)> {
        self.0
            .iter()
            .map(|(n, r)| (*n, r))
            .find(|(_, matcher)| matcher.is_match(path))
    }

    fn find_name(&self, path: &Path) -> Option<&'static str> {
        self.find(path).map(|(name, _)| name)
    }
}

#[tracing::instrument(level = "debug", skip(token))]
pub async fn watch(
    token: CancellationToken,
    tracker: &TaskTracker,
    app: &Path,
    matchers: Vec<(&'static str, Match)>,
) -> Result<Receiver<(&'static str, HashSet<PathBuf>)>, eyre::Report> {
    let directory = app
        .canonicalize()
        .wrap_err_with(|| format!("cannot canonicalize {}", app.display()))?;
    let directory = directory.parent().expect("parent").to_path_buf();

    let matchers = Matchers(matchers);
    let (tx, rx) = channel(5);

    tracker.spawn(
        async move {
            let debouncer = spawn_blocking(move || {
                let checksums =
                    initial_checksums(&matchers, &directory).expect("initial checksums");
                let mut debouncer = new_debouncer(
                    Duration::from_secs(2),
                    None,
                    EventHandler {
                        checksums,
                        matchers,
                        tx,
                    },
                )
                .expect("new debouncer");
                debouncer
                    .watch(directory, RecursiveMode::Recursive)
                    .expect("watch");

                debouncer
            })
            .await
            .expect("spawn_blocking");

            tracing::debug!("watching files, will reload on change");
            token.cancelled().await;
            drop(debouncer);
            tracing::debug!("no longer watching files");
        }
        .instrument(tracing::debug_span!("watcher task")),
    );

    Ok(rx)
}

type Changed = HashMap<&'static str, HashSet<PathBuf>>;

type Checksums = HashMap<&'static str, HashMap<PathBuf, u32>>;

#[tracing::instrument(level = "debug", skip(matcher))]
fn initial_checksums(matcher: &Matchers, directory: &Path) -> Result<Checksums, Error> {
    let mut checksums = Checksums::new();

    let mut count: usize = 0;
    for entry in Walk::new(directory) {
        count += 1;
        let entry = entry?;
        let path = entry.path();

        if matches!(entry.file_type(), Some(file_type) if !file_type.is_file()) {
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

    tracing::debug!(count, "watcher checked files");

    Ok(checksums)
}

#[tracing::instrument(level = "error")]
fn report_errors(errors: Vec<notify::Error>) {
    for error in errors {
        tracing::error!(?error, "error watching files");
    }
}

impl DebounceEventHandler for EventHandler {
    #[tracing::instrument(level = "debug", skip(self, event))]
    fn handle_event(&mut self, event: DebounceEventResult) {
        match event {
            Ok(events) => {
                let paths = events.iter().flat_map(|event| event.paths.iter());
                let mut changes = Changed::new();

                for path in paths {
                    if !path.is_file() {
                        continue;
                    }
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

#[tracing::instrument(level = "debug")]
fn checksum_file(path: &Path) -> Result<u32, io::Error> {
    let contents = std::fs::read(path)?;
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&contents);
    Ok(hasher.finalize())
}
