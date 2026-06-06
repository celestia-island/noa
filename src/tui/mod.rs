mod app;
mod render;
mod terminal;
mod virtual_scroll;

use anyhow::Result;
use std::time::Duration;

pub use app::{App, AppMode, Focus};
use crossterm::event::{self, Event};
pub use terminal::{cleanup_terminal, setup_terminal};
pub use virtual_scroll::VirtualScroll;

pub fn run_interactive(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    mut app: App,
) -> Result<()> {
    loop {
        terminal.draw(|f| render::render(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if app.handle_key(key) {
                    break;
                }
            }
        }
    }
    Ok(())
}
