/// Filesystem watcher for incremental re-indexing.
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::DaemonState;

pub async fn watch(state: Arc<DaemonState>) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<Event>(256);

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = tx.blocking_send(event);
        }
    })?;

    watcher.watch(&state.root, RecursiveMode::Recursive)?;
    info!("Watching {} for changes", state.root.display());

    // Debounce: collect events for 500ms before processing
    let mut pending_paths = std::collections::HashSet::new();

    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                        for path in event.paths {
                            if path.is_file() || !path.exists() {
                                // Skip .hypergrep and .git directories
                                let path_str = path.display().to_string();
                                if path_str.contains(".hypergrep") || path_str.contains(".git/") {
                                    continue;
                                }
                                pending_paths.insert(path);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(500)), if !pending_paths.is_empty() => {
                let paths: Vec<_> = pending_paths.drain().collect();
                let count = paths.len();

                let mut index = state.index.write().await;
                for path in paths {
                    debug!("Re-indexing: {}", path.display());
                    if let Err(e) = index.update_file(&path, &state.root) {
                        warn!("Failed to re-index {}: {}", path.display(), e);
                    }
                }
                drop(index);

                info!("Incrementally re-indexed {} files", count);
            }
        }
    }
}
