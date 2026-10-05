/// ソース上の位置。行・列とも 1 始まり
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}
