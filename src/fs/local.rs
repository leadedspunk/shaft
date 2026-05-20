use super::{Entry, FileKind, FilesystemProvider};
use anyhow::{Context, Result};
use std::path::PathBuf;

pub struct LocalProvider;

impl LocalProvider {
    pub fn new() -> Self {
        LocalProvider
    }
}

impl FilesystemProvider for LocalProvider {
    fn list_dir(&mut self, path: &PathBuf) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();
        let read = std::fs::read_dir(path)
            .with_context(|| format!("read_dir {}", path.display()))?;

        for item in read {
            let item = item?;
            let meta = item.metadata()?;
            let file_type = item.file_type()?;

            let kind = if file_type.is_symlink() {
                FileKind::Symlink
            } else if file_type.is_dir() {
                FileKind::Dir
            } else if is_executable(&meta, item.path()) {
                FileKind::Executable
            } else {
                FileKind::File
            };

            let modified = meta
                .modified()
                .ok()
                .and_then(|t| {
                    let dt: chrono::DateTime<chrono::Local> = t.into();
                    Some(dt)
                });

            entries.push(Entry {
                name: item.file_name().to_string_lossy().to_string(),
                path: item.path(),
                kind,
                size: meta.len(),
                modified,
            });
        }

        entries.sort_by(|a, b| {
            let da = a.kind == FileKind::Dir;
            let db = b.kind == FileKind::Dir;
            db.cmp(&da).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(entries)
    }

    fn mkdir(&mut self, path: &PathBuf) -> Result<()> {
        std::fs::create_dir_all(path)?;
        Ok(())
    }

    fn delete(&mut self, path: &PathBuf) -> Result<()> {
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    fn rename(&mut self, from: &PathBuf, to: &PathBuf) -> Result<()> {
        std::fs::rename(from, to)?;
        Ok(())
    }

    fn read_file(&mut self, path: &PathBuf) -> Result<Vec<u8>> {
        Ok(std::fs::read(path)?)
    }

    fn write_file(&mut self, path: &PathBuf, data: &[u8]) -> Result<()> {
        std::fs::write(path, data)?;
        Ok(())
    }

    fn home_dir(&self) -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
    }
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata, _path: std::path::PathBuf) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(windows)]
fn is_executable(_meta: &std::fs::Metadata, path: std::path::PathBuf) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("exe" | "bat" | "cmd" | "com")
    )
}

#[cfg(not(any(unix, windows)))]
fn is_executable(_meta: &std::fs::Metadata, _path: std::path::PathBuf) -> bool {
    false
}
