use unicode_width::UnicodeWidthChar;

/// 将字符列索引转换为终端显示列偏移，处理 CJK/宽字符。
pub fn display_col(text: &str, char_col: usize) -> u16 {
    text.chars()
        .take(char_col)
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0) as u16)
        .sum()
}
