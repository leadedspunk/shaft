mod app;
mod fs;
mod keybinds;
mod pane;
mod ssh;
mod transfer;
mod ui;

use anyhow::Result;
use app::{App, AppMode, DialogKind};
use crossterm::{
    event::{self},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use ssh::SshTarget;
use std::{io, time::Duration};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut app = if args.is_empty() {
        let mut a = App::new_local()?;
        a.mode = AppMode::Dialog(DialogKind::Connect);
        a
    } else {
        let raw = &args[0];
        match App::new_with_remote(SshTarget::parse(raw)?) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Connection failed: {}. Starting in local mode.", e);
                App::new_local()?
            }
        }
    };

    run_tui(&mut app)
}

fn run_tui(app: &mut App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| {
            let rows = f.area().height.saturating_sub(5) as usize;
            app.set_visible_rows(rows);
            ui::draw(f, app);
        })?;

        if event::poll(Duration::from_millis(50))? {
            let ev = event::read()?;
            app.handle_event(ev)?;
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
