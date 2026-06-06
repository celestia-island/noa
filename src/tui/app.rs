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
