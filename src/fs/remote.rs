use super::{Entry, FileKind, FilesystemProvider};
use anyhow::{Context, Result};
use ssh2::Sftp;
use std::io::{Read, Write};
use std::path::PathBuf;

pub struct RemoteProvider {
    sftp: Sftp,
    home: PathBuf,
}

impl RemoteProvider {
    pub fn new(sftp: Sftp, home: PathBuf) -> Self {
        RemoteProvider { sftp, home }
    }
}

impl FilesystemProvider for RemoteProvider {
    fn list_dir(&mut self, path: &PathBuf) -> Result<Vec<Entry>> {
        let items = self
            .sftp
            .readdir(path)
            .with_context(|| format!("sftp readdir {}", path.display()))?;

        let mut entries: Vec<Entry> = items
            .into_iter()
            .map(|(p, stat)| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                let kind = if stat.is_dir() {
                    FileKind::Dir
                } else if let Some(perm) = stat.perm {
                    if perm & 0o111 != 0 {
                        FileKind::Executable
                    } else {
                        FileKind::File
                    }
                } else {
                    FileKind::File
                };

                let size = stat.size.unwrap_or(0);
                let modified = stat.mtime.and_then(|t| {
                    let secs = t as i64;
                    chrono::DateTime::from_timestamp(secs, 0)
                        .map(|utc| utc.with_timezone(&chrono::Local))
                });

                Entry {
                    name,
                    path: p,
                    kind,
                    size,
                    modified,
                }
            })
            .filter(|e| e.name != "." && e.name != "..")
            .collect();

        entries.sort_by(|a, b| {
            let da = a.kind == FileKind::Dir;
            let db = b.kind == FileKind::Dir;
            db.cmp(&da).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(entries)
    }

    fn mkdir(&mut self, path: &PathBuf) -> Result<()> {
        self.sftp.mkdir(path, 0o755)?;
        Ok(())
    }

    fn delete(&mut self, path: &PathBuf) -> Result<()> {
        let stat = self
            .sftp
            .stat(path)
            .with_context(|| format!("stat {}", path.display()))?;

        if stat.is_dir() {
            let children = self
                .sftp
                .readdir(path)
                .with_context(|| format!("readdir {}", path.display()))?;
            for (child_path, _) in children {
                let name = child_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if name == "." || name == ".." {
                    continue;
                }
                self.delete(&child_path)?;
            }
            self.sftp.rmdir(path)?;
        } else {
            self.sftp.unlink(path)?;
        }
        Ok(())
    }

    fn rename(&mut self, from: &PathBuf, to: &PathBuf) -> Result<()> {
        self.sftp.rename(from, to, None)?;
        Ok(())
    }

    fn read_file(&mut self, path: &PathBuf) -> Result<Vec<u8>> {
        let mut file = self.sftp.open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }

    fn write_file(&mut self, path: &PathBuf, data: &[u8]) -> Result<()> {
        let mut file = self.sftp.create(path)?;
        file.write_all(data)?;
        Ok(())
    }

    fn home_dir(&self) -> PathBuf {
        self.home.clone()
    }
}
