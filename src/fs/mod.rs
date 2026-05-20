pub mod local;
pub mod remote;

use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum FileKind {
    Dir,
    File,
    Symlink,
    Executable,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: FileKind,
    pub size: u64,
    pub modified: Option<chrono::DateTime<chrono::Local>>,
}

impl Entry {
    pub fn icon(&self) -> &'static str {
        match self.kind {
            FileKind::Dir => "\u{f07b}",        // nf-fa-folder
            FileKind::Symlink => "\u{f0c1}",    // nf-fa-link
            FileKind::Executable => "\u{f489}", // nf-dev-terminal
            FileKind::File => "\u{f15b}",       // nf-fa-file
            FileKind::Unknown => "\u{f128}",    // nf-fa-question
        }
    }

    pub fn size_human(&self) -> String {
        if self.kind == FileKind::Dir {
            return "-".to_string();
        }
        use humansize::{format_size, BINARY};
        format_size(self.size, BINARY)
    }
}

pub trait FilesystemProvider: Send + Sync {
    fn list_dir(&mut self, path: &PathBuf) -> Result<Vec<Entry>>;
    fn mkdir(&mut self, path: &PathBuf) -> Result<()>;
    fn delete(&mut self, path: &PathBuf) -> Result<()>;
    fn rename(&mut self, from: &PathBuf, to: &PathBuf) -> Result<()>;
    fn read_file(&mut self, path: &PathBuf) -> Result<Vec<u8>>;
    fn write_file(&mut self, path: &PathBuf, data: &[u8]) -> Result<()>;
    fn home_dir(&self) -> PathBuf;
}
