# Nginx-Tools TUI 架构设计文档

## 一、文档定位

本文档用于指导 `tui-next` 的 Rust TUI 实现，承接已确认的 `design.md` 交互设计，不修改既有设计文档。

本文档回答以下问题：

1. 使用什么 Rust 技术栈构建 TUI。
2. 如何把 `design.md` 中的页面、覆盖模式、弹窗、快捷键和数据流落到代码结构。
3. 如何组织 Nginx、Certbot、Systemd、文件系统、日志流和模板渲染等能力。
4. 如何控制权限、错误、长任务、回滚和配置持久化。

## 二、已确认技术决策

| 项目 | 决策 |
|------|------|
| 界面语言 | 只做中文界面 |
| 主语言 | Rust |
| TUI 框架 | `ratatui` + `crossterm` |
| 异步运行 | `tokio` |
| 无 root 权限 | 允许只读模式运行 |
| 日志查看 | 实时跟踪，使用 `tail -F` 等价能力 |
| 外部编辑器 | 保留 `$EDITOR` 外部编辑入口 |
| 配置与备份根目录 | `~/.local/ngtool` |
| Nginx 目录结构 | 首版固定 Debian/Ubuntu 风格：`/etc/nginx/sites-available` + `/etc/nginx/sites-enabled` |
| 快速提交 | 保留快捷键，但必须执行字段校验，不允许跳过校验 |
| 证书申请 | 必须绑定已有站点，从站点配置解析域名 |
| Init 系统 | 首版只支持 systemd |
| 开发路线 | 先 MVP，再补齐完整替代 Bash 脚本 |

## 三、架构目标

### 3.1 功能目标

TUI 首版最终应覆盖现有 `nginx-site.sh` 与 `nginx-monitor.sh` 的核心能力：

- 仪表盘：Nginx 状态、站点数量、证书、系统资源、最近错误。
- 站点管理：列表、新建、编辑、启用、停用、删除、单站点证书申请、跳转日志。
- 证书管理：证书列表、站点关联、续签、自动续签状态检查。
- 日志查看：全局日志和站点日志实时跟踪、暂停、搜索、清屏。
- 服务控制：测试配置、重载、重启、查看状态。
- 备份还原：创建备份、还原、删除备份。

### 3.2 工程目标

- 单二进制发布，尽量减少运行时依赖。
- UI 层不直接执行系统命令。
- 系统命令统一通过执行器封装，输出、错误、超时、取消都可观测。
- Nginx 配置写入必须有明确回滚策略。
- 长任务不能阻塞 TUI 渲染和输入响应。
- 模板渲染不能破坏用户自定义注入槽。
- 无 root 权限时可浏览数据，但写操作禁用并给出中文提示。

## 四、技术栈

### 4.1 核心依赖建议

| 领域 | crate | 用途 |
|------|-------|------|
| TUI 渲染 | `ratatui` | 布局、组件绘制、样式 |
| 终端事件 | `crossterm` | 键盘事件、raw mode、alternate screen |
| 异步运行 | `tokio` | 后台任务、命令执行、事件通道 |
| CLI 参数 | `clap` | 启动参数、只读模式、配置路径 |
| 序列化 | `serde` | 配置、缓存、结构化数据 |
| 配置格式 | `toml` | `~/.local/ngtool/config.toml` |
| 错误处理 | `anyhow` | 应用入口和跨层错误 |
| 领域错误 | `thiserror` | 可分类、可展示的业务错误 |
| 日志 | `tracing` | 应用日志与调试 |
| 日志落盘 | `tracing-appender` | 写入 `~/.local/ngtool/logs/` |
| 时间处理 | `chrono` 或 `time` | 证书过期时间、备份时间戳 |
| 目录定位 | `directories` | 用户目录与本地数据目录 |
| 模板渲染 | `minijinja` | Nginx 配置模板渲染 |
| 文本编辑 | `tui-textarea` | 原始配置编辑器和注入槽 |
| 文件遍历 | `walkdir` | 站点、备份、日志扫描 |
| 正则解析 | `regex` | Nginx 配置轻量解析、certbot 输出兜底解析 |
| 压缩归档 | `tar` + `flate2` | 备份与还原 |

### 4.2 不建议首版引入

| 依赖/方案 | 原因 |
|-----------|------|
| 数据库 | 当前数据来自文件系统和命令输出，数据库会增加状态同步复杂度 |
| Web 服务 | 目标是本地 TUI，首版无需后台 daemon |
| 完整 Nginx AST 解析器 | 首版只需要稳定提取常见字段和注入槽，完整语法解析成本高 |
| 多语言 i18n 框架 | 已确认只做中文，首版直接中文文案即可 |

## 五、项目目录结构

建议初始结构如下：

```text
tui-next/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── app/
│   │   ├── mod.rs
│   │   ├── state.rs
│   │   ├── event.rs
│   │   ├── route.rs
│   │   └── task.rs
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── layout.rs
│   │   ├── theme.rs
│   │   ├── widgets.rs
│   │   ├── focus.rs
│   │   ├── modal.rs
│   │   └── views/
│   │       ├── dashboard.rs
│   │       ├── sites.rs
│   │       ├── site_form.rs
│   │       ├── site_editor.rs
│   │       ├── certs.rs
│   │       ├── logs.rs
│   │       ├── service.rs
│   │       └── backup.rs
│   ├── domain/
│   │   ├── mod.rs
│   │   ├── site.rs
│   │   ├── cert.rs
│   │   ├── log.rs
│   │   ├── service.rs
│   │   ├── backup.rs
│   │   └── command.rs
│   ├── infra/
│   │   ├── mod.rs
│   │   ├── executor.rs
│   │   ├── filesystem.rs
│   │   ├── nginx.rs
│   │   ├── systemd.rs
│   │   ├── certbot.rs
│   │   ├── log_tail.rs
│   │   └── archive.rs
│   ├── template/
│   │   ├── mod.rs
│   │   ├── renderer.rs
│   │   ├── parser.rs
│   │   └── snippets.rs
│   ├── config/
│   │   ├── mod.rs
│   │   └── settings.rs
│   └── error.rs
├── templates/
│   ├── proxy.conf.j2
│   ├── emby.conf.j2
│   └── static.conf.j2
└── tests/
    ├── fixtures/
    └── integration/
```

## 六、分层架构

```text
┌───────────────────────────────────────────┐
│ UI 层                                      │
│ ratatui 绘制、焦点、快捷键、弹窗、页面状态 │
└─────────────────────┬─────────────────────┘
                      │ AppEvent / CommandIntent
┌─────────────────────▼─────────────────────┐
│ App 层                                     │
│ 路由、全局状态、任务调度、权限裁剪、提示消息 │
└─────────────────────┬─────────────────────┘
                      │ Use Case
┌─────────────────────▼─────────────────────┐
│ Domain 层                                  │
│ Site、Cert、Backup、Log、Service 等模型     │
└─────────────────────┬─────────────────────┘
                      │ Port / Repository
┌─────────────────────▼─────────────────────┐
│ Infra 层                                   │
│ 文件系统、systemctl、nginx、certbot、tail   │
└───────────────────────────────────────────┘
```

### 6.1 UI 层职责

UI 层只处理：

- 绘制顶部标题栏、左侧菜单、右侧主视图、底部状态栏。
- 实现 `design.md` 中的焦点状态、覆盖模式、弹窗规范。
- 将键盘输入转换为 `AppEvent`。
- 根据 `AppState` 显示数据、错误、进度和任务输出。

UI 层禁止：

- 直接读取 `/etc/nginx`。
- 直接执行 `nginx`、`systemctl`、`certbot`。
- 直接决定命令执行顺序。
- 直接修改配置文件。

### 6.2 App 层职责

App 层是 TUI 的中枢：

- 管理当前一级菜单和覆盖模式。
- 管理全局权限状态：读写模式、只读模式、root 检测。
- 管理后台任务：刷新仪表盘、日志跟踪、命令执行、备份还原。
- 将 UI 意图转换为领域用例。
- 接收任务结果并更新状态。
- 控制操作提示的生命周期：成功 2 秒，失败 3 秒。

### 6.3 Domain 层职责

Domain 层定义业务对象和规则：

- `Site`：站点名、域名、类型、启用状态、目标、SSL 状态。
- `SiteConfig`：表单字段、注入槽、模板类型。
- `Certificate`：证书名、域名列表、过期时间、剩余天数、关联站点。
- `Backup`：文件名、大小、创建时间、路径。
- `ServiceStatus`：运行状态、版本、systemd 状态摘要。
- `LogSource`：全局访问日志、全局错误日志、站点访问日志、站点错误日志。

Domain 层负责校验：

- 站点名格式和重复检查。
- 域名格式。
- 代理目标解析。
- 静态站点目录推导。
- 证书申请域名必须来自已有站点。
- 只读模式下禁止写操作。

### 6.4 Infra 层职责

Infra 层是对系统环境的适配：

- 扫描 `/etc/nginx/sites-available/*.conf`。
- 检查 `/etc/nginx/sites-enabled/*.conf` 符号链接。
- 执行 `nginx -t`、`nginx -v`。
- 执行 `systemctl is-active/status/reload/restart nginx`。
- 执行 `certbot certificates`、`certbot renew`、`certbot --nginx`。
- 实现日志实时跟踪。
- 实现备份压缩和还原。
- 实现原子写入和回滚。

Infra 层必须通过统一 `CommandExecutor` 执行外部命令，不允许散落 `std::process::Command`。

## 七、核心状态模型

### 7.1 AppState

```rust
pub struct AppState {
    pub mode: RunMode,
    pub route: Route,
    pub sidebar: SidebarState,
    pub dashboard: DashboardState,
    pub sites: SitesState,
    pub certs: CertsState,
    pub logs: LogsState,
    pub service: ServiceState,
    pub backup: BackupState,
    pub modal: Option<ModalState>,
    pub notification: Option<Notification>,
    pub tasks: TaskRegistry,
    pub settings: Settings,
}
```

### 7.2 RunMode

```rust
pub enum RunMode {
    ReadWrite,
    ReadOnly { reason: String },
}
```

进入只读模式的常见原因：

- 当前用户不是 root。
- 显式启动参数 `--readonly`。
- `/etc/nginx` 不可写。
- `systemctl` 或 `nginx` 缺失时降级。

只读模式允许：

- 查看仪表盘。
- 查看站点列表。
- 查看证书列表。
- 查看日志。
- 查看服务状态。
- 查看备份列表。
- 打开配置只读预览。

只读模式禁止：

- 新建、编辑、删除站点。
- 启用、停用站点。
- 申请或续签证书。
- 测试后重载、重启服务。
- 创建、删除、还原备份。
- 外部编辑器写入。

### 7.3 Route

```rust
pub enum Route {
    Dashboard,
    Sites(SitesRoute),
    Certs(CertsRoute),
    Logs(LogsRoute),
    Service,
    Backup(BackupRoute),
}

pub enum SitesRoute {
    List,
    New,
    EditForm { site_name: String },
    EditRaw { site_name: String },
}
```

Route 必须贴合 `design.md` 的主视图覆盖原则：

- 一级菜单保持不变。
- 右侧主视图被完整替换。
- 顶部标题栏和底部状态栏保持存在。
- `Esc` 返回上一级路由。

## 八、事件与任务模型

### 8.1 事件来源

```text
键盘输入
  └── UI Event
        └── AppEvent

后台任务
  └── TaskEvent
        └── AppEvent

定时器
  └── Tick / Refresh
        └── AppEvent
```

### 8.2 AppEvent

```rust
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Refresh,
    Task(TaskEvent),
    NotificationExpired,
    QuitRequested,
}
```

### 8.3 CommandIntent

UI 不直接执行命令，而是产生意图：

```rust
pub enum CommandIntent {
    RefreshDashboard,
    LoadSites,
    CreateSite(CreateSiteInput),
    EnableSite(String),
    DisableSite(String),
    DeleteSite(String),
    SaveSiteForm(SaveSiteInput),
    SaveRawConfig(SaveRawInput),
    RequestCert { site_name: String },
    RenewCerts,
    TailLog(LogSource),
    TestNginxConfig,
    ReloadNginx,
    RestartNginx,
    CreateBackup,
    RestoreBackup(PathBuf),
    DeleteBackup(PathBuf),
}
```

### 8.4 后台任务

长任务必须异步执行：

- `certbot --nginx`
- `certbot renew`
- `systemctl status nginx`
- `tail -F` 日志流
- 备份压缩
- 还原备份
- 外部编辑器

任务状态：

```rust
pub enum TaskStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
```

任务输出：

- 普通任务保留完整 stdout/stderr。
- 长输出任务保留最近 N 行，默认 1000 行。
- 任务失败必须保留 exit code 和 stderr。
- 任务运行中 UI 必须显示进度或“执行中”状态。

## 九、权限模型

### 9.1 启动检测

启动时执行：

1. 检查是否 root。
2. 检查 `nginx` 是否存在。
3. 检查 `systemctl` 是否存在。
4. 检查 `certbot` 是否存在。
5. 检查 `/etc/nginx/sites-available` 是否存在且可读。
6. 检查 `/etc/nginx/sites-enabled` 是否存在且可读。
7. 检查 `~/.local/ngtool` 是否可创建。
8. 尝试在 `~/.local/ngtool/tui.lock` 上获取 `flock`（参见 §15.0 单实例软锁）。

### 9.2 权限策略

| 条件 | 模式 |
|------|------|
| root + 依赖完整 | 读写模式 |
| 非 root | 只读模式 |
| 显式 `--readonly` | 只读模式 |
| Nginx 目录缺失 | 降级只读 + 环境异常态 |
| Nginx 目录存在但不可读（权限改动） | 降级只读 + 环境异常态，并显示具体路径 |
| `certbot` 缺失 | 整体可用，证书操作禁用 |
| `tui.lock` 已被占用 | 降级只读，并提示已有实例运行 |

**环境异常态**：在仪表盘和受影响视图顶部展示红色横幅，说明哪些路径不可访问、对应功能被禁用，但 TUI 仍可启动并展示其它信息（如系统资源、日志查看），不直接退出。

### 9.3 UI 表现

只读模式下：

- 禁用写操作按钮或快捷键。
- 底部状态栏移除不可用快捷键，或显示"只读模式"。
- 用户触发写操作时显示提示：`当前为只读模式，需要 root 权限执行此操作`（或：`当前实例为只读，主实例正在运行`）。
- 原始配置查看允许，但禁用 `Ctrl+S`/`Ctrl+W`，`$EDITOR` 入口禁用。

## 十、配置与数据目录

根目录固定为：

```text
~/.local/ngtool/
```

建议结构：

```text
~/.local/ngtool/
├── config.toml
├── backups/
│   ├── nginx-config-20260429-210000.tar.gz
│   └── nginx-config-current-20260429-220000.tar.gz
├── logs/
│   └── tui.log
├── cache/
│   ├── dashboard.toml
│   └── certs.toml
└── tmp/
```

### 10.1 config.toml

```toml
[ui]
auto_refresh_seconds = 30
theme = "default"
max_log_lines = 1000
min_terminal_cols = 80
min_terminal_rows = 24

[paths]
nginx_root = "/etc/nginx"
sites_available = "/etc/nginx/sites-available"
sites_enabled = "/etc/nginx/sites-enabled"
backup_dir = "~/.local/ngtool/backups"

[behavior]
confirm_restart = true
confirm_delete_site = true
confirm_restore_backup = true
external_editor = true

[command]
default_timeout_secs = 3
dashboard_total_timeout_secs = 5

[backup]
keep_recent = 0          # 0 表示不自动裁剪；> 0 时保留最近 N 份手动创建的备份
keep_pre_restore = 5     # 还原前自动备份保留份数

[audit]
enabled = true
max_size_mb = 5
keep_files = 5
```

首版虽然固定 Nginx 目录，但配置文件预留字段。实现时不得在 UI 暴露自定义目录入口，避免和"暂时固定"的决策冲突。配置文件不存在时使用上述默认值，并在 `~/.local/ngtool/config.toml` 写入一份带注释的样例。

## 十一、页面架构映射

### 11.1 全局布局

`design.md` 要求：

- 顶部标题栏固定 1 行。
- 左侧边栏宽度 25%，不小于 20 列。
- 右侧主视图宽度 75%。
- 底部状态栏固定 1 行。

实现建议：

```text
ui/layout.rs
  ├── root_layout(frame_area) -> RootAreas
  ├── header_area
  ├── body_area
  ├── sidebar_area
  ├── content_area
  └── footer_area
```

所有 View 只接收 `content_area`，不能自行覆盖全屏。

### 11.2 仪表盘

模块：

- `ui/views/dashboard.rs`
- `domain/service.rs`
- `infra/nginx.rs`
- `infra/systemd.rs`
- `infra/certbot.rs`
- `infra/filesystem.rs`

数据采集：

- `systemctl is-active nginx`
- `nginx -v`
- `/etc/nginx/sites-enabled/*.conf`
- `certbot certificates`
- `df -h /`
- `free -h`
- `/var/log/nginx/error.log` 最近 3 条

刷新策略：

- 进入页面立即刷新。
- 按 `r` 手动刷新。
- 默认每 30 秒刷新（间隔可在 `config.toml` 中配置）。
- 刷新失败不清空旧数据，显示错误提示和上次更新时间。

性能与超时预算：

- 上述七项数据采集**全部并发执行**（`tokio::join!` 或 `FuturesUnordered`），不允许串行。
- 每个外部命令单独超时 3 秒，超时返回 `DependencyMissing` 或 `CommandFailed { Cancelled }`，不阻塞整次刷新。
- 整次刷新硬上限 5 秒，超时则保留已返回的部分数据，未完成项显示 `加载中...` 或上次值 + ⚠ 标记。
- `certbot certificates` 在 P3 阶段往往是最慢项；UI 必须容忍其单项失败，仪表盘其它部分照常显示。

### 11.3 站点管理

模块：

- `ui/views/sites.rs`
- `ui/views/site_form.rs`
- `ui/views/site_editor.rs`
- `domain/site.rs`
- `infra/nginx.rs`
- `template/renderer.rs`
- `template/parser.rs`

子路由：

- `SitesRoute::List`
- `SitesRoute::New`
- `SitesRoute::EditForm`
- `SitesRoute::EditRaw`

核心流程：

- 列表扫描 `sites-available`。
- 用 `sites-enabled` 符号链接判断启用状态。
- 解析 `server_name`、`proxy_pass`、`root`。
- 交叉匹配证书状态。
- 新建时用模板渲染。
- 保存时执行校验、写入、`nginx -t`、按需 reload。

快速提交：

- `Ctrl+Enter` 表示“快速提交当前表单”。
- 必须执行完整字段校验。
- 不允许绕过站点名、域名、目标、重复文件等校验。

### 11.4 证书管理

模块：

- `ui/views/certs.rs`
- `domain/cert.rs`
- `infra/certbot.rs`
- `infra/systemd.rs`

证书申请：

- 只能选择已有站点。
- 从站点配置解析 `server_name`。
- 多域名站点默认申请全部解析出的域名。
- 无域名时禁用申请按钮。

证书状态：

- 剩余 > 30 天：正常。
- 7-30 天：即将到期。
- < 7 天：紧急。
- 证书存在但无匹配站点：孤立证书。

### 11.5 日志查看

模块：

- `ui/views/logs.rs`
- `domain/log.rs`
- `infra/log_tail.rs`

实现策略：

- 使用 Rust 实现 `tail -F` 等价能力，或通过统一执行器启动 `tail -F`。
- 首版建议优先封装系统 `tail -F`，降低实现成本。
- 日志任务通过 channel 持续向 App 层发送新行。
- 切换站点或日志类型时取消旧 tail 任务并启动新任务。
- 暂停时继续接收但不滚动，或直接暂停渲染追加，需在实现文档中固定。

日志缓冲：

- 默认最多保留 1000 行。
- 搜索只在当前缓冲内执行。
- 清屏只清 UI 缓冲，不删除日志文件。

### 11.6 服务控制

模块：

- `ui/views/service.rs`
- `domain/service.rs`
- `infra/nginx.rs`
- `infra/systemd.rs`

命令：

- 测试配置：`nginx -t`
- 重载配置：先 `nginx -t`，通过后 `systemctl reload nginx`
- 重启服务：确认弹窗后 `systemctl restart nginx`
- 查看状态：`systemctl status nginx`

注意：

- 不允许用 shell 串联 `nginx -t && systemctl reload nginx`。
- 必须分步执行，分别展示测试结果和重载结果。

### 11.7 备份还原

模块：

- `ui/views/backup.rs`
- `domain/backup.rs`
- `infra/archive.rs`
- `infra/nginx.rs`
- `infra/systemd.rs`

路径：

- 备份目录：`~/.local/ngtool/backups/`

#### 11.7.1 备份范围（重要）

为避免覆盖用户手工维护的 Nginx 资源（参见 `risks.md R6`），**首版备份和还原都不以 `/etc/nginx/` 整目录为单位**。范围严格限定为以下三类工具可管理的对象：

| 类别 | 路径 | 说明 |
|------|------|------|
| 主配置 | `/etc/nginx/nginx.conf` | 顶层入口 |
| 可用站点 | `/etc/nginx/sites-available/*.conf` | 工具创建或可识别的站点定义 |
| 启用状态 | `/etc/nginx/sites-enabled/*.conf` | 实际是符号链接，备份记录链接关系而非内容 |

**显式不在备份/还原范围内的内容：**

- `/etc/nginx/conf.d/`、`/etc/nginx/snippets/`、`/etc/nginx/modules-enabled/`
- `/etc/nginx/mime.types`、`/etc/nginx/fastcgi_params` 等系统提供的辅助文件
- 任何 `nginx.conf` 中通过 `include` 引入的、位于上述目录之外的文件

UI 必须在备份创建前展示范围摘要，让用户对边界一目了然。

#### 11.7.2 备份包结构

```text
nginx-config-YYYYMMDD-HHMMSS.tar.gz
├── manifest.toml           # 备份元数据
├── nginx.conf              # 复制自 /etc/nginx/nginx.conf
├── sites-available/
│   ├── app.conf
│   └── blog.conf
└── sites-enabled.toml      # 记录启用关系，不打包符号链接本身
```

`manifest.toml` 字段：

```toml
schema_version = 1
created_at = "2026-04-29T22:00:00+08:00"
hostname = "orangepi"
nginx_version = "1.24.0"
ngtool_version = "0.1.0"
source = "manual"        # 或 "pre-restore"（还原前自动生成）

[scope]
nginx_conf = true
sites_available = ["app.conf", "blog.conf"]
sites_enabled = ["app.conf"]   # 实际启用的站点名

[checksums]
"nginx.conf" = "sha256:..."
"sites-available/app.conf" = "sha256:..."
```

无 manifest 或 schema 不兼容的备份包视为外部备份，**只允许查看不允许还原**，UI 显示警告并提示用户手动处理。

#### 11.7.3 创建备份

1. 校验 `/etc/nginx/nginx.conf` 可读。
2. 列出 `sites-available/*.conf`、`sites-enabled/` 当前启用集合。
3. 计算每个文件的 sha256。
4. 生成 manifest.toml。
5. 打包 tar.gz 到临时路径 `~/.local/ngtool/tmp/<name>.tar.gz.tmp`。
6. 校验包可解开后原子重命名到 `backups/`。
7. 保留策略首版只展示，不自动删除；后续按 `config.toml` 中 `[backup].keep_recent` 配置裁剪。

#### 11.7.4 还原备份

1. 验证备份包：manifest 存在、schema 兼容、checksum 一致。
2. 弹出确认弹窗，**展示影响摘要**：
   - 将覆盖：`nginx.conf`、`sites-available/{a,b,c}.conf`
   - 将启用：`{x,y}.conf`；将停用：`{z}.conf`
   - 不在范围内的文件**不会**被修改
3. 用户确认后，自动创建一次 `source = "pre-restore"` 备份。
4. 解压目标备份到 `~/.local/ngtool/tmp/restore-<timestamp>/`。
5. 按文件粒度执行写入：
   1. 对范围内的每个目标文件，先写入临时文件再原子替换。
   2. 对 `sites-enabled/`：根据 manifest 中的启用集合，删除当前不在集合中的链接、补全缺失链接，**不动其他符号链接**。
6. 执行 `nginx -t`。
7. 通过则 `systemctl reload nginx`。
8. 失败则提示"是否回滚到还原前备份"，用户确认后用 pre-restore 备份重复 4-7 步。
9. 二次失败时保留临时目录并展示完整错误，等待人工干预，**不自动二次回滚**。

## 十二、Nginx 配置模板与注入槽

### 12.1 模板策略

首版 Rust 模板不直接复用 Bash 的 `sed` 替换流程，而是使用 `minijinja`。

#### 12.1.1 模板加载方式

模板源文件位于源码目录 `tui-next/templates/*.j2`，**通过 `include_str!` 在编译期嵌入二进制**，运行时不读取磁盘。这样保证：

- 单二进制发布，无运行时模板依赖。
- 无法被部署环境意外修改或缺失。
- 与仓库根目录 `templates/` 中 Bash 版模板（`*.template`）解耦，互不影响（两套并行至 Bash 版退役）。

参考实现：

```rust
const PROXY_TPL: &str = include_str!("../templates/proxy.conf.j2");
const EMBY_TPL: &str  = include_str!("../templates/emby.conf.j2");
const STATIC_TPL: &str = include_str!("../templates/static.conf.j2");
```

注入槽预定义片段（`templates/snippets/*.j2`）同样使用 `include_str!` 加载，并在启动时注册到一个静态片段表。

> **与 Bash 版模板的关系：** Bash 版 `templates/*.template` 在 Bash 工具退役前保持不动，TUI 不读、不写、不引用。新增模板能力只在 `tui-next/templates/` 维护。

#### 12.1.2 模板变量

| 变量 | 说明 |
|------|------|
| `site_name` | 站点名 |
| `domain_name` | server_name |
| `upstream_scheme` | `http` 或 `https` |
| `upstream_target` | 解析后的上游地址 |
| `upstream_host` | HTTPS 上游 SNI 使用 |
| `static_root` | 静态站点根目录 |
| `custom_before_location` | server 级注入槽 |
| `custom_inside_location` | location 内注入槽 |
| `custom_after_location` | location 后注入槽 |

### 12.2 注入槽标记

模板必须包含稳定标记：

```nginx
# nginx-tools:custom-before-location:start
# nginx-tools:custom-before-location:end

# nginx-tools:custom-inside-location:start
# nginx-tools:custom-inside-location:end

# nginx-tools:custom-after-location:start
# nginx-tools:custom-after-location:end
```

解析规则：

- 标记存在：提取标记之间内容。
- 标记缺失：进入兼容模式，只解析标准字段，不覆盖未知自定义内容。
- 兼容模式保存表单时必须提示：`该配置缺少注入槽标记，表单保存可能覆盖自定义配置，建议使用原始配置模式`。

### 12.3 原始配置编辑

原始配置模式：

- 内置编辑器使用 `tui-textarea`。
- `Ctrl+S`：保存、测试、通过后 reload。
- `Ctrl+W`：仅保存，不测试不 reload。
- `o`：返回表单模式时重新解析文件。
- `$EDITOR`：作为高级入口，执行期间 TUI 暂停 raw mode，编辑器退出后恢复并重新读取文件。

## 十三、系统命令执行器

### 13.1 CommandExecutor

所有外部命令必须通过统一接口：

```rust
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
    pub timeout: Option<Duration>,
    pub require_root: bool,
}

pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}
```

### 13.2 安全规则

- 不通过 shell 拼接执行用户输入。
- 命令和参数分离传入。
- 站点名、域名、路径必须先校验再进入命令参数。
- 长任务支持取消。
- stderr 必须保留给 UI 展示。

## 十四、错误模型

错误分为：

```rust
pub enum NgToolError {
    PermissionDenied { operation: String },
    DependencyMissing { name: String },
    InvalidInput { field: String, message: String },
    NginxTestFailed { output: String },
    CommandFailed { command: String, code: Option<i32>, stderr: String },
    FileOperationFailed { path: PathBuf, message: String },
    TemplateFailed { message: String },
    ParseFailed { target: String, message: String },
    Cancelled,
}
```

展示规则：

- 用户输入错误：字段下方显示。
- 系统操作错误：主视图输出区显示完整信息。
- 权限错误：红色提示，保留当前页面。
- 后台任务错误：任务输出区展示，并弹出失败通知。
- 解析失败：页面降级显示，不能导致 TUI 崩溃。

## 十五、文件写入与回滚

### 15.0 通用约束

- **原子替换**：所有目标文件写入必须先写到 `~/.local/ngtool/tmp/`，再用 `rename(2)` 替换目标，避免读到半写状态。
- **mtime 并发保护**：编辑、原始配置、还原等会改写既有文件的操作，必须在"读取阶段"记录目标文件的 `mtime`，在"写入阶段"再次读取并比较；不一致表示文件被外部进程修改（另一个 TUI 实例、外部编辑器、手工 vim 等），此时必须中止本次保存并提示：`目标文件已被外部修改（mtime 变化），保存被取消。请按 Ctrl+R 重新加载或退出后再试`。
- **单实例软锁**：启动时尝试在 `~/.local/ngtool/tui.lock` 上获取 `flock`；若已被占用，提示当前已有实例运行并降级为只读模式打开。锁文件在进程退出时释放。
- **临时目录回收**：每次启动时清理 `~/.local/ngtool/tmp/` 中超过 7 天的残留文件。

### 15.1 新建站点

写入顺序：

1. 渲染模板到内存。
2. 写入临时文件 `~/.local/ngtool/tmp/<site>.conf`，并 `fsync`。
3. 校验目标文件 `/etc/nginx/sites-available/<site>.conf` 不存在（race 检测：用 `O_CREAT | O_EXCL`）。
4. 原子重命名到目标路径。
5. 如选择启用，创建符号链接。
6. 执行 `nginx -t`。
7. 通过后 `systemctl reload nginx`。
8. 任何步骤失败时，按 design.md 第 II 节"创建流程"中的反向次序回滚。

### 15.2 编辑站点

写入顺序：

1. 读取原文件，记录 `mtime_before`。
2. 保存原文件快照到 `~/.local/ngtool/tmp/rollback/<site>-<timestamp>.conf`。
3. 写入临时文件并 `fsync`。
4. **mtime 复检**：再次读取目标 `mtime_now`，若 `mtime_now != mtime_before` 立即中止并提示外部修改。
5. 原子替换目标文件。
6. 如果 `Ctrl+S`，执行 `nginx -t`。
7. 测试失败时使用快照恢复原文件，再次 `nginx -t`，若仍失败保留两份备份并报告详细错误。
8. 测试通过后 reload。

`Ctrl+W` 仅保存时：

- 不执行 `nginx -t`。
- 不 reload。
- 仍需执行 mtime 复检，避免静默覆盖外部修改。
- 必须显示黄色提示：`已保存但未测试配置，请手动测试后再重载`。

### 15.3 启用/停用站点

启用：

1. 创建符号链接（用 `symlink(2)`，存在则报错）。
2. 执行 `nginx -t`。
3. 失败则删除符号链接，再次 `nginx -t` 验证回滚后状态。
4. 通过则 reload。

停用：

1. 记录原符号链接的 target。
2. 删除符号链接。
3. 执行 `nginx -t`。
4. 失败则按记录的 target 恢复符号链接，再次 `nginx -t`。
5. 通过则 reload。

## 十六、测试策略

### 16.1 单元测试

必须覆盖：

- 站点名校验。
- 域名校验。
- 代理目标解析。
- Nginx 配置字段解析。
- 注入槽提取与回填。
- certbot 输出解析。
- 备份文件名解析。

### 16.2 集成测试

使用 fixture 目录模拟：

```text
tests/fixtures/nginx/
├── sites-available/
├── sites-enabled/
├── logs/
└── certbot/
```

集成测试覆盖：

- 列出站点。
- 启用/停用站点的文件变化。
- 新建站点模板输出。
- 编辑站点保留注入槽。
- 备份压缩和解压。

### 16.3 手工验收

每个视图完成后至少执行：

- 终端尺寸变化测试（含 80×24 最小尺寸）。
- 只读模式测试（非 root 启动 + 显式 `--readonly`）。
- root 读写模式测试。
- 快捷键冲突检查。
- 错误提示检查。
- 长任务期间输入响应检查（启动 `certbot renew` 模拟时仍能 `Esc` 返回）。

### 16.4 CJK 冒烟测试

由于 `tui-textarea` 与 ratatui 对 CJK 双宽字符历史上存在 corner case，每次涉及输入框的代码改动后必须执行：

| 场景 | 预期 |
|------|------|
| 站点名输入框输入纯中文 | 不可通过校验，提示"只能包含字母、数字、连字符" |
| 注入槽中输入中英文混排注释 | 光标位置正确，逐字删除不留残影 |
| 编辑器原始模式粘贴含中文的配置 | 行号对齐、宽度计算正确，无重叠 |
| 终端宽度 80 列下显示长中文路径 | 自动截断或换行，不溢出表格列 |
| `LANG=C` 环境启动 | 中文文案至少能展示为可读字符（推荐使用 `LANG=zh_CN.UTF-8`，但启动时检测到非 UTF-8 locale 给出提示） |

发现 corner case 时优先在文档中记录边界，而非临时补丁；若问题严重则评估替换为 `tui-input` + 自绘的备选方案。

## 十七、信号与终端处理

### 17.1 信号

| 信号 | 行为 |
|------|------|
| `SIGWINCH` | 终端尺寸变化，触发整屏重绘；保留当前路由和焦点；若新尺寸小于最低要求（80×24），降级显示提示横幅但不退出 |
| `SIGINT` (Ctrl+C) | 优雅退出：取消所有后台任务、释放 `tui.lock`、关闭 alternate screen、恢复原终端模式；有未保存修改时**不**询问，直接丢弃（保留 Ctrl+C 的"硬退出"语义） |
| `SIGTERM` | 与 `SIGINT` 相同处理路径，但额外写入审计日志一条退出原因 |
| `SIGHUP` | 同 `SIGTERM` |
| `SIGPIPE` | 忽略，由命令执行器在写 stdin 失败时返回 `BrokenPipe` 错误 |

`q` 键退出走"软退出"路径：有未保存修改时弹出确认弹窗。

### 17.2 终端模式切换

进入 raw mode + alternate screen 在程序启动时一次完成。以下情况必须临时退出 raw mode：

| 场景 | 处理 |
|------|------|
| 调用 `$EDITOR` | 退出 alternate screen → 退出 raw mode → fork 编辑器并等待 → 重新进入 raw mode → 重新进入 alternate screen → 强制重绘 → 重新读取目标文件并提示是否检测到外部变化 |
| 调用 `systemctl status nginx`（带分页器） | 通过执行器以 `--no-pager` 参数调用，不切换终端模式 |
| panic | 在 `panic_hook` 中尝试恢复终端模式后再打印 panic 信息，避免用户看到乱码终端 |

### 17.3 最小终端尺寸

设计文档对各页表格列宽有要求，运行时按下列阈值处理：

- 宽度 < 80 或高度 < 24：仅显示提示横幅"终端尺寸过小，请放大窗口至 80×24 以上"，不渲染主界面，仍接受 `q` 退出。
- 80×24 ≤ 尺寸 < 100×30：表格列采用截断模式，长域名/路径在末尾显示 `…`。
- 尺寸 ≥ 100×30：完整布局。

## 十八、操作审计日志

### 18.1 目标

工具会执行 nginx 配置变更、服务 reload/restart、证书申请等敏感操作，必须留下操作记录便于事后排查。审计日志独立于 `tracing` 应用日志，专门记录写操作。

### 18.2 路径与格式

文件：`~/.local/ngtool/logs/audit.log`，每行一条 JSON：

```json
{"ts":"2026-04-29T22:05:13+08:00","actor":"orangepi","mode":"read-write","action":"site.create","target":"app","result":"success","duration_ms":230,"details":{"domain":"app.example.com","enable":true,"cert":false}}
```

字段：

| 字段 | 说明 |
|------|------|
| `ts` | RFC 3339 时间戳 |
| `actor` | 执行用户（`whoami`） |
| `mode` | `read-write` / `read-only` |
| `action` | 操作类型枚举（见 18.3） |
| `target` | 操作对象（站点名、备份文件名、域名等） |
| `result` | `success` / `failure` / `cancelled` |
| `duration_ms` | 操作耗时 |
| `details` | 结构化扩展字段，按 action 不同 |

### 18.3 必须记录的操作

- `site.create`、`site.edit`、`site.delete`
- `site.enable`、`site.disable`
- `service.test`、`service.reload`、`service.restart`
- `cert.request`、`cert.renew`
- `backup.create`、`backup.restore`、`backup.delete`
- `editor.external`（调用 `$EDITOR` 进入与退出，分两条）

### 18.4 滚动与保留

- 单文件超过 5 MB 时自动滚动到 `audit.log.1`，最多保留 5 份（约 25 MB）。
- 启动时检查文件权限为 `0600`，否则修正。
- 删除审计日志不影响工具运行，但 UI 在"关于"页面显示日志路径供运维查看。

## 十九、MVP 边界

MVP 目标不是一次性完成全部细节，而是建立稳定架构并优先替代高频能力。

MVP 包含：

1. TUI 框架和全局布局。
2. 仪表盘。
3. 站点列表。
4. 新建站点（含可选启用，**含可选证书申请的简易模式**：不依赖完整证书管理页面，仅在新建流程内调用 `certbot --nginx`）。
5. 启用/停用站点。
6. 服务控制：测试、重载、状态、检查 Release、更新 TUI 二进制。
7. 日志实时查看。

MVP 暂缓：

- 完整证书管理页面（列表、续签、孤立证书、自动续签状态检查）。
- 备份还原。
- 高级注入槽片段库。
- 原始配置内置编辑器的全部快捷键。
- 自动续签修复建议。

## 二十、架构约束清单

实现时必须遵守：

1. 不修改 `design.md` 的交互原则。
2. 右侧主视图覆盖模式不能全屏接管。
3. 弹窗只用于高危确认、结果提示、必要输入。
4. UI 层不得直接执行系统命令。
5. 外部命令不得通过 shell 字符串拼接用户输入。
6. 只读模式必须可用。
7. 快速提交必须校验。
8. 证书申请必须绑定已有站点。
9. `systemctl` 首版固定，不做非 systemd 兼容。
10. 配置和备份路径使用 `~/.local/ngtool`。
11. 写配置必须有回滚或明确的失败状态，并通过 mtime 检测外部并发修改（参见 §15.0）。
12. 长任务必须异步，不能阻塞 TUI。
13. 模板通过 `include_str!` 编译期内嵌，不读取运行时模板文件（参见 §12.1.1）。
14. 备份和还原范围限定为 `nginx.conf` + `sites-available/*.conf` + `sites-enabled` 启用关系，**不**整目录覆盖（参见 §11.7.1）。
15. 所有写操作必须写入审计日志（参见 §18.3）。
16. 仪表盘数据采集必须并发执行并设单命令超时（参见 §11.2）。
17. TUI 自升级必须只使用 GitHub Release 中匹配当前架构的 `ngtool-*-linux-<arch>` asset，并在写入前校验 ELF 文件头。

---

*版本: v0.2 · 状态: 工程草案 · 日期: 2026-04-29*

*变更记录：*
- *v0.2: 备份还原范围限定到 `nginx.conf` + `sites-available` + 启用关系，新增 manifest；模板通过 `include_str!` 编译期内嵌；新增 §17 信号与终端处理、§18 操作审计日志；§15 加入 mtime 并发保护和单实例软锁；§11.2 仪表盘并发刷新和超时预算；§9 权限模型加入扫描失败降级；§16 加入 CJK 冒烟测试；config.toml 扩充 command/backup/audit 段；MVP §19 明确包含简易证书申请*
- *v0.1: 初版，承接 design.md v0.5*
