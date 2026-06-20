/// Utility helpers used by the summarizer.
/// Return the largest index <= `limit` that is a valid char boundary.
pub fn floor_char_boundary(s: &str, limit: usize) -> usize {
    if s.len() <= limit {
        return s.len();
    }
    let mut i = limit;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

