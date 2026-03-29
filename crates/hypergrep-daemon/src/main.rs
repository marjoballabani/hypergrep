use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::RwLock;
use tracing::{error, info};

use hypergrep_core::index::Index;

mod watcher;

#[derive(Parser)]
#[command(name = "hypergrep-daemon", about = "Hypergrep persistent index daemon")]
struct Cli {
    /// Root directory to index and watch
    #[arg(default_value = ".")]
    root: PathBuf,

    /// Unix socket path (auto-generated if not specified)
    #[arg(long)]
    socket: Option<PathBuf>,
}

/// Shared daemon state.
struct DaemonState {
    index: RwLock<Index>,
    root: PathBuf,
}

#[derive(serde::Deserialize)]
struct SearchRequest {
    pattern: String,
}

#[derive(serde::Serialize)]
struct SearchResponse {
    matches: Vec<MatchResult>,
    elapsed_us: u64,
}

#[derive(serde::Serialize)]
struct MatchResult {
    path: String,
    line_number: usize,
    line: String,
    match_start: usize,
    match_end: usize,
}

fn socket_path(root: &PathBuf) -> PathBuf {
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        root.hash(&mut hasher);
        hasher.finish()
    };

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/tmp".to_string());

    PathBuf::from(runtime_dir).join(format!("hypergrep-{:x}.sock", hash))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("hypergrep=info")
        .init();

    let cli = Cli::parse();
    let root = std::fs::canonicalize(&cli.root)?;

    info!("Building index for {}", root.display());
    let index = Index::build(&root)?;
    info!(
        "Index ready: {} files, {} trigrams",
        index.file_count(),
        index.trigram_count()
    );

    let state = Arc::new(DaemonState {
        index: RwLock::new(index),
        root: root.clone(),
    });

    // Start filesystem watcher
    let watcher_state = Arc::clone(&state);
    tokio::spawn(async move {
        if let Err(e) = watcher::watch(watcher_state).await {
            error!("Filesystem watcher error: {}", e);
        }
    });

    // Listen on Unix socket
    let sock_path = cli.socket.unwrap_or_else(|| socket_path(&root));

    // Clean up stale socket
    let _ = std::fs::remove_file(&sock_path);

    let listener = UnixListener::bind(&sock_path)?;
    info!("Listening on {}", sock_path.display());

    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {}
                    Err(e) => {
                        error!("Read error: {}", e);
                        break;
                    }
                }

                let request: SearchRequest = match serde_json::from_str(&line) {
                    Ok(r) => r,
                    Err(e) => {
                        let err = format!("{{\"error\": \"{}\"}}\n", e);
                        let _ = writer.write_all(err.as_bytes()).await;
                        continue;
                    }
                };

                let start = std::time::Instant::now();
                let index = state.index.read().await;
                let search_result = index.search(&request.pattern);
                drop(index);

                let response = match search_result {
                    Ok(matches) => SearchResponse {
                        matches: matches
                            .into_iter()
                            .map(|m| MatchResult {
                                path: m.path.display().to_string(),
                                line_number: m.line_number,
                                line: m.line,
                                match_start: m.match_start,
                                match_end: m.match_end,
                            })
                            .collect(),
                        elapsed_us: start.elapsed().as_micros() as u64,
                    },
                    Err(e) => {
                        let err = format!("{{\"error\": \"{}\"}}\n", e);
                        let _ = writer.write_all(err.as_bytes()).await;
                        continue;
                    }
                };

                let json = serde_json::to_string(&response).unwrap() + "\n";
                let _ = writer.write_all(json.as_bytes()).await;
            }
        });
    }
}
