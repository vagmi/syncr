use anyhow::Result;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

pub struct FileWatcher {
    watcher: RecommendedWatcher,
    rx: mpsc::Receiver<Result<PathBuf>>,
}

impl FileWatcher {
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel(100);

        let watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Filter for Modify, Create, Remove?
                        // For now, just send the first path affected
                        if let Some(path) = event.paths.first() {
                            let _ = tx.blocking_send(Ok(path.clone()));
                        }
                    }
                    Err(e) => {
                        let _ = tx.blocking_send(Err(anyhow::anyhow!("Watch error: {}", e)));
                    }
                }
            },
            Config::default(),
        )?;

        Ok(Self { watcher, rx })
    }

    pub fn watch(&mut self, path: &Path) -> Result<()> {
        self.watcher.watch(path, RecursiveMode::Recursive)?;
        Ok(())
    }

    pub fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.watcher.unwatch(path)?;
        Ok(())
    }

    pub async fn next_event(&mut self) -> Option<Result<PathBuf>> {
        self.rx.recv().await
    }
}
