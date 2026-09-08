//! 单行输入编辑器 + 提交历史（终端绘制由主循环负责，这里只管缓冲与历史状态，便于单测）。

#[derive(Default)]
pub struct LineEditor {
    buf: String,
    history: Vec<String>,
    /// 浏览历史时的位置：None=在编辑新行；Some(i)=正在看 history[i]。
    hist_pos: Option<usize>,
    /// 开始浏览历史前暂存的新行内容（浏览到底再还原）。
    stash: String,
}

impl LineEditor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn buffer(&self) -> &str {
        &self.buf
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 插入一个字符（并退出历史浏览态）。
    pub fn insert(&mut self, c: char) {
        self.buf.push(c);
        self.hist_pos = None;
    }

    /// 退格。
    pub fn backspace(&mut self) {
        self.buf.pop();
        self.hist_pos = None;
    }

    /// 清空当前行。
    pub fn clear(&mut self) {
        self.buf.clear();
        self.hist_pos = None;
    }

    /// 取走当前行内容并清空；非空则计入历史。
    pub fn take(&mut self) -> String {
        let s = std::mem::take(&mut self.buf);
        self.hist_pos = None;
        if !s.trim().is_empty() && self.history.last() != Some(&s) {
            self.history.push(s.clone());
        }
        s
    }

    /// 上一条历史（更早）。
    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.hist_pos {
            None => {
                self.stash = self.buf.clone(); // 暂存正在编辑的新行
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.hist_pos = Some(next);
        self.buf = self.history[next].clone();
    }

    /// 下一条历史（更近）；到底则还原暂存的新行。
    pub fn history_next(&mut self) {
        match self.hist_pos {
            None => {}
            Some(i) if i + 1 < self.history.len() => {
                self.hist_pos = Some(i + 1);
                self.buf = self.history[i + 1].clone();
            }
            Some(_) => {
                self.hist_pos = None;
                self.buf = std::mem::take(&mut self.stash);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_backspace() {
        let mut e = LineEditor::new();
        for c in "abc".chars() {
            e.insert(c);
        }
        assert_eq!(e.buffer(), "abc");
        e.backspace();
        assert_eq!(e.buffer(), "ab");
    }

    #[test]
    fn take_returns_and_clears_and_records_history() {
        let mut e = LineEditor::new();
        for c in "hello".chars() {
            e.insert(c);
        }
        assert_eq!(e.take(), "hello");
        assert!(e.is_empty());
        // 历史里有了，可用 prev 取回。
        e.history_prev();
        assert_eq!(e.buffer(), "hello");
    }

    #[test]
    fn blank_lines_not_recorded() {
        let mut e = LineEditor::new();
        e.insert(' ');
        assert_eq!(e.take(), " ");
        e.history_prev();
        assert_eq!(e.buffer(), ""); // 空白行没进历史
    }

    #[test]
    fn history_prev_next_navigates_and_restores_draft() {
        let mut e = LineEditor::new();
        for cmd in ["one", "two"] {
            for c in cmd.chars() {
                e.insert(c);
            }
            e.take();
        }
        // 正在打一半的新行
        for c in "dra".chars() {
            e.insert(c);
        }
        e.history_prev(); // → two
        assert_eq!(e.buffer(), "two");
        e.history_prev(); // → one
        assert_eq!(e.buffer(), "one");
        e.history_prev(); // 到顶，停在 one
        assert_eq!(e.buffer(), "one");
        e.history_next(); // → two
        assert_eq!(e.buffer(), "two");
        e.history_next(); // 到底，还原草稿
        assert_eq!(e.buffer(), "dra");
    }

    #[test]
    fn dedup_consecutive_identical_history() {
        let mut e = LineEditor::new();
        for _ in 0..2 {
            for c in "same".chars() {
                e.insert(c);
            }
            e.take();
        }
        e.history_prev();
        assert_eq!(e.buffer(), "same");
        e.history_prev(); // 只有一条（去重），停在 same
        assert_eq!(e.buffer(), "same");
    }
}
