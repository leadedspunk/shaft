use crate::fs::{Entry, FileKind, FilesystemProvider};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct TransferProgress {
    pub file_name: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub speed_bps: f64,
    pub done: bool,
    pub error: Option<String>,
}

impl TransferProgress {
    pub fn percent(&self) -> u16 {
        if self.bytes_total == 0 {
            return 0;
        }
        ((self.bytes_done as f64 / self.bytes_total as f64) * 100.0) as u16
    }

    pub fn eta_secs(&self) -> Option<u64> {
        if self.speed_bps < 1.0 || self.bytes_total == 0 {
            return None;
        }
        let remaining = self.bytes_total.saturating_sub(self.bytes_done);
        Some((remaining as f64 / self.speed_bps) as u64)
    }

    pub fn speed_human(&self) -> String {
        use humansize::{format_size, BINARY};
        format!("{}/s", format_size(self.speed_bps as u64, BINARY))
    }
}

pub type SharedProgress = Arc<Mutex<TransferProgress>>;


pub fn copy_entry(
    src: &mut dyn FilesystemProvider,
    dst: &mut dyn FilesystemProvider,
    entry: &Entry,
    dst_dir: &PathBuf,
    progress: SharedProgress,
) -> Result<()> {
    let dst_path = dst_dir.join(&entry.name);

    if entry.kind == FileKind::Dir {
        dst.mkdir(&dst_path)?;
        let children = src.list_dir(&entry.path)?;
        for child in children {
            copy_entry(src, dst, &child, &dst_path, Arc::clone(&progress))?;
        }
        return Ok(());
    }

    {
        let mut p = progress.lock().unwrap();
        p.file_name = entry.name.clone();
        p.bytes_total = entry.size;
        p.bytes_done = 0;
        p.done = false;
        p.error = None;
    }

    let data = src.read_file(&entry.path)?;
    let total = data.len() as u64;

    let start = std::time::Instant::now();
    dst.write_file(&dst_path, &data)?;
    let written = total;

    let elapsed = start.elapsed().as_secs_f64().max(0.001);
    let mut p = progress.lock().unwrap();
    p.bytes_done = written;
    p.bytes_total = total;
    p.speed_bps = written as f64 / elapsed;
    p.done = true;

    Ok(())
}

pub fn move_entry(
    src: &mut dyn FilesystemProvider,
    dst: &mut dyn FilesystemProvider,
    entry: &Entry,
    dst_dir: &PathBuf,
    progress: SharedProgress,
) -> Result<()> {
    copy_entry(src, dst, entry, dst_dir, progress)?;
    src.delete(&entry.path)?;
    Ok(())
}
