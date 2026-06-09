use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::app::{App, AppMode, Focus};

pub fn render(f: &mut Frame, app: &App) {
    let size = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(size);

    render_header(f, chunks[0], app);
    match app.mode {
        AppMode::Log => render_log_mode(f, chunks[1], app),
        AppMode::Branches => render_branches_mode(f, chunks[1], app),
    }
    render_footer(f, chunks[2], app);
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let mode_label = match app.mode {
        AppMode::Log => "log",
        AppMode::Branches => "branches",
    };
    let title = format!(
        " noa v{}  |  {}  |  branch: {}  |  {} snapshots",
        env!("CARGO_PKG_VERSION"),
        mode_label,
        app.current_branch,
        app.snapshots.len(),
    );
    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan).bold())
        .on_dark_gray();
    f.render_widget(header, area);
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![
        Span::styled(" q/Esc", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" quit "),
        Span::styled(" Tab", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" panel "),
        Span::styled(" j/k", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" scroll "),
        Span::styled(" Ctrl+B", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" toggle mode "),
    ];
    if app.mode == AppMode::Log {
        spans.push(Span::styled(
            " Enter",
            Style::default().fg(Color::Yellow).bold(),
        ));
        spans.push(Span::raw(" detail "));
    }
    let help = Line::from(spans);
    let footer = Paragraph::new(help).style(Style::default().on_dark_gray());
    f.render_widget(footer, area);
}

fn render_log_mode(f: &mut Frame, area: Rect, app: &App) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    render_log(f, columns[0], app);
    render_detail(f, columns[1], app);
}

fn render_branches_mode(f: &mut Frame, area: Rect, app: &App) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(45),
            Constraint::Percentage(30),
        ])
        .split(area);

    render_branches(f, columns[0], app);
    render_log(f, columns[1], app);
    render_detail(f, columns[2], app);
}

fn render_branches(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Branches;
    let border_style = if focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" Branches ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let items: Vec<ListItem> = app
        .branches
        .iter()
        .map(|ws| {
            let marker = if ws.name == app.current_branch {
                "* "
            } else {
                "  "
            };
            let style = if ws.name == app.current_branch {
                Style::default().fg(Color::Green).bold()
            } else {
                Style::default()
            };
            let agent = ws
                .agent_id
                .as_deref()
                .map(|a| format!(" ({})", a))
                .unwrap_or_default();
            ListItem::new(Line::from(Span::styled(
                format!("{}{}{}", marker, ws.name, agent),
                style,
            )))
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default().bg(Color::DarkGray).bold());
    let mut state = ListState::default();
    if focused {
        if let Some(idx) = app.branch_scroll.selected_index() {
            state.select(Some(idx));
        }
    }

    let list = List::new(items).highlight_style(Style::default().bg(Color::DarkGray).bold());
    let mut state = ListState::default();
    if focused {
        if let Some(idx) = app.branch_scroll.selected_index() {
            state.select(Some(idx));
        }
    }
    f.render_stateful_widget(list, inner, &mut state);
}

fn render_log(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Log;
    let border_style = if focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" Log ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let display: Vec<&crate::snapshot::Snapshot> = app.snapshots.iter().rev().collect();
    let items: Vec<ListItem> = display
        .iter()
        .map(|snap| {
            let msg = if snap.message.chars().count() > 35 {
                let truncated: String = snap.message.chars().take(32).collect();
                format!("{}...", truncated)
            } else {
                snap.message.clone()
            };
            let id_display: String = snap.id.0.chars().take(12).collect();
            let id_short = if snap.id.0.chars().count() > 12 {
                id_display
            } else {
                snap.id.0.clone()
            };
            let ts = chrono::DateTime::from_timestamp(snap.timestamp as i64 / 1_000_000, 0)
                .map(|dt| dt.format("%m/%d %H:%M").to_string())
                .unwrap_or_default();

            let line = Line::from(vec![
                Span::styled(
                    format!("{:<12} ", id_short),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{:<8} ", snap.workspace),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(format!("{:<10} ", ts), Style::default().fg(Color::DarkGray)),
                Span::raw(msg),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).highlight_style(Style::default().bg(Color::DarkGray).bold());
    let mut state = ListState::default();
    if focused {
        if let Some(idx) = app.log_scroll.selected_index() {
            let display_idx = app.snapshots.len().saturating_sub(1).saturating_sub(idx);
            state.select(Some(display_idx.min(display.len().saturating_sub(1))));
        }
    }
    f.render_stateful_widget(list, inner, &mut state);
}

fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Detail;
    let border_style = if focused {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(" Detail ")
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let snap = match app.selected_snapshot() {
        Some(s) => s,
        None => {
            let hint = Paragraph::new("Select a snapshot to view details")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(hint, inner);
            return;
        }
    };

    let ts = chrono::DateTime::from_timestamp(snap.timestamp as i64 / 1_000_000, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let parents: String = snap
        .parents
        .iter()
        .map(|p| {
            if p.0.chars().count() > 12 {
                let truncated: String = p.0.chars().take(12).collect();
                format!("{}...", truncated)
            } else {
                p.0.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    let lines = vec![
        Line::from(vec![
            Span::styled("ID:       ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&snap.id.0),
        ]),
        Line::from(vec![
            Span::styled("Author:   ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&snap.author),
        ]),
        Line::from(vec![
            Span::styled("Workspace:", Style::default().fg(Color::Yellow).bold()),
            Span::raw(format!(" {}", snap.workspace)),
        ]),
        Line::from(vec![
            Span::styled("Date:     ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&ts),
        ]),
        Line::from(vec![
            Span::styled("Tree:     ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(if snap.tree_hash.chars().count() > 20 {
                let truncated: String = snap.tree_hash.chars().take(20).collect();
                format!("{}...", truncated)
            } else {
                snap.tree_hash.clone()
            }),
        ]),
        Line::from(vec![
            Span::styled("Parents:  ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(if parents.is_empty() {
                "none".to_string()
            } else {
                parents
            }),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Message:",
            Style::default().fg(Color::Yellow).bold(),
        )]),
        Line::from(format!("  {}", snap.message)),
    ];

    let detail = Paragraph::new(lines).wrap(Wrap { trim: true });
    f.render_widget(detail, inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{Snapshot, SnapshotId};
    use crate::tui::virtual_scroll::VirtualScroll;

    fn make_snap(id: &str, ws: &str, msg: &str, ts: u64) -> Snapshot {
        Snapshot {
            id: SnapshotId(id.to_string()),
            tree_hash: format!("tree_{}", id),
            parents: vec![],
            workspace: ws.to_string(),
            author: "test".to_string(),
            timestamp: ts,
            message: msg.to_string(),
        }
    }

    fn make_log_app() -> App {
        let snapshots = vec![
            make_snap("noa_s1", "default", "initial commit", 1_000_000_000_000_000),
            make_snap(
                "noa_s2",
                "feature",
                "add feature module",
                1_000_001_000_000_000,
            ),
        ];
        App {
            mode: AppMode::Log,
            focus: Focus::Log,
            branches: vec![],
            snapshots,
            current_branch: "default".to_string(),
            branch_scroll: VirtualScroll::new(0),
            log_scroll: VirtualScroll::new(2),
            should_quit: false,
        }
    }

    fn make_branches_app() -> App {
        let branches = vec![
            crate::workspace::Workspace {
                name: "default".to_string(),
                head: SnapshotId("noa_s1".to_string()),
                base: SnapshotId("noa_s1".to_string()),
                agent_id: None,
                last_seq: 0,
                created_at: 1000,
                updated_at: 1000,
            },
            crate::workspace::Workspace {
                name: "feature".to_string(),
                head: SnapshotId("noa_s2".to_string()),
                base: SnapshotId("noa_s1".to_string()),
                agent_id: Some("agent-001".to_string()),
                last_seq: 0,
                created_at: 2000,
                updated_at: 2000,
            },
        ];
        let snapshots = vec![
            make_snap("noa_s1", "default", "initial commit", 1_000_000_000_000_000),
            make_snap("noa_s2", "feature", "add feature", 1_000_001_000_000_000),
        ];
        App {
            mode: AppMode::Branches,
            focus: Focus::Branches,
            branches,
            snapshots,
            current_branch: "default".to_string(),
            branch_scroll: VirtualScroll::new(2),
            log_scroll: VirtualScroll::new(2),
            should_quit: false,
        }
    }

    #[test]
    fn test_log_mode_80x24() {
        let app = make_log_app();
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        assert!(!terminal.backend().buffer().content.is_empty());
    }

    #[test]
    fn test_branches_mode_80x24() {
        let app = make_branches_app();
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        assert!(!terminal.backend().buffer().content.is_empty());
    }

    #[test]
    fn test_log_mode_deterministic() {
        let app = make_log_app();
        let b1 = ratatui::backend::TestBackend::new(80, 24);
        let mut t1 = ratatui::Terminal::new(b1).unwrap();
        t1.draw(|f| render(f, &app)).unwrap();
        let buf1 = t1.backend().buffer().clone();

        let b2 = ratatui::backend::TestBackend::new(80, 24);
        let mut t2 = ratatui::Terminal::new(b2).unwrap();
        t2.draw(|f| render(f, &app)).unwrap();
        let buf2 = t2.backend().buffer().clone();

        assert_eq!(buf1, buf2);
    }

    #[test]
    fn test_branches_mode_deterministic() {
        let app = make_branches_app();
        let b1 = ratatui::backend::TestBackend::new(80, 24);
        let mut t1 = ratatui::Terminal::new(b1).unwrap();
        t1.draw(|f| render(f, &app)).unwrap();
        let buf1 = t1.backend().buffer().clone();

        let b2 = ratatui::backend::TestBackend::new(80, 24);
        let mut t2 = ratatui::Terminal::new(b2).unwrap();
        t2.draw(|f| render(f, &app)).unwrap();
        let buf2 = t2.backend().buffer().clone();

        assert_eq!(buf1, buf2);
    }

    #[test]
    fn test_empty_snapshots() {
        let mut app = make_log_app();
        app.snapshots.clear();
        app.log_scroll.set_total(0);
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn test_small_terminal() {
        let app = make_log_app();
        let backend = ratatui::backend::TestBackend::new(40, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn test_many_snapshots_scroll() {
        let mut app = make_log_app();
        let mut snaps = Vec::new();
        for i in 0..50 {
            snaps.push(make_snap(
                &format!("noa_s{}", i),
                "default",
                &format!("commit {}", i),
                (1000000000 + i as u64) * 1_000_000,
            ));
        }
        app.snapshots = snaps;
        app.log_scroll.set_total(50);
        app.log_scroll.scroll_down(10);
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn test_log_mode_and_branches_mode_produce_different_output() {
        let log_app = make_log_app();
        let b1 = ratatui::backend::TestBackend::new(80, 24);
        let mut t1 = ratatui::Terminal::new(b1).unwrap();
        t1.draw(|f| render(f, &log_app)).unwrap();
        let buf1 = t1.backend().buffer().clone();

        let branches_app = make_branches_app();
        let b2 = ratatui::backend::TestBackend::new(80, 24);
        let mut t2 = ratatui::Terminal::new(b2).unwrap();
        t2.draw(|f| render(f, &branches_app)).unwrap();
        let buf2 = t2.backend().buffer().clone();

        assert_ne!(buf1, buf2);
    }
}
