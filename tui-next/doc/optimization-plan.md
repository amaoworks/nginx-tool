# TUI 优化计划

## 概述

基于当前 TUI 实现的评估（综合评分 8.5/10），本文档列出三个优先级的优化方向：
- **P1 交互增强**：表格排序、搜索高亮、配置 diff 预览
- **P2 视觉优化**：彩色站点类型、自适应表格列宽、进度指示
- **P3 性能优化**：仪表盘并行采集、文件监听

预计工作量：2-3 周（单人），可并行开发。

---

## P1：交互增强

### 1.1 站点列表表格排序

**现状问题：**
- 站点列表固定按文件名顺序显示
- 大量站点时无法快速定位（如"找所有启用的"、"找所有 SSL 异常的"）

**目标：**
- 支持按 状态/名称/类型/SSL 排序
- 按键：`s` 切换排序字段，`o` 切换升序/降序
- 底部状态栏显示当前排序规则（如 `[排序: 状态↓]`）

**实现要点：**
```rust
// src/app/state.rs
pub struct SitesState {
    // 新增字段
    pub sort_by: SortField,
    pub sort_order: SortOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Status,   // 启用/停用
    Name,     // 站点名称
    Type,     // Proxy/Emby/Static
    Ssl,      // SSL 状态
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SitesState {
    pub fn apply_sort(&mut self) {
        self.list.sort_by(|a, b| {
            let cmp = match self.sort_by {
                SortField::Status => a.enabled.cmp(&b.enabled),
                SortField::Name => a.name.cmp(&b.name),
                SortField::Type => a.site_type.cmp(&b.site_type),
                SortField::Ssl => a.ssl.status.cmp(&b.ssl.status),
            };
            match self.sort_order {
                SortOrder::Asc => cmp,
                SortOrder::Desc => cmp.reverse(),
            }
        });
    }
}
```

**UI 变更：**
- `src/ui/views/sites.rs`：表头显示排序指示器（`↑/↓`）
- `src/ui/mod.rs`：底部状态栏增加排序提示 `[s] 切换排序 [o] 升序/降序`

**测试点：**
- 排序后光标保持在同一站点（而非同一索引）
- 刷新列表后排序规则保持

---

### 1.2 日志搜索高亮与匹配计数

**现状问题：**
- 搜索关键字后，匹配行不高亮，视觉上难以定位
- `n/N` 跳转上/下一个匹配时，不显示当前位置（如 `3/12`）

**目标：**
- 匹配关键字用黄色背景高亮
- 顶部显示匹配计数 `[搜索: "error" - 12 处匹配，当前 3/12]`
- `Esc` 快速清空搜索

**实现要点：**
```rust
// src/app/state.rs
pub struct LogsState {
    // 新增字段
    pub search_matches: Vec<usize>,  // 匹配行的索引列表
    pub current_match_idx: usize,    // 当前聚焦的匹配索引
}

impl LogsState {
    pub fn perform_search(&mut self) {
        self.search_matches.clear();
        self.current_match_idx = 0;
        
        if self.search_query.is_empty() {
            return;
        }
        
        for (i, line) in self.lines.iter().enumerate() {
            if line.contains(&self.search_query) {
                self.search_matches.push(i);
            }
        }
    }
    
    pub fn jump_next_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.current_match_idx = (self.current_match_idx + 1) % self.search_matches.len();
        let target = self.search_matches[self.current_match_idx];
        self.scroll_to_line(target);
    }
}
```

**UI 变更：**
- `src/ui/views/logs.rs`：
  - 渲染匹配行时用 `Style::bg(Color::Yellow)` 高亮关键字
  - 顶部显示匹配统计：`format!("[搜索: \"{}\" - {} 处匹配，当前 {}/{}]", ...)`
- `src/app/state.rs`：`Esc` 清空 `search_query` + `search_matches`

**测试点：**
- 搜索 CJK 字符能正确高亮
- 匹配计数准确（包括跨行、重复关键字）
- 滚动到屏幕外的匹配时自动翻页

---

### 1.3 配置编辑 Diff 预览

**现状问题：**
- 站点编辑页（托管/原始模式）保存前看不到改了什么
- `F4` 重置时用户不知道会丢失哪些修改

**目标：**
- `F2` 保存前弹出确认弹窗，显示 unified diff
- 格式：`- 删除行（红色）` / `+ 新增行（绿色）`
- 支持 `y` 确认、`n` 取消、滚动查看

**实现要点：**
```rust
// src/domain/diff.rs（新增模块）
pub fn compute_diff(old: &str, new: &str) -> Vec<DiffLine> {
    // 使用 similar crate
    use similar::{ChangeTag, TextDiff};
    
    let diff = TextDiff::from_lines(old, new);
    diff.iter_all_changes()
        .map(|change| match change.tag() {
            ChangeTag::Delete => DiffLine::Remove(change.value().to_string()),
            ChangeTag::Insert => DiffLine::Add(change.value().to_string()),
            ChangeTag::Equal => DiffLine::Context(change.value().to_string()),
        })
        .collect()
}

pub enum DiffLine {
    Remove(String),   // 红色前缀 -
    Add(String),      // 绿色前缀 +
    Context(String),  // 灰色无前缀
}
```

**依赖新增：**
```toml
# tui-next/Cargo.toml
[dependencies]
similar = "2.6"  # diff 算法库
```

**UI 变更：**
- `src/ui/modal.rs`：新增 `ModalAction::ConfirmSaveWithDiff { site_name, diff }`
- `src/app/state.rs`：
  - `F2` 时先 `compute_diff(original, edited)` 生成 diff
  - 如果有变更，弹出确认弹窗；无变更则直接保存
- 弹窗内容：
  ```
  ┌─ 保存确认 ─────────────────┐
  │ 站点：example.com          │
  │ 变更摘要：2 处修改         │
  │                            │
  │ - proxy_pass http://old:80 │ (红色)
  │ + proxy_pass http://new:80 │ (绿色)
  │   location / {             │ (灰色)
  │                            │
  │ [y] 确认  [n] 取消         │
  └────────────────────────────┘
  ```

**测试点：**
- diff 计算正确（增删改混合场景）
- 大文件 diff 能正常滚动
- 取消后编辑器状态不变

---

## P2：视觉优化

### 2.1 彩色站点类型标识

**现状问题：**
- 站点类型列只有文字（"代理"/"Emby"/"静态"），视觉识别度低

**目标：**
- 不同类型用不同颜色 + emoji：
  - Proxy: `🔄 代理` (Cyan)
  - Emby: `🎬 Emby` (Magenta)
  - Static: `📄 静态` (Blue)
  - Unknown: `❓ 未知` (DarkGray)

**实现要点：**
```rust
// src/ui/views/sites.rs
fn render_site_type_cell(site_type: &SiteType) -> Cell {
    let (emoji, label, color) = match site_type {
        SiteType::Proxy => ("🔄", "代理", Color::Cyan),
        SiteType::Emby => ("🎬", "Emby", Color::Magenta),
        SiteType::Static => ("📄", "静态", Color::Blue),
        SiteType::Unknown => ("❓", "未知", Color::DarkGray),
    };
    Cell::from(Span::styled(
        format!("{} {}", emoji, label),
        Style::default().fg(color),
    ))
}
```

**配色新增：**
```rust
// src/ui/theme.rs
pub const FG_TYPE_PROXY: Color = Color::Cyan;
pub const FG_TYPE_EMBY: Color = Color::Magenta;
pub const FG_TYPE_STATIC: Color = Color::Blue;
```

**测试点：**
- 颜色在深色/浅色终端都清晰可辨
- emoji 宽度计算正确（不破坏列对齐）

---

### 2.2 自适应表格列宽

**现状问题：**
- `sites.rs:144` 硬编码 `area.width.saturating_sub(54)`
- 窗口缩放时列宽不合理（域名列可能过窄或过宽）

**目标：**
- 用 `Constraint` 定义列宽比例
- 固定列（状态/类型/SSL）用 `Length`，域名列用 `Percentage` + `Min`

**实现要点：**
```rust
// src/ui/views/sites.rs
let widths = [
    Constraint::Length(8),   // 状态
    Constraint::Length(16),  // 名称
    Constraint::Min(40),     // 域名→目标（最少40，自动扩展）
    Constraint::Length(10),  // 类型
    Constraint::Length(14),  // SSL
];

let table = Table::new(rows, widths)
    .header(header)
    .highlight_style(focus::focused_button_style());
```

**测试点：**
- 终端从 80 列缩放到 200 列，表格布局正常
- 域名列不会被截断（除非窗口小于 MIN_COLS）

---

### 2.3 操作进度指示

**现状问题：**
- 证书申请、备份创建等耗时操作只显示 "操作中…"
- 用户不知道卡住还是在执行

**目标：**
- 短操作（<2s）：spinner 动画 `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`
- 长操作（≥2s）：进度条 + 状态文字

**实现要点：**
```rust
// src/app/state.rs
pub struct OperationProgress {
    pub message: String,
    pub spinner_frame: usize,  // 0-9 循环
    pub started_at: Instant,
}

impl AppState {
    pub fn on_tick(&mut self) {
        if let Some(progress) = &mut self.operation_progress {
            progress.spinner_frame = (progress.spinner_frame + 1) % 10;
        }
    }
}

const SPINNER_FRAMES: [char; 10] = ['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏'];
```

**UI 变更：**
- `src/ui/mod.rs`：操作中时在右下角显示：
  ```
  ⠹ 正在申请证书 example.com... (3s)
  ```
- 使用 `ratatui::widgets::Gauge` 显示进度（如果操作能报告进度）

**测试点：**
- spinner 动画流畅（250ms 刷新）
- 操作完成后自动清除进度提示

---

## P3：性能优化

### 3.1 仪表盘并行采集

**现状问题：**
- `domain/dashboard.rs` 中各指标串行采集：
  ```rust
  let nginx_version = probe_nginx_version().await;  // 等待
  let certs = probe_certs().await;                  // 等待
  let disk = probe_disk().await;                     // 等待
  ```
- 总耗时 = 所有探测时间之和

**目标：**
- 用 `tokio::join!` 并行采集，总耗时 ≈ 最慢的探测时间
- 预期提速 50-70%

**实现要点：**
```rust
// src/domain/dashboard.rs
pub async fn collect(ctx: Arc<AppContext>) -> DashboardSnapshot {
    let start = Instant::now();
    
    // 并行采集所有指标
    let (
        nginx_active,
        nginx_version,
        enabled_count,
        certs,
        disk,
        memory,
        recent_errors,
    ) = tokio::join!(
        probe_nginx_active(&ctx),
        probe_nginx_version(&ctx),
        probe_enabled_count(&ctx),
        probe_certs(&ctx),
        probe_disk(&ctx),
        probe_memory(),
        probe_recent_errors(&ctx),
    );
    
    DashboardSnapshot {
        nginx_active,
        nginx_version,
        enabled_count,
        certs,
        disk,
        memory,
        recent_errors,
        elapsed_ms: start.elapsed().as_millis() as u64,
    }
}
```

**测试点：**
- 某个探测超时不影响其他探测
- 并行采集后 `elapsed_ms` 显著降低

---

### 3.2 站点列表文件监听

**现状问题：**
- 每次刷新都 `fs::read_dir` + 解析所有配置文件
- 外部修改（如 shell 脚本新建站点）无法自动刷新

**目标：**
- 用 `notify` 监听 `/etc/nginx/sites-available/` 和 `/etc/nginx/sites-enabled/`
- 文件变化时自动刷新列表（debounce 500ms）

**实现要点：**
```rust
// src/infra/fs_watch.rs（新增模块）
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

pub fn spawn_sites_watcher(
    sites_available: PathBuf,
    sites_enabled: PathBuf,
) -> mpsc::UnboundedReceiver<()> {
    let (tx, rx) = mpsc::unbounded_channel();
    
    std::thread::spawn(move || {
        let mut watcher = RecommendedWatcher::new(
            move |_| {
                let _ = tx.send(());
            },
            Config::default(),
        ).unwrap();
        
        watcher.watch(&sites_available, RecursiveMode::NonRecursive).ok();
        watcher.watch(&sites_enabled, RecursiveMode::NonRecursive).ok();
        
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    });
    
    rx
}
```

**依赖新增：**
```toml
# tui-next/Cargo.toml
[dependencies]
notify = "6.1"  # 文件系统监听
```

**主循环集成：**
```rust
// src/main.rs
let mut fs_watch_rx = infra::fs_watch::spawn_sites_watcher(
    ctx.paths.sites_available.clone(),
    ctx.paths.sites_enabled.clone(),
);

loop {
    tokio::select! {
        // ... 其他分支
        Some(_) = fs_watch_rx.recv() => {
            state.request_sites_refresh();
        }
    }
}
```

**测试点：**
- 在另一个终端 `touch /etc/nginx/sites-available/test.conf`，TUI 自动刷新
- debounce 生效（短时间多次修改只触发一次刷新）

---

## 实施建议

### 阶段划分

**第一阶段（1 周）：P1 交互增强**
1. Day 1-2：站点列表排序
2. Day 3-4：日志搜索高亮
3. Day 5-7：配置 diff 预览（含 similar crate 集成）

**第二阶段（3-5 天）：P2 视觉优化**
1. Day 1：彩色站点类型
2. Day 2-3：自适应表格列宽（含测试不同终端尺寸）
3. Day 4-5：操作进度指示（spinner + gauge）

**第三阶段（3-5 天）：P3 性能优化**
1. Day 1-2：仪表盘并行采集（含性能基准测试）
2. Day 3-5：文件监听（含 notify crate 集成 + debounce 逻辑）

### 质量保证

**每个功能完成后：**
1. 运行 `cargo test` 确保无回归
2. 手动测试核心流程（新建站点 → 编辑 → 启用 → 删除）
3. 更新 `CHANGELOG.md` 记录变更
4. 更新 `doc/design.md` 补充新增的交互规范

**发布前：**
1. 在 3 种终端测试（gnome-terminal / tmux / kitty）
2. 测试不同窗口尺寸（80x24 / 120x40 / 200x60）
3. 压力测试（100+ 站点、1000+ 行日志）

---

## 依赖变更摘要

```toml
# tui-next/Cargo.toml
[dependencies]
similar = "2.6"   # P1.3 diff 预览
notify = "6.1"    # P3.2 文件监听
```

---

## 文档更新清单

- [ ] `doc/design.md`：补充排序、搜索、diff 交互规范
- [ ] `doc/architecture.md`：补充文件监听架构、并行采集策略
- [ ] `CHANGELOG.md`：记录所有变更（按版本号组织）
- [ ] `README.md`：更新功能列表和截图（如有）

---

## 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| similar crate 增加二进制体积 | 编译产物增大 10-20% | 可接受；diff 功能价值高 |
| notify 跨平台兼容性 | macOS/Windows 行为差异 | CI 增加 macOS 测试；文档说明 Linux 优先 |
| 并行采集导致超时控制复杂 | 某个探测卡住影响整体 | 每个探测独立 timeout，`tokio::join!` 等待所有完成 |
| 文件监听误触发 | 频繁刷新影响性能 | debounce 500ms + 只监听 `*.conf` 文件 |

---

## 后续迭代方向（P4）

完成 P1-P3 后，可考虑：
- 批量操作（多选站点）
- Nginx 配置语法高亮（原始编辑模式）
- 国际化（i18n）
- 插件系统（自定义站点模板）

---

**文档版本**：v1.0  
**创建日期**：2026-06-03  
**适用版本**：tui-next >= v1.2.0
