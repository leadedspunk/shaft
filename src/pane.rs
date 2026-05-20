use crate::fs::{Entry, FilesystemProvider};
use std::collections::HashSet;
use std::path::PathBuf;

pub struct Pane {
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    pub scroll: usize,
    pub selected: HashSet<usize>,
    pub label: String,
    pub error: Option<String>,
    pub show_hidden: bool,
}

impl Pane {
    pub fn new(cwd: PathBuf, label: impl Into<String>) -> Self {
        Pane {
            cwd,
            entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            selected: HashSet::new(),
            label: label.into(),
            error: None,
            show_hidden: false,
        }
    }

    pub fn refresh(&mut self, provider: &mut dyn FilesystemProvider) {
        match provider.list_dir(&self.cwd.clone()) {
            Ok(mut entries) => {
                if !self.show_hidden {
                    entries.retain(|e| !e.name.starts_with('.'));
                }
                self.entries = entries;
                self.error = None;
                self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.entries.clear();
            }
        }
        self.selected.clear();
    }

    pub fn toggle_hidden(&mut self, provider: &mut dyn FilesystemProvider) {
        self.show_hidden = !self.show_hidden;
        self.refresh(provider);
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            if self.cursor < self.scroll {
                self.scroll = self.cursor;
            }
        }
    }

    pub fn move_down(&mut self, visible: usize) {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
            if self.cursor >= self.scroll + visible {
                self.scroll = self.cursor - visible + 1;
            }
        }
    }

    pub fn enter_dir(&mut self, provider: &mut dyn FilesystemProvider) -> bool {
        if let Some(entry) = self.entries.get(self.cursor) {
            if entry.kind == crate::fs::FileKind::Dir {
                self.cwd = entry.path.clone();
                self.cursor = 0;
                self.scroll = 0;
                self.refresh(provider);
                return true;
            }
        }
        false
    }

    pub fn go_up(&mut self, provider: &mut dyn FilesystemProvider) {
        if let Some(parent) = self.cwd.parent() {
            let prev = self.cwd.clone();
            self.cwd = parent.to_path_buf();
            self.cursor = 0;
            self.scroll = 0;
            self.refresh(provider);
            // restore cursor to the dir we came from
            if let Some(idx) = self.entries.iter().position(|e| e.path == prev) {
                self.cursor = idx;
            }
        }
    }

    pub fn go_home(&mut self, provider: &mut dyn FilesystemProvider) {
        self.cwd = provider.home_dir();
        self.cursor = 0;
        self.scroll = 0;
        self.refresh(provider);
    }

    pub fn toggle_select(&mut self) {
        if self.selected.contains(&self.cursor) {
            self.selected.remove(&self.cursor);
        } else {
            self.selected.insert(self.cursor);
        }
    }

    pub fn current_entry(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    pub fn selected_entries(&self) -> Vec<&Entry> {
        if self.selected.is_empty() {
            self.current_entry().into_iter().collect()
        } else {
            self.selected
                .iter()
                .filter_map(|&i| self.entries.get(i))
                .collect()
        }
    }
}
