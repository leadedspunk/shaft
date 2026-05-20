use crate::app::{App, AppMode, DialogKind};
use crate::fs::FileKind;
use crate::keybinds::KeyLegend;
use crate::pane::Pane;
use crate::transfer::TransferProgress;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, Paragraph,
    },
};

const ACCENT: Color = Color::Cyan;
const INACTIVE: Color = Color::DarkGray;
const DIR_COLOR: Color = Color::Blue;
const EXEC_COLOR: Color = Color::Green;
const LINK_COLOR: Color = Color::Magenta;
const FILE_COLOR: Color = Color::White;
const SELECT_BG: Color = Color::Rgb(40, 60, 80);
const ERR_COLOR: Color = Color::Red;
const TITLE_FG: Color = Color::Yellow;
const STATUS_BG: Color = Color::Rgb(20, 20, 30);
const PROGRESS_FG: Color = Color::Green;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // root layout: panes / status / legend
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(3), // status bar
            Constraint::Length(1), // key legend
        ])
        .split(area);

    // pane layout: left / right
    let pane_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    draw_pane(f, pane_chunks[0], &app.left, app.active == 0);
    draw_pane(f, pane_chunks[1], &app.right, app.active == 1);
    draw_status(f, chunks[1], app);
    draw_legend(f, chunks[2]);

    // overlays
    match &app.mode {
        AppMode::Transfer(prog) => {
            let p = prog.lock().unwrap().clone();
            draw_progress(f, area, &p);
        }
        AppMode::Dialog(kind) => {
            draw_dialog(f, area, kind, &app.input_buf);
        }
        _ => {}
    }
}

fn draw_pane(f: &mut Frame, area: Rect, pane: &Pane, active: bool) {
    let border_color = if active { ACCENT } else { INACTIVE };
    let title_style = Style::default()
        .fg(TITLE_FG)
        .add_modifier(if active { Modifier::BOLD } else { Modifier::empty() });

    let label = format!(" {} ", pane.label);
    let cwd = format!(" {} ", pane.cwd.display());

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(label, title_style),
            Span::styled(cwd, Style::default().fg(Color::Gray).add_modifier(Modifier::ITALIC)),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(Color::Rgb(10, 10, 15)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if let Some(ref err) = pane.error {
        let msg = Paragraph::new(err.as_str())
            .style(Style::default().fg(ERR_COLOR));
        f.render_widget(msg, inner);
        return;
    }

    let visible = inner.height as usize;

    let items: Vec<ListItem> = pane
        .entries
        .iter()
        .enumerate()
        .skip(pane.scroll)
        .take(visible)
        .map(|(i, e)| {
            let selected = pane.selected.contains(&i);
            let is_cursor = i == pane.cursor;

            let fg = match e.kind {
                FileKind::Dir => DIR_COLOR,
                FileKind::Executable => EXEC_COLOR,
                FileKind::Symlink => LINK_COLOR,
                _ => FILE_COLOR,
            };

            let icon = e.icon();
            let size_str = e.size_human();
            let date_str = e
                .modified
                .map(|dt| dt.format("%b %d %H:%M").to_string())
                .unwrap_or_else(|| "           ".to_string());
            // icon(2) + sp(1) + name + sp(1) + size(8) + sp(1) + date(12) = 25 fixed
            let name_w = inner.width.saturating_sub(25) as usize;
            let name_truncated = if e.name.len() > name_w {
                format!("{}…", &e.name[..name_w.saturating_sub(1)])
            } else {
                e.name.clone()
            };

            let text = format!(
                "{} {:<name_w$} {:>8} {}",
                icon,
                name_truncated,
                size_str,
                date_str,
                name_w = name_w
            );

            let mut style = Style::default().fg(fg);
            if selected {
                style = style.bg(SELECT_BG).add_modifier(Modifier::BOLD);
            }
            if is_cursor {
                style = style
                    .bg(if active { ACCENT } else { Color::Rgb(40, 40, 50) })
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD);
            }

            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let active_pane = if app.active == 0 { &app.left } else { &app.right };

    let conn_info = if let Some(ref t) = app.ssh_target {
        format!("  {}@{}:{}", t.user, t.host, t.port)
    } else {
        "  LOCAL".to_string()
    };

    let file_info = active_pane
        .current_entry()
        .map(|e| format!("  {} ({}) ", e.name, e.size_human()))
        .unwrap_or_else(|| "  No selection ".to_string());

    let sel_count = active_pane.selected.len();
    let sel_info = if sel_count > 0 {
        format!("  {} selected", sel_count)
    } else {
        String::new()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(INACTIVE))
        .style(Style::default().bg(STATUS_BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = Line::from(vec![
        Span::styled(conn_info, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::raw(" │"),
        Span::styled(file_info, Style::default().fg(Color::White)),
        Span::styled(sel_info, Style::default().fg(Color::Yellow)),
    ]);

    f.render_widget(Paragraph::new(text), inner);
}

fn draw_legend(f: &mut Frame, area: Rect) {
    let spans: Vec<Span> = KeyLegend::entries()
        .iter()
        .flat_map(|(key, desc)| {
            vec![
                Span::styled(
                    format!(" {} ", key),
                    Style::default()
                        .bg(Color::Rgb(50, 50, 80))
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{} ", desc),
                    Style::default().fg(Color::DarkGray),
                ),
            ]
        })
        .collect();

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_progress(f: &mut Frame, area: Rect, p: &TransferProgress) {
    let popup = centered_rect(60, 6, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Transferring ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(Color::Rgb(10, 10, 20)));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    let name_line = Paragraph::new(format!(" {}", p.file_name))
        .style(Style::default().fg(Color::White));
    f.render_widget(name_line, chunks[0]);

    use humansize::{format_size, BINARY};
    let done_str = format_size(p.bytes_done, BINARY);
    let total_str = format_size(p.bytes_total, BINARY);
    let speed = p.speed_human();
    let eta = p
        .eta_secs()
        .map(|s| format!("ETA {}s", s))
        .unwrap_or_default();

    let info = Paragraph::new(format!(" {} / {}  {}  {}", done_str, total_str, speed, eta))
        .style(Style::default().fg(Color::Gray));
    f.render_widget(info, chunks[1]);

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(PROGRESS_FG).bg(Color::Rgb(20, 20, 20)))
        .percent(p.percent());
    f.render_widget(gauge, chunks[2]);
}

fn draw_dialog(f: &mut Frame, area: Rect, kind: &DialogKind, input: &str) {
    let (title, prompt, masked): (String, String, bool) = match kind {
        DialogKind::ConfirmDelete(name) => (
            " Confirm Delete ".to_string(),
            format!("Delete \"{}\"? (y/n)", name),
            false,
        ),
        DialogKind::MkDir => (" New Directory ".to_string(), "Name: ".to_string(), false),
        DialogKind::Rename => (" Rename ".to_string(), "New name: ".to_string(), false),
        DialogKind::KeyPassphrase(key) => (
            " Key Passphrase Required ".to_string(),
            format!("Passphrase for {}: ", key),
            true,
        ),
        DialogKind::Connect => (
            " Connect to SSH Host ".to_string(),
            "host, user@host, or ssh alias: ".to_string(),
            false,
        ),
        DialogKind::Password(host) => (
            " Authentication Required ".to_string(),
            format!("Password or passphrase for {}: ", host),
            true,
        ),
    };

    let height = 5u16;
    let popup = centered_rect(50, height, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Rgb(15, 15, 25)));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let display_input = if masked {
        "*".repeat(input.len())
    } else {
        input.to_string()
    };

    let text = if matches!(
        kind,
        DialogKind::MkDir
            | DialogKind::Rename
            | DialogKind::Connect
            | DialogKind::KeyPassphrase(_)
            | DialogKind::Password(_)
    ) {
        format!("{}{}_", prompt, display_input)
    } else {
        prompt.to_string()
    };

    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::White)),
        inner,
    );
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}
