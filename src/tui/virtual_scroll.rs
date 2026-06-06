#[derive(Debug, Clone)]
pub struct VirtualScroll {
    total_items: usize,
    scroll_offset: usize,
    visible_height: usize,
}

impl VirtualScroll {
    pub fn new(total_items: usize) -> Self {
        Self {
            total_items,
            scroll_offset: 0,
            visible_height: 0,
        }
    }

    pub fn set_total(&mut self, total: usize) {
        self.total_items = total;
        self.clamp();
    }

    pub fn set_visible_height(&mut self, height: usize) {
        self.visible_height = height;
        self.clamp();
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        let max = self.max_offset();
        self.scroll_offset = (self.scroll_offset + amount).min(max);
    }

    pub fn scroll_to(&mut self, index: usize) {
        self.scroll_offset = index.min(self.max_offset());
    }

    pub fn selected_index(&self) -> Option<usize> {
        if self.total_items == 0 {
            None
        } else {
            Some(self.scroll_offset.min(self.total_items - 1))
        }
    }

    pub fn visible_range(&self) -> std::ops::Range<usize> {
        let start = self.scroll_offset;
        let end = (start + self.visible_height).min(self.total_items);
        start..end
    }

    pub fn is_at_top(&self) -> bool {
        self.scroll_offset == 0
    }

    pub fn is_at_bottom(&self) -> bool {
        self.scroll_offset >= self.max_offset()
    }

    fn max_offset(&self) -> usize {
        self.total_items.saturating_sub(self.visible_height)
    }

    fn clamp(&mut self) {
        self.scroll_offset = self.scroll_offset.min(self.max_offset());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scroll_down_clamps() {
        let mut s = VirtualScroll::new(10);
        s.set_visible_height(5);
        s.scroll_down(100);
        assert_eq!(s.scroll_offset, 5);
    }

    #[test]
    fn test_scroll_up_clamps() {
        let mut s = VirtualScroll::new(10);
        s.scroll_up(100);
        assert_eq!(s.scroll_offset, 0);
    }

    #[test]
    fn test_visible_range() {
        let mut s = VirtualScroll::new(10);
        s.set_visible_height(5);
        s.scroll_down(3);
        assert_eq!(s.visible_range(), 3..8);
    }

    #[test]
    fn test_visible_range_at_end() {
        let mut s = VirtualScroll::new(10);
        s.set_visible_height(8);
        s.scroll_down(100);
        assert_eq!(s.visible_range(), 2..10);
    }

    #[test]
    fn test_empty() {
        let mut s = VirtualScroll::new(0);
        s.set_visible_height(10);
        assert_eq!(s.visible_range(), 0..0);
        assert!(s.is_at_top());
        assert!(s.is_at_bottom());
        assert_eq!(s.selected_index(), None);
    }

    #[test]
    fn test_set_total_clamps_offset() {
        let mut s = VirtualScroll::new(20);
        s.set_visible_height(5);
        s.scroll_down(15);
        assert_eq!(s.scroll_offset, 15);
        s.set_total(3);
        assert_eq!(s.scroll_offset, 0);
    }

    #[test]
    fn test_selected_index() {
        let mut s = VirtualScroll::new(10);
        s.set_visible_height(5);
        s.scroll_down(3);
        assert_eq!(s.selected_index(), Some(3));
    }
}
