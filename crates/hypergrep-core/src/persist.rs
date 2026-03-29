/// Disk persistence for the trigram index.
///
/// Saves/loads the posting lists and file table to `.hypergrep/` directory.
/// On subsequent runs, loads the cached index and only re-indexes changed files
/// (detected by mtime comparison).
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;
use tracing::info;

use crate::index::FileEntry;
use crate::trigram::Trigram;

const INDEX_VERSION: u32 = 1;

/// Serializable index data.
#[derive(bincode::Encode, bincode::Decode)]
struct PersistedIndex {
    version: u32,
    files: Vec<PersistedFile>,
    /// Flat representation: (trigram, doc_ids)
    postings: Vec<(Trigram, Vec<u32>)>,
    /// Bloom filter bits
    bloom_bits: Vec<u64>,
    bloom_num_bits: usize,
    bloom_num_hashes: u32,
    bloom_items: usize,
}

#[derive(bincode::Encode, bincode::Decode)]
struct PersistedFile {
    path: String,
    mtime_secs: u64,
    mtime_nanos: u32,
    size: u64,
}

/// Directory where index files are stored.
fn index_dir(root: &Path) -> PathBuf {
    root.join(".hypergrep")
}

fn index_file(root: &Path) -> PathBuf {
    index_dir(root).join("index.bin")
}

/// Save index to disk.
pub fn save(
    root: &Path,
    files: &[FileEntry],
    posting_lists: &HashMap<Trigram, Vec<u32>>,
    bloom: &crate::bloom::BloomFilter,
) -> Result<()> {
    let dir = index_dir(root);
    std::fs::create_dir_all(&dir)?;

    // Add .hypergrep to .gitignore if not already there
    let gitignore = root.join(".gitignore");
    if gitignore.exists() {
        let content = std::fs::read_to_string(&gitignore).unwrap_or_default();
        if !content.contains(".hypergrep") {
            std::fs::write(&gitignore, format!("{}\n.hypergrep/\n", content.trim_end()))?;
        }
    } else {
        std::fs::write(&gitignore, ".hypergrep/\n")?;
    }

    let persisted = PersistedIndex {
        version: INDEX_VERSION,
        files: files
            .iter()
            .map(|f| {
                let (secs, nanos) = mtime_to_parts(&f.mtime);
                PersistedFile {
                    path: f.path.display().to_string(),
                    mtime_secs: secs,
                    mtime_nanos: nanos,
                    size: f.size,
                }
            })
            .collect(),
        postings: posting_lists.iter().map(|(k, v)| (*k, v.clone())).collect(),
        bloom_bits: bloom.bits().to_vec(),
        bloom_num_bits: bloom.num_bits(),
        bloom_num_hashes: bloom.num_hashes(),
        bloom_items: bloom.len(),
    };

    let encoded = bincode::encode_to_vec(&persisted, bincode::config::standard())?;
    std::fs::write(index_file(root), &encoded)?;

    info!(
        "Index saved: {} bytes ({} files, {} trigrams)",
        encoded.len(),
        files.len(),
        posting_lists.len()
    );

    Ok(())
}

/// Try to load a cached index. Returns None if cache is missing or invalid.
/// Returns the loaded data plus a list of doc_ids that need re-indexing (stale files).
pub fn load(
    root: &Path,
) -> Option<(
    Vec<FileEntry>,
    HashMap<Trigram, Vec<u32>>,
    crate::bloom::BloomFilter,
    Vec<u32>, // stale doc_ids
)> {
    let path = index_file(root);
    let data = std::fs::read(&path).ok()?;

    let (persisted, _): (PersistedIndex, _) =
        bincode::decode_from_slice(&data, bincode::config::standard()).ok()?;

    if persisted.version != INDEX_VERSION {
        info!("Index version mismatch, rebuilding");
        return None;
    }

    let mut files = Vec::with_capacity(persisted.files.len());
    let mut stale = Vec::new();

    for (i, pf) in persisted.files.iter().enumerate() {
        let path = PathBuf::from(&pf.path);
        let mtime = parts_to_mtime(pf.mtime_secs, pf.mtime_nanos);

        // Check if file still exists and hasn't changed
        match std::fs::metadata(&path) {
            Ok(meta) => {
                let current_mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let current_size = meta.len();
                if current_mtime != mtime || current_size != pf.size {
                    stale.push(i as u32);
                }
            }
            Err(_) => {
                stale.push(i as u32); // file deleted
            }
        }

        files.push(FileEntry {
            path,
            mtime,
            size: pf.size,
        });
    }

    let posting_lists: HashMap<Trigram, Vec<u32>> = persisted.postings.into_iter().collect();

    let bloom = crate::bloom::BloomFilter::from_raw(
        persisted.bloom_bits,
        persisted.bloom_num_bits,
        persisted.bloom_num_hashes,
        persisted.bloom_items,
    );

    info!(
        "Index loaded: {} files, {} trigrams, {} stale",
        files.len(),
        posting_lists.len(),
        stale.len()
    );

    Some((files, posting_lists, bloom, stale))
}

fn mtime_to_parts(mtime: &SystemTime) -> (u64, u32) {
    match mtime.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => (d.as_secs(), d.subsec_nanos()),
        Err(_) => (0, 0),
    }
}

fn parts_to_mtime(secs: u64, nanos: u32) -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::new(secs, nanos)
}
