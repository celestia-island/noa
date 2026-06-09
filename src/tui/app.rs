#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Log,
    Branches,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Branches,
    Log,
    Detail,
}

impl Focus {
    pub fn cycle(self, mode: AppMode) -> Self {
        match mode {
            AppMode::Branches => match self {
                Focus::Branches => Focus::Log,
                Focus::Log => Focus::Detail,
                Focus::Detail => Focus::Branches,
            },
            AppMode::Log => match self {
                Focus::Log => Focus::Detail,
                Focus::Detail => Focus::Log,
                _ => Focus::Log,
            },
        }
    }

    pub fn cycle_back(self, mode: AppMode) -> Self {
        match mode {
            AppMode::Branches => match self {
                Focus::Branches => Focus::Detail,
                Focus::Log => Focus::Branches,
                Focus::Detail => Focus::Log,
            },
            AppMode::Log => match self {
                Focus::Log => Focus::Detail,
                Focus::Detail => Focus::Log,
                _ => Focus::Detail,
            },
        }
    }
}

pub struct App {
    pub mode: AppMode,
    pub focus: Focus,
    pub branches: Vec<crate::workspace::Workspace>,
    pub snapshots: Vec<crate::snapshot::Snapshot>,
    pub current_branch: String,
    pub branch_scroll: super::VirtualScroll,
    pub log_scroll: super::VirtualScroll,
    pub should_quit: bool,
}

impl App {
    pub fn for_log(snapshots: Vec<crate::snapshot::Snapshot>, current_branch: String) -> Self {
        let count = snapshots.len();
        Self {
            mode: AppMode::Log,
            focus: Focus::Log,
            branches: vec![],
            snapshots,
            current_branch,
            branch_scroll: super::VirtualScroll::new(0),
            log_scroll: super::VirtualScroll::new(count),
            should_quit: false,
        }
    }

    pub fn for_branches(
        branches: Vec<crate::workspace::Workspace>,
        snapshots: Vec<crate::snapshot::Snapshot>,
        current_branch: String,
    ) -> Self {
        let branch_count = branches.len();
        let log_count = snapshots.len();
        Self {
            mode: AppMode::Branches,
            focus: Focus::Branches,
            branches,
            snapshots,
            current_branch,
            branch_scroll: super::VirtualScroll::new(branch_count),
            log_scroll: super::VirtualScroll::new(log_count),
            should_quit: false,
        }
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.modifiers, key.code) {
            (_, KeyCode::Char('q') | KeyCode::Esc) => {
                self.should_quit = true;
                return true;
            }
            (_, KeyCode::Tab) => {
                self.focus = self.focus.cycle(self.mode);
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.focus = self.focus.cycle_back(self.mode);
            }
            (_, KeyCode::Up | KeyCode::Char('k')) => self.scroll_up(),
            (_, KeyCode::Down | KeyCode::Char('j')) => self.scroll_down(),
            (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                if self.mode == AppMode::Log {
                    self.mode = AppMode::Branches;
                    self.focus = Focus::Branches;
                } else {
                    self.mode = AppMode::Log;
                    self.focus = Focus::Log;
                }
            }
            (_, KeyCode::Enter) => {
                if self.mode == AppMode::Log && self.focus == Focus::Log {
                    self.focus = Focus::Detail;
                }
            }
            _ => {}
        }
        false
    }

    fn scroll_up(&mut self) {
        match self.focus {
            Focus::Branches => self.branch_scroll.scroll_up(1),
            Focus::Log => self.log_scroll.scroll_up(1),
            Focus::Detail => {}
        }
    }

    fn scroll_down(&mut self) {
        match self.focus {
            Focus::Branches => self.branch_scroll.scroll_down(1),
            Focus::Log => self.log_scroll.scroll_down(1),
            Focus::Detail => {}
        }
    }

    pub fn selected_snapshot(&self) -> Option<&crate::snapshot::Snapshot> {
        let idx = self.log_scroll.selected_index()?;
        self.snapshots.get(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn make_test_app() -> App {
        App {
            mode: AppMode::Log,
            focus: Focus::Log,
            branches: vec![],
            snapshots: vec![],
            current_branch: "default".to_string(),
            branch_scroll: crate::tui::VirtualScroll::new(0),
            log_scroll: crate::tui::VirtualScroll::new(0),
            should_quit: false,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_quit_on_q() {
        let mut app = make_test_app();
        let result = app.handle_key(key(KeyCode::Char('q')));
        assert!(result);
        assert!(app.should_quit);
    }

    #[test]
    fn test_quit_on_esc() {
        let mut app = make_test_app();
        let result = app.handle_key(key(KeyCode::Esc));
        assert!(result);
        assert!(app.should_quit);
    }

    #[test]
    fn test_tab_cycles_focus_in_log_mode() {
        let mut app = make_test_app();
        assert_eq!(app.focus, Focus::Log);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Detail);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Log);
    }

    #[test]
    fn test_enter_focuses_detail_in_log_mode() {
        let mut app = make_test_app();
        app.mode = AppMode::Log;
        app.focus = Focus::Log;
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.focus, Focus::Detail);
    }

    #[test]
    fn test_enter_does_nothing_in_branches_mode() {
        let mut app = make_test_app();
        app.mode = AppMode::Branches;
        app.focus = Focus::Log;
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.focus, Focus::Log);
    }

    #[test]
    fn test_ctrl_b_toggles_mode() {
        let mut app = make_test_app();
        assert_eq!(app.mode, AppMode::Log);
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert_eq!(app.mode, AppMode::Branches);
        assert_eq!(app.focus, Focus::Branches);
        app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert_eq!(app.mode, AppMode::Log);
        assert_eq!(app.focus, Focus::Log);
    }

    #[test]
    fn test_unknown_key_does_not_quit() {
        let mut app = make_test_app();
        let result = app.handle_key(key(KeyCode::Char('x')));
        assert!(!result);
        assert!(!app.should_quit);
    }
}
