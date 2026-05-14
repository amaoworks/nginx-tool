use unicode_width::UnicodeWidthChar;

fn display_col(text: &str, char_col: usize) -> u16 {
    text.chars()
        .take(char_col)
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0) as u16)
        .sum()
}

#[test]
fn ascii_cursor_offset_matches_char_count() {
    assert_eq!(display_col("server_name", 6), 6);
}

#[test]
fn cjk_cursor_offset_uses_display_width() {
    assert_eq!(display_col("注释", 0), 0);
    assert_eq!(display_col("注释", 1), 2);
    assert_eq!(display_col("注释", 2), 4);
}

#[test]
fn mixed_cursor_offset_handles_ascii_and_cjk() {
    assert_eq!(display_col("a注b释", 0), 0);
    assert_eq!(display_col("a注b释", 1), 1);
    assert_eq!(display_col("a注b释", 2), 3);
    assert_eq!(display_col("a注b释", 3), 4);
    assert_eq!(display_col("a注b释", 4), 6);
}
