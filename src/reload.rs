#![allow(dead_code)]
#![allow(unused)]

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use mlua::serde::de;
use notify::{event, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, Debouncer};

#[derive(Debug, Default)]
pub struct Reloaders {
    debouncers: HashMap<&'static str, Debouncer<RecommendedWatcher, notify_debouncer_full::RecommendedCache>>
}

impl Reloaders {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add<R>(&mut self, reload: R) -> Result<(), notify::Error>
    where
        R: Reload + Send + 'static,
    {
        let name = reload.name();
        let files = reload.files();
        let mut previous_checksums = checksum_files(&files)?;
        let mut debouncer = new_debouncer(Duration::from_secs(2), None, move |event| {
            let new_checksums = checksum_events(event);
            let changed_files = new_checksums
                .iter()
                .filter(
                    |(path, checksum)| match previous_checksums.get(path.as_path()) {
                        Some(initial_checksum) => initial_checksum != *checksum,
                        None => true,
                    },
                )
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            if !changed_files.is_empty() {
                reload.reload(changed_files);
            }

            for (path, checksum) in new_checksums {
                previous_checksums.insert(path, checksum);
            }
        })?;
        for (file, recursive) in files {
            debouncer.watch(file, recursive)?;
        }

        self.debouncers.insert(name, debouncer);

        Ok(())
    }
}

pub trait Reload: Send + 'static {
    fn name(&self) -> &'static str;

    fn reload(&self, files: Vec<PathBuf>);

    fn files(&self) -> Vec<(PathBuf, RecursiveMode)>;
}

type Events = Vec<notify_debouncer_full::DebouncedEvent>;
type Errors = Vec<notify::Error>;

fn checksum_events(events: Result<Events, Errors>) -> HashMap<PathBuf, u32> {
    let mut checksums = HashMap::new();
    let Ok(events) = events else {
        return checksums;
    };

    for event in events {
        event.paths.iter().for_each(|path| {
            if checksums.contains_key(path) {
                return;
            }
            let contents = std::fs::read(path).unwrap();
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&contents);
            checksums.insert(path.to_owned(), hasher.finalize());
        });
    }

    checksums
}

fn checksum_files(
    files: &[(PathBuf, RecursiveMode)],
) -> Result<HashMap<PathBuf, u32>, std::io::Error>
{
    let mut checksums = HashMap::new();
    for (path, mode) in files {
        match mode {
            RecursiveMode::Recursive => {
                walkdir::WalkDir::new(path)
                    .into_iter()
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.file_type().is_file())
                    .for_each(|entry| {
                        let path = entry.path();
                        let contents = std::fs::read(path).unwrap();
                        let mut hasher = crc32fast::Hasher::new();
                        hasher.update(&contents);
                        checksums.insert(path.to_owned(), hasher.finalize());
                    });
            }
            RecursiveMode::NonRecursive => {
                let contents = std::fs::read(path)?;
                let mut hasher = crc32fast::Hasher::new();
                hasher.update(&contents);
                checksums.insert(path.to_owned(), hasher.finalize());
            }
        }
    }
    

    Ok(checksums)
}
