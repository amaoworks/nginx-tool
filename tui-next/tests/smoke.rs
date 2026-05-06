//! P1 冒烟测试：CJK 双宽字符的列宽计算与基础布局边界。
//! 详见 architecture.md §16.4 CJK 冒烟测试。

use unicode_width::UnicodeWidthStr;

#[test]
fn cjk_label_double_width() {
    // 一级菜单 label 含中文，列宽计算必须使用 unicode_width。
    // "📊 仪表盘" 中的中文每字 2 列，emoji 至少 1 列。
    let s = "📊 仪表盘";
    let w = s.width();
    // 列宽必须严格大于纯字节长度的 1/3（说明使用了真实宽度而非字节）
    assert!(w >= 8, "expected width >= 8, got {}", w);
    // 一定不会和 String::len 字节数混淆
    assert!(s.len() > w);
}

#[test]
fn ascii_and_cjk_table_columns_align() {
    // 列对齐：必须用 UnicodeWidthStr 而非 chars().count() 或 len()
    // 验证：纯中文字符串显示宽度 = 2 × 字符数
    let cn = "应用代理"; // 4 个中文
    assert_eq!(cn.chars().count(), 4);
    assert_eq!(cn.width(), 8);

    let mixed = "app-应用"; // 4 ASCII + 2 中文
    assert_eq!(mixed.width(), 4 + 4);

    // 长域名截断场景：超出列宽时按宽度截断
    let domain = "blog.例子.com";
    assert_eq!(domain.width(), 9 + 4);
}

#[test]
fn skeleton_compiles() {
    assert_eq!(2 + 2, 4);
}
