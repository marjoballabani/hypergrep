/// Parallel directory traversal with .gitignore filtering and binary detection.
use std::path::{Path, PathBuf};

use anyhow::Result;
use ignore::WalkBuilder;
use rayon::prelude::*;

use crate::trigram;

/// A file that has been read and is ready for indexing.
pub struct IndexableFile {
    pub doc_id: u32,
    pub path: PathBuf,
    pub content: Vec<u8>,
}

/// Walk a directory, read all text files in parallel, return them with assigned doc IDs.
/// Respects .gitignore, skips binary files, skips hidden files.
pub fn walk_and_read(root: &Path) -> Result<Vec<IndexableFile>> {
    // Collect paths first (ignore crate's walker is not Send-friendly for rayon)
    let paths: Vec<PathBuf> = WalkBuilder::new(root)
        .hidden(true) // skip hidden files/dirs
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            // Skip .hypergrep directory
            if let Some(name) = entry.file_name().to_str()
                && name == ".hypergrep"
            {
                return false;
            }
            true
        })
        .build()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type()?.is_file() {
                Some(entry.into_path())
            } else {
                None
            }
        })
        .collect();

    // Read files in parallel and filter binaries
    let files: Vec<IndexableFile> = paths
        .into_par_iter()
        .filter_map(|path| {
            let content = std::fs::read(&path).ok()?;
            if trigram::is_binary(&content) {
                return None;
            }
            Some((path, content))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .enumerate()
        .map(|(i, (path, content))| IndexableFile {
            doc_id: i as u32,
            path,
            content,
        })
        .collect();

    Ok(files)
}
