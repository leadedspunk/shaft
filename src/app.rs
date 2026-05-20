use crate::fs::{local::LocalProvider, remote::RemoteProvider, FilesystemProvider};
use crate::keybinds::{map_key, Action};
use crate::pane::Pane;
use crate::ssh::{NeedsKeyPassphrase, NeedsPassword, SshConnection, SshTarget};
use crate::transfer::{copy_entry, move_entry, SharedProgress, TransferProgress};
use anyhow::Result;
use crossterm::event::{Event, KeyCode};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub enum AppMode {
    Normal,
    Transfer(SharedProgress),
    Dialog(DialogKind),
}

#[derive(Clone)]
pub enum DialogKind {
    ConfirmDelete(String),
    MkDir,
    Rename,
    Connect,
    KeyPassphrase(String),  // key file path display string
    Password(String),       // user@host
}

pub struct App {
    pub left: Pane,
    pub right: Pane,
    pub active: usize, // 0 = left, 1 = right
    pub mode: AppMode,
    pub input_buf: String,
    pub ssh_target: Option<SshTarget>,

    left_provider: Box<dyn FilesystemProvider>,
    right_provider: Option<Box<dyn FilesystemProvider>>,
    pub should_quit: bool,
    pub visible_rows: usize,
}

impl App {
    pub fn new_local() -> Result<Self> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let mut left_prov: Box<dyn FilesystemProvider> = Box::new(LocalProvider::new());
        let mut right_prov: Box<dyn FilesystemProvider> = Box::new(LocalProvider::new());

        let mut left = Pane::new(home.clone(), "Local");
        let mut right = Pane::new(home.clone(), "Local");
        left.refresh(left_prov.as_mut());
        right.refresh(right_prov.as_mut());

        Ok(App {
            left,
            right,
            active: 0,
            mode: AppMode::Normal,
            input_buf: String::new(),
            ssh_target: None,
            left_provider: left_prov,
            right_provider: Some(right_prov),
            should_quit: false,
            visible_rows: 20,
        })
    }

    pub fn new_with_remote(mut target: SshTarget) -> Result<Self> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let mut left_prov: Box<dyn FilesystemProvider> = Box::new(LocalProvider::new());

        match SshConnection::connect(&target) {
            Ok(conn) => {
                let sftp = conn.session.sftp()?;
                let remote_home = conn.home.clone();
                let mut right_prov: Box<dyn FilesystemProvider> =
                    Box::new(RemoteProvider::new(sftp, remote_home.clone()));

                let mut left = Pane::new(home.clone(), "Local");
                let mut right = Pane::new(remote_home, format!("{}@{}", target.user, target.host));
                left.refresh(left_prov.as_mut());
                right.refresh(right_prov.as_mut());

                Ok(App {
                    left,
                    right,
                    active: 0,
                    mode: AppMode::Normal,
                    input_buf: String::new(),
                    ssh_target: Some(target),
                    left_provider: left_prov,
                    right_provider: Some(right_prov),
                    should_quit: false,
                    visible_rows: 20,
                })
            }
            Err(e) if e.downcast_ref::<NeedsKeyPassphrase>().is_some() => {
                let key_path = e.downcast_ref::<NeedsKeyPassphrase>().map(|k| k.0.clone());
                let key_display = key_path.as_ref()
                    .map(|k| k.to_string_lossy().to_string())
                    .unwrap_or_default();
                target.key_passphrase_for = key_path;
                let mut right_prov: Box<dyn FilesystemProvider> = Box::new(LocalProvider::new());
                let mut left = Pane::new(home.clone(), "Local");
                let mut right = Pane::new(home.clone(), "Local");
                left.refresh(left_prov.as_mut());
                right.refresh(right_prov.as_mut());
                Ok(App {
                    left,
                    right,
                    active: 0,
                    mode: AppMode::Dialog(DialogKind::KeyPassphrase(key_display)),
                    input_buf: String::new(),
                    ssh_target: Some(target),
                    left_provider: left_prov,
                    right_provider: Some(right_prov),
                    should_quit: false,
                    visible_rows: 20,
                })
            }
            Err(e) if e.downcast_ref::<NeedsPassword>().is_some() => {
                let mut right_prov: Box<dyn FilesystemProvider> = Box::new(LocalProvider::new());
                let mut left = Pane::new(home.clone(), "Local");
                let mut right = Pane::new(home.clone(), "Local");
                left.refresh(left_prov.as_mut());
                right.refresh(right_prov.as_mut());
                let host = format!("{}@{}", target.user, target.host);
                Ok(App {
                    left,
                    right,
                    active: 0,
                    mode: AppMode::Dialog(DialogKind::Password(host)),
                    input_buf: String::new(),
                    ssh_target: Some(target),
                    left_provider: left_prov,
                    right_provider: Some(right_prov),
                    should_quit: false,
                    visible_rows: 20,
                })
            }
            Err(e) => Err(e),
        }
    }

    pub fn connect_remote(&mut self, raw: &str) -> Result<()> {
        let target = SshTarget::parse(raw)?;
        // Match directly — do NOT propagate with ? so NeedsPassword downcast works
        match SshConnection::connect(&target) {
            Ok(conn) => self.finish_connect(conn, target),
            Err(e) if e.downcast_ref::<NeedsKeyPassphrase>().is_some() => {
                let key_path = e.downcast_ref::<NeedsKeyPassphrase>().map(|k| k.0.clone());
                let key_display = key_path.as_ref()
                    .map(|k| k.to_string_lossy().to_string())
                    .unwrap_or_default();
                let mut target = target;
                target.key_passphrase_for = key_path;
                self.ssh_target = Some(target);
                self.mode = AppMode::Dialog(DialogKind::KeyPassphrase(key_display));
                Ok(())
            }
            Err(e) if e.downcast_ref::<NeedsPassword>().is_some() => {
                let host = format!("{}@{}", target.user, target.host);
                self.ssh_target = Some(target);
                self.mode = AppMode::Dialog(DialogKind::Password(host));
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    fn finish_connect(&mut self, conn: SshConnection, target: SshTarget) -> Result<()> {
        let sftp = conn.session.sftp()?;
        let remote_home = conn.home.clone();
        let mut prov: Box<dyn FilesystemProvider> =
            Box::new(RemoteProvider::new(sftp, remote_home.clone()));

        self.right.cwd = remote_home.clone();
        self.right.label = format!("{}@{}", target.user, target.host);
        self.right.refresh(prov.as_mut());
        self.right_provider = Some(prov);
        self.ssh_target = Some(target);
        Ok(())
    }

    pub fn set_visible_rows(&mut self, rows: usize) {
        self.visible_rows = rows;
    }

    pub fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Key(key) if key.kind == crossterm::event::KeyEventKind::Press => {
                self.handle_key(key)?
            }
            Event::Resize(_, _) => {}
            _ => {}
        }
        Ok(())
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        // dialog mode intercepts keys
        if let AppMode::Dialog(ref kind) = self.mode {
            let kind = kind.clone();
            return self.handle_dialog_key(key, kind);
        }
        // transfer mode — only allow quit
        if let AppMode::Transfer(_) = self.mode {
            return Ok(());
        }

        let action = map_key(key);
        match action {
            Action::Quit => {
                self.should_quit = true;
            }
            Action::SwitchPane => {
                self.active = 1 - self.active;
            }
            Action::MoveUp => {
                self.active_pane_mut().move_up();
            }
            Action::MoveDown => {
                let rows = self.visible_rows;
                self.active_pane_mut().move_down(rows);
            }
            Action::Enter => {
                let active = self.active;
                if active == 0 {
                    self.left.enter_dir(self.left_provider.as_mut());
                } else if let Some(ref mut prov) = self.right_provider {
                    self.right.enter_dir(prov.as_mut());
                }
            }
            Action::GoUp => {
                let active = self.active;
                if active == 0 {
                    self.left.go_up(self.left_provider.as_mut());
                } else if let Some(ref mut prov) = self.right_provider {
                    self.right.go_up(prov.as_mut());
                }
            }
            Action::GoHome => {
                let active = self.active;
                if active == 0 {
                    self.left.go_home(self.left_provider.as_mut());
                } else if let Some(ref mut prov) = self.right_provider {
                    self.right.go_home(prov.as_mut());
                }
            }
            Action::ToggleSelect => {
                self.active_pane_mut().toggle_select();
            }
            Action::Copy => {
                self.start_transfer(false)?;
            }
            Action::Move => {
                self.start_transfer(true)?;
            }
            Action::MkDir => {
                self.input_buf.clear();
                self.mode = AppMode::Dialog(DialogKind::MkDir);
            }
            Action::Delete => {
                let name = self
                    .active_pane()
                    .current_entry()
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
                if !name.is_empty() {
                    self.mode = AppMode::Dialog(DialogKind::ConfirmDelete(name));
                }
            }
            Action::Rename => {
                if let Some(entry) = self.active_pane().current_entry() {
                    self.input_buf = entry.name.clone();
                    self.mode = AppMode::Dialog(DialogKind::Rename);
                }
            }
            Action::ToggleHidden => {
                let active = self.active;
                if active == 0 {
                    self.left.toggle_hidden(self.left_provider.as_mut());
                } else if let Some(ref mut prov) = self.right_provider {
                    self.right.toggle_hidden(prov.as_mut());
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_dialog_key(
        &mut self,
        key: crossterm::event::KeyEvent,
        kind: DialogKind,
    ) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.input_buf.clear();
            }
            KeyCode::Enter => {
                self.confirm_dialog(kind)?;
            }
            KeyCode::Backspace => {
                self.input_buf.pop();
            }
            KeyCode::Char(c) => {
                self.input_buf.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    fn confirm_dialog(&mut self, kind: DialogKind) -> Result<()> {
        match kind {
            DialogKind::ConfirmDelete(_) => {
                if self.input_buf.trim().to_lowercase() == "y" {
                    let path = self
                        .active_pane()
                        .current_entry()
                        .map(|e| e.path.clone());
                    if let Some(path) = path {
                        let prov = self.active_provider_mut();
                        prov.delete(&path)?;
                    }
                    self.refresh_active();
                }
                self.mode = AppMode::Normal;
                self.input_buf.clear();
            }
            DialogKind::MkDir => {
                let name = self.input_buf.trim().to_string();
                if !name.is_empty() {
                    let new_dir = self.active_pane().cwd.join(&name);
                    let prov = self.active_provider_mut();
                    prov.mkdir(&new_dir)?;
                }
                self.refresh_active();
                self.mode = AppMode::Normal;
                self.input_buf.clear();
            }
            DialogKind::Rename => {
                let new_name = self.input_buf.trim().to_string();
                if !new_name.is_empty() {
                    let old_path = self.active_pane().current_entry().map(|e| e.path.clone());
                    if let Some(old_path) = old_path {
                        let new_path = old_path
                            .parent()
                            .map(|p| p.join(&new_name))
                            .unwrap_or_else(|| PathBuf::from(&new_name));
                        let result = self.active_provider_mut().rename(&old_path, &new_path);
                        if let Err(e) = result {
                            self.active_pane_error(e.to_string());
                        }
                    }
                }
                self.refresh_active();
                self.mode = AppMode::Normal;
                self.input_buf.clear();
            }
            DialogKind::KeyPassphrase(_) => {
                let pp = self.input_buf.clone();
                self.mode = AppMode::Normal;
                self.input_buf.clear();
                if let Some(ref mut t) = self.ssh_target {
                    t.key_passphrase = Some(pp);
                }
                let target = self.ssh_target.clone();
                if let Some(target) = target {
                    if let Err(e) = self.connect_remote_with_target(target) {
                        self.right.error = Some(format!("Auth failed: {}", e));
                    }
                }
            }
            DialogKind::Connect => {
                let raw = self.input_buf.trim().to_string();
                self.mode = AppMode::Normal;
                self.input_buf.clear();
                if !raw.is_empty() {
                    if let Err(e) = self.connect_remote(&raw) {
                        self.right.error = Some(format!("Connect failed: {}", e));
                    }
                }
            }
            DialogKind::Password(_) => {
                let pw = self.input_buf.clone();
                self.mode = AppMode::Normal;
                self.input_buf.clear();
                if let Some(ref mut t) = self.ssh_target {
                    t.password = Some(pw);
                }
                let target = self.ssh_target.clone();
                if let Some(target) = target {
                    if let Err(e) = self.connect_remote_with_target(target) {
                        self.right.error = Some(format!("Auth failed: {}", e));
                    }
                }
            }
        }
        Ok(())
    }

    fn connect_remote_with_target(&mut self, target: SshTarget) -> Result<()> {
        let conn = SshConnection::connect(&target)?;
        self.finish_connect(conn, target)
    }

    fn start_transfer(&mut self, is_move: bool) -> Result<()> {
        let src_active = self.active;
        let dst_active = 1 - src_active;

        let entries: Vec<_> = self.active_pane().selected_entries().iter().map(|e| (*e).clone()).collect();
        if entries.is_empty() {
            return Ok(());
        }

        let dst_dir = if dst_active == 0 {
            self.left.cwd.clone()
        } else {
            self.right.cwd.clone()
        };

        let progress = Arc::new(Mutex::new(TransferProgress::default()));
        let prog_clone = Arc::clone(&progress);
        self.mode = AppMode::Transfer(Arc::clone(&progress));

        // We can't easily split the borrow here for async, so we do a synchronous transfer
        // and refresh after. The UI will show progress on next tick.
        // For a truly async UX a channel-based approach with a separate thread would be needed.
        let result = if src_active == 0 {
            // left -> right
            let src = self.left_provider.as_mut();
            let dst = self
                .right_provider
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("no right provider"))?;

            for entry in &entries {
                if is_move {
                    move_entry(src, dst.as_mut(), entry, &dst_dir, Arc::clone(&prog_clone))?;
                } else {
                    copy_entry(src, dst.as_mut(), entry, &dst_dir, Arc::clone(&prog_clone))?;
                }
            }
            Ok(())
        } else {
            // right -> left
            let dst = self.left_provider.as_mut();
            let src = self
                .right_provider
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("no right provider"))?;

            for entry in &entries {
                if is_move {
                    move_entry(src.as_mut(), dst, entry, &dst_dir, Arc::clone(&prog_clone))?;
                } else {
                    copy_entry(src.as_mut(), dst, entry, &dst_dir, Arc::clone(&prog_clone))?;
                }
            }
            Ok(())
        };

        self.mode = AppMode::Normal;
        self.refresh_active();
        self.refresh_inactive();

        result
    }

    pub fn refresh_active(&mut self) {
        if self.active == 0 {
            let prov = self.left_provider.as_mut();
            self.left.refresh(prov);
        } else {
            if let Some(ref mut prov) = self.right_provider {
                self.right.refresh(prov.as_mut());
            }
        }
    }

    fn refresh_inactive(&mut self) {
        if self.active == 1 {
            let prov = self.left_provider.as_mut();
            self.left.refresh(prov);
        } else {
            if let Some(ref mut prov) = self.right_provider {
                self.right.refresh(prov.as_mut());
            }
        }
    }

    fn active_pane_error(&mut self, msg: String) {
        if self.active == 0 {
            self.left.error = Some(msg);
        } else {
            self.right.error = Some(msg);
        }
    }

    fn active_pane(&self) -> &Pane {
        if self.active == 0 {
            &self.left
        } else {
            &self.right
        }
    }

    fn active_pane_mut(&mut self) -> &mut Pane {
        if self.active == 0 {
            &mut self.left
        } else {
            &mut self.right
        }
    }

    fn active_provider_mut(&mut self) -> &mut dyn FilesystemProvider {
        if self.active == 0 {
            self.left_provider.as_mut()
        } else {
            self.right_provider
                .as_mut()
                .map(|p| p.as_mut())
                .unwrap_or(self.left_provider.as_mut())
        }
    }
}
