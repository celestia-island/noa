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
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.handle_key(key)));
                match result {
                    Ok(should_quit) => {
                        if should_quit {
                            break;
                        }
                    }
                    Err(_) => {
                        tracing::error!("panic in TUI key handler, exiting gracefully");
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
