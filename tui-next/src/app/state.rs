use crate::ui::modal::Modal;
use crate::ui::theme;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::event::AppEvent;
use crate::app::route::{MenuItem, Route, SitesRoute};
use crate::domain::dashboard::DashboardSnapshot;
use crate::domain::log::LogSource;
use crate::domain::site::Site;
use crate::domain::update::UpdateInfo;
use crate::infra::AppContext;

/// 仪表盘自动刷新间隔。详见 design.md 视图 1 与 architecture.md §11.2。
const DASHBOARD_AUTO_REFRESH: Duration = Duration::from_secs(30);
/// 站点列表自动刷新间隔（保守一些，避免频繁打扰 certbot）。
const SITES_AUTO_REFRESH: Duration = Duration::from_secs(60);
/// 证书页自动刷新间隔。certbot certificates 较慢，给久一些。
const CERTS_AUTO_REFRESH: Duration = Duration::from_secs(120);

/// 运行模式：读写或只读，详见 architecture.md §7.2
#[derive(Debug, Clone)]
pub enum RunMode {
    ReadWrite,
    ReadOnly {
        #[allow(dead_code)]
        reason: String,
    },
}

impl RunMode {
    #[allow(dead_code)]
    pub fn is_readonly(&self) -> bool {
        matches!(self, RunMode::ReadOnly { .. })
    }

    pub fn label(&self) -> &str {
        match self {
            RunMode::ReadWrite => "读写",
            RunMode::ReadOnly { .. } => "只读",
        }
    }
}

/// 焦点区域：左侧菜单或右侧主视图
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    Sidebar,
    Content,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Success,
    Failure,
    Info,
}

impl NotificationKind {
    pub fn fg(&self) -> ratatui::style::Color {
        match self {
            NotificationKind::Success => theme::FG_OK,
            NotificationKind::Failure => theme::FG_ERR,
            NotificationKind::Info => theme::FG_PATH,
        }
    }

    pub fn glyph(&self) -> &'static str {
        match self {
            NotificationKind::Success => "✓",
            NotificationKind::Failure => "✗",
            NotificationKind::Info => "ℹ",
        }
    }
}

/// 操作结果提示，详见 design.md §三 操作结果提示
#[derive(Debug, Clone)]
pub struct Notification {
    pub kind: NotificationKind,
    pub message: String,
    pub expires_at: Instant,
}

#[allow(dead_code)]
impl Notification {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Success,
            message: message.into(),
            expires_at: Instant::now() + Duration::from_secs(2),
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Failure,
            message: message.into(),
            expires_at: Instant::now() + Duration::from_secs(3),
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Info,
            message: message.into(),
            expires_at: Instant::now() + Duration::from_secs(2),
        }
    }
}

/// 仪表盘子状态。snapshot 在每次刷新返回后更新；refreshing 标识异步任务在飞中；
/// pending_refresh 由按键、Tick 或 menu 切换设置，主循环消费后置 false。
#[derive(Debug, Default)]
pub struct DashboardState {
    pub snapshot: Option<DashboardSnapshot>,
    pub last_refresh: Option<Instant>,
    pub refreshing: bool,
    pub pending_refresh: bool,
}

/// 站点列表子状态。
#[derive(Debug, Default)]
pub struct SitesState {
    pub list: Vec<Site>,
    pub last_refresh: Option<Instant>,
    pub refreshing: bool,
    pub pending_refresh: bool,
    /// 表格内当前选中行
    pub selected: usize,
    /// 当前正在执行启停操作的站点名（操作期间禁用同一站点的二次触发）
    pub action_in_flight: Option<String>,
    /// 最近一次加载错误（如目录不可读）
    pub last_error: Option<String>,
    /// 待派发的启停请求（站点名 + 目标启用状态）
    pub pending_toggle: Option<(String, bool)>,
    /// 待派发的删除请求（站点名）
    pub pending_delete: Option<String>,
}

impl SitesState {
    pub fn move_cursor(&mut self, delta: i32) {
        if self.list.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.list.len() as i32;
        let mut idx = self.selected as i32 + delta;
        if idx < 0 {
            idx = len - 1;
        } else if idx >= len {
            idx = 0;
        }
        self.selected = idx as usize;
    }

    pub fn current(&self) -> Option<&Site> {
        self.list.get(self.selected)
    }
}

/// 服务控制视图所选按钮。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ServiceButton {
    #[default]
    Test,
    Reload,
    Restart,
    Status,
    CheckUpdate,
}

impl ServiceButton {
    pub const ALL: [ServiceButton; 5] = [
        ServiceButton::Test,
        ServiceButton::Reload,
        ServiceButton::Restart,
        ServiceButton::Status,
        ServiceButton::CheckUpdate,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            ServiceButton::Test => "测试配置",
            ServiceButton::Reload => "重载配置",
            ServiceButton::Restart => "重启服务 ⚠",
            ServiceButton::Status => "查看状态",
            ServiceButton::CheckUpdate => "检查更新",
        }
    }
}

/// 证书页焦点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CertsFocus {
    /// 证书表格（↑↓ 选中证书）
    #[default]
    Table,
    /// 站点选择器（用于申请证书）
    SiteSelector,
    /// 操作按钮（申请 / 续期 / 检查自动续签）
    ActionButtons,
}

/// 证书页操作按钮
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CertsAction {
    #[default]
    Request,
    RenewAll,
    CheckAutoRenew,
    InstallDeployHook,
}

impl CertsAction {
    pub const ALL: [CertsAction; 4] = [
        CertsAction::Request,
        CertsAction::RenewAll,
        CertsAction::CheckAutoRenew,
        CertsAction::InstallDeployHook,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            CertsAction::Request => "申请新证书",
            CertsAction::RenewAll => "续期所有证书",
            CertsAction::CheckAutoRenew => "检查自动续签",
            CertsAction::InstallDeployHook => "安装 deploy hook",
        }
    }
}

/// 证书页子状态
#[derive(Debug, Default)]
pub struct CertsState {
    pub list: Vec<crate::domain::cert::CertWithSite>,
    /// certbot 原始输出，解析失败时供 UI 展示（R2）
    pub raw_output: Option<String>,
    pub auto_renew: Option<crate::domain::cert::AutoRenewStatus>,
    pub last_refresh: Option<Instant>,
    pub refreshing: bool,
    pub pending_refresh: bool,
    pub selected: usize,
    pub last_error: Option<String>,
    pub focused: CertsFocus,
    /// 站点选择器中指向的站点索引（依赖 SitesState.list）
    pub site_selector_index: usize,
    /// 操作按钮当前焦点
    pub action_focus: CertsAction,
    /// 操作输出区，展示 certbot 流式输出
    pub output: Vec<String>,
    /// 操作进行中标志
    pub running: Option<CertsAction>,
    /// 待派发：申请证书（站点名 + 域名列表）
    pub pending_request: Option<(String, Vec<String>)>,
    /// 待派发：续签全部
    pub pending_renew: bool,
    /// 待派发：自动续签检查
    pub pending_check_renew: bool,
    /// 待派发：安装 deploy hook
    pub pending_install_hook: bool,
}

impl CertsState {
    pub fn move_cursor(&mut self, delta: i32) {
        if self.list.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.list.len() as i32;
        let mut idx = self.selected as i32 + delta;
        if idx < 0 {
            idx = len - 1;
        } else if idx >= len {
            idx = 0;
        }
        self.selected = idx as usize;
    }

    pub fn cycle_action(&mut self, delta: i32) {
        let len = CertsAction::ALL.len() as i32;
        let cur = CertsAction::ALL
            .iter()
            .position(|a| *a == self.action_focus)
            .map(|x| x as i32)
            .unwrap_or(0);
        let next = (cur + delta).rem_euclid(len) as usize;
        self.action_focus = CertsAction::ALL[next];
    }

    pub fn push_output(&mut self, lines: impl IntoIterator<Item = String>) {
        let limit = 200usize;
        for line in lines {
            self.output.push(line);
        }
        if self.output.len() > limit {
            let drop = self.output.len() - limit;
            self.output.drain(0..drop);
        }
    }

    pub fn clear_output(&mut self) {
        self.output.clear();
    }
}

/// 备份页子状态
#[derive(Debug, Default)]
pub struct BackupState {
    pub list: Vec<crate::domain::backup::Backup>,
    pub selected: usize,
    pub last_refresh: Option<Instant>,
    pub refreshing: bool,
    pub pending_refresh: bool,
    pub last_error: Option<String>,
    /// 操作输出
    pub output: Vec<String>,
    pub running: bool,
    /// 待派发：创建备份
    pub pending_create: bool,
    /// 待派发：删除指定备份
    pub pending_delete: Option<std::path::PathBuf>,
    /// 待派发：还原指定备份
    pub pending_restore: Option<std::path::PathBuf>,
}

impl BackupState {
    pub fn move_cursor(&mut self, delta: i32) {
        if self.list.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.list.len() as i32;
        let mut idx = self.selected as i32 + delta;
        if idx < 0 {
            idx = len - 1;
        } else if idx >= len {
            idx = 0;
        }
        self.selected = idx as usize;
    }

    pub fn current(&self) -> Option<&crate::domain::backup::Backup> {
        self.list.get(self.selected)
    }

    pub fn push_output(&mut self, lines: impl IntoIterator<Item = String>) {
        let limit = 200usize;
        for line in lines {
            self.output.push(line);
        }
        if self.output.len() > limit {
            let drop = self.output.len() - limit;
            self.output.drain(0..drop);
        }
    }

    pub fn clear_output_buffer(&mut self) {
        self.output.clear();
    }
}

/// 服务控制视图子状态。
#[derive(Debug, Default)]
pub struct ServiceState {
    pub focused: ServiceButton,
    pub output: Vec<String>,
    pub running: Option<ServiceButton>,
    pub update_info: Option<UpdateInfo>,
    /// 待派发的操作意图
    pub pending_action: Option<ServiceButton>,
    /// 弹窗确认后的升级意图
    pub pending_upgrade: bool,
}

/// 站点类型选项
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SiteTypeChoice {
    #[default]
    Proxy,
    Emby,
    Static,
}

impl SiteTypeChoice {
    pub const ALL: [SiteTypeChoice; 3] = [
        SiteTypeChoice::Proxy,
        SiteTypeChoice::Emby,
        SiteTypeChoice::Static,
    ];
    pub fn label(&self) -> &'static str {
        match self {
            SiteTypeChoice::Proxy => "反向代理（通用）",
            SiteTypeChoice::Emby => "反向代理（Emby/Jellyfin）",
            SiteTypeChoice::Static => "静态站点",
        }
    }
}

/// 表单字段焦点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormField {
    #[default]
    SiteName,
    Domain,
    DomainAliases,
    SiteType,
    Target,
    EnableCheckbox,
    CertCheckbox,
    SubmitButton,
}

impl FormField {
    pub const ORDER: [FormField; 8] = [
        FormField::SiteName,
        FormField::Domain,
        FormField::DomainAliases,
        FormField::SiteType,
        FormField::Target,
        FormField::EnableCheckbox,
        FormField::CertCheckbox,
        FormField::SubmitButton,
    ];
}

/// 新建站点表单状态
#[derive(Debug, Default)]
pub struct SiteFormState {
    pub site_name: String,
    pub domain: String,
    pub domain_aliases: String,
    pub site_type: SiteTypeChoice,
    pub target: String,
    pub enable_now: bool,
    pub request_cert: bool,
    pub focused: FormField,
    /// 字段错误提示（字段名 → 错误消息）
    pub field_errors: std::collections::HashMap<String, String>,
    /// 是否正在提交
    pub submitting: bool,
    /// 待派发的创建请求
    pub pending_create: Option<crate::domain::site::CreateSiteInput>,
}

impl SiteFormState {
    pub fn move_focus(&mut self, delta: i32) {
        let len = FormField::ORDER.len() as i32;
        let cur = FormField::ORDER
            .iter()
            .position(|f| *f == self.focused)
            .map(|x| x as i32)
            .unwrap_or(0);
        let mut next_idx = (cur + delta).rem_euclid(len) as usize;
        // 静态站点时跳过 Target 字段（检查目标而非当前位置）
        if self.site_type == SiteTypeChoice::Static
            && FormField::ORDER[next_idx] == FormField::Target
        {
            next_idx = (cur + delta * 2).rem_euclid(len) as usize;
        }
        self.focused = FormField::ORDER[next_idx];
    }

    pub fn toggle_site_type(&mut self, delta: i32) {
        let len = SiteTypeChoice::ALL.len() as i32;
        let cur = SiteTypeChoice::ALL
            .iter()
            .position(|t| *t == self.site_type)
            .map(|x| x as i32)
            .unwrap_or(0);
        let next = (cur + delta).rem_euclid(len) as usize;
        self.site_type = SiteTypeChoice::ALL[next];
    }

    pub fn clear_errors(&mut self) {
        self.field_errors.clear();
    }

    pub fn set_error(&mut self, field: &str, msg: String) {
        self.field_errors.insert(field.into(), msg);
    }

    pub fn get_error(&self, field: &str) -> Option<&String> {
        self.field_errors.get(field)
    }

    pub fn has_errors(&self) -> bool {
        !self.field_errors.is_empty()
    }

    /// 验证所有字段，返回是否通过
    pub fn validate(&mut self) -> bool {
        self.clear_errors();

        // 站点名称
        if let Err(e) = crate::template::renderer::validate_site_name(&self.site_name) {
            self.set_error("site_name", e);
        }

        // 域名
        if let Err(e) = crate::template::renderer::validate_domain(&self.domain) {
            self.set_error("domain", e);
        }

        // 附加域名（逗号或空格分隔的多个域名，逐个验证）
        for alias in split_aliases(&self.domain_aliases) {
            if let Err(e) = crate::template::renderer::validate_domain(alias) {
                self.set_error("domain_aliases", format!("附加域名 '{}': {}", alias, e));
                break;
            }
        }

        // 代理目标（非静态站点）
        if self.site_type != SiteTypeChoice::Static {
            if self.target.trim().is_empty() {
                self.set_error("target", "目标地址不能为空".into());
            } else if let Err(e) = crate::template::renderer::parse_upstream(&self.target) {
                self.set_error("target", e);
            }
        }

        // 证书申请必须依赖立即启用
        if self.request_cert && !self.enable_now {
            self.set_error("cert_checkbox", "申请证书需要先启用站点".into());
        }

        !self.has_errors()
    }

    /// 构建 CreateSiteInput
    pub fn build_input(&self) -> Option<crate::domain::site::CreateSiteInput> {
        let target_parsed = if self.site_type == SiteTypeChoice::Static {
            None
        } else {
            crate::template::renderer::parse_upstream(&self.target).ok()
        };

        let (upstream_scheme, upstream_target) = target_parsed.unwrap_or_default();

        // 规范化别名：逗号/空格分隔 → 空格连接（兼容 nginx server_name 语法）
        let aliases = self.domain_aliases.trim().to_string();
        let aliases_normalized = split_aliases(&aliases).join(" ");

        Some(crate::domain::site::CreateSiteInput {
            name: self.site_name.trim().to_string(),
            domain: self.domain.trim().to_string(),
            domain_aliases: aliases_normalized,
            kind: match self.site_type {
                SiteTypeChoice::Proxy => crate::template::renderer::SiteKind::Proxy,
                SiteTypeChoice::Emby => crate::template::renderer::SiteKind::Emby,
                SiteTypeChoice::Static => crate::template::renderer::SiteKind::Static,
            },
            upstream_scheme,
            upstream_target,
            static_root: if self.site_type == SiteTypeChoice::Static {
                format!("/var/www/{}", self.site_name.trim())
            } else {
                String::new()
            },
            enable_now: self.enable_now,
            request_cert: self.request_cert,
        })
    }
}

/// 切分逗号或空格分隔的域名字符串，返回非空域名列表。
fn split_aliases(input: &str) -> Vec<&str> {
    input
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

impl ServiceState {
    pub fn move_focus(&mut self, delta: i32) {
        let len = ServiceButton::ALL.len() as i32;
        let cur = ServiceButton::ALL
            .iter()
            .position(|b| *b == self.focused)
            .map(|x| x as i32)
            .unwrap_or(0);
        let next = (cur + delta).rem_euclid(len) as usize;
        self.focused = ServiceButton::ALL[next];
    }

    pub fn push_output(&mut self, lines: impl IntoIterator<Item = String>) {
        let limit = 200usize;
        for line in lines {
            self.output.push(line);
        }
        if self.output.len() > limit {
            let drop = self.output.len() - limit;
            self.output.drain(0..drop);
        }
    }

    pub fn clear_output(&mut self) {
        self.output.clear();
    }
}

/// 日志视图焦点区域
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogsFocus {
    #[default]
    SiteSelector,
    KindSelector,
    LogContent,
    SearchInput,
}

/// 日志视图子状态
#[derive(Debug)]
pub struct LogsState {
    /// 当前日志源
    pub source: LogSource,
    /// 焦点区域
    pub focused: LogsFocus,
    /// 日志行缓冲（环形队列）
    pub buffer: VecDeque<String>,
    /// 最大行数
    pub max_lines: usize,
    /// 是否暂停滚动（暂停时仍收集但不自动滚动）
    pub paused: bool,
    /// 搜索关键词
    pub search_query: Option<String>,
    /// 当前匹配索引（搜索结果中的位置）
    pub match_index: Option<usize>,
    /// 匹配行号列表
    pub match_lines: Vec<usize>,
    /// tail 任务句柄（用于取消）
    pub tail_handle: Option<tokio::task::JoinHandle<()>>,
    /// tail 输出接收通道
    pub tail_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::infra::log_tail::TailLine>>,
    /// 日志源变更请求（主循环消费后启动新 tail）
    pub pending_tail_change: bool,
}

impl Default for LogsState {
    fn default() -> Self {
        Self {
            source: LogSource::default(),
            focused: LogsFocus::default(),
            buffer: VecDeque::with_capacity(1000),
            max_lines: 1000,
            paused: false,
            search_query: None,
            match_index: None,
            match_lines: Vec::new(),
            tail_handle: None,
            tail_rx: None,
            pending_tail_change: false,
        }
    }
}

impl LogsState {
    /// 追加日志行，超出限制时丢弃最旧行
    pub fn push_line(&mut self, line: String) {
        if self.buffer.len() >= self.max_lines {
            self.buffer.pop_front();
        }
        self.buffer.push_back(line);
    }

    /// 清空缓冲区
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
        self.match_lines.clear();
        self.match_index = None;
    }

    /// 执行搜索，更新 match_lines
    pub fn search(&mut self, query: &str) {
        self.search_query = Some(query.to_string());
        self.match_lines.clear();
        self.match_index = None;

        if query.is_empty() {
            return;
        }

        for (i, line) in self.buffer.iter().enumerate() {
            if line.contains(query) {
                self.match_lines.push(i);
            }
        }

        if !self.match_lines.is_empty() {
            self.match_index = Some(0);
        }
    }

    /// 跳转到下一个匹配
    pub fn next_match(&mut self) {
        if self.match_lines.is_empty() || self.match_index.is_none() {
            return;
        }
        let cur = self.match_index.unwrap();
        let next = (cur + 1) % self.match_lines.len();
        self.match_index = Some(next);
    }

    /// 跳转到上一个匹配
    pub fn prev_match(&mut self) {
        if self.match_lines.is_empty() || self.match_index.is_none() {
            return;
        }
        let cur = self.match_index.unwrap();
        let prev = if cur == 0 {
            self.match_lines.len() - 1
        } else {
            cur - 1
        };
        self.match_index = Some(prev);
    }

    /// 清除搜索状态
    pub fn clear_search(&mut self) {
        self.search_query = None;
        self.match_lines.clear();
        self.match_index = None;
    }

    /// 切换暂停状态
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// 切换日志类型
    pub fn toggle_kind(&mut self) {
        self.source = self.source.toggle_kind();
    }

    /// 设置站点
    pub fn set_site(&mut self, name: Option<String>) {
        self.source = self.source.with_site(name);
    }

    /// 停止 tail 任务
    pub fn stop_tail(&mut self) {
        if let Some(handle) = self.tail_handle.take() {
            handle.abort();
        }
        self.tail_rx = None;
    }
}

/// 编辑器表单模式焦点区域
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditFocus {
    #[default]
    Domain,
    Target,
    Scheme,
    SlotSelector,
    SlotContent,
    TemplateList,
}

/// 站点编辑器状态
#[derive(Debug)]
pub struct SiteEditState {
    /// 正在编辑的站点名称
    pub site_name: String,
    /// 焦点区域
    pub focused: EditFocus,
    /// 域名
    pub domain: String,
    /// 代理目标（显示格式，如 "127.0.0.1:8080"）
    pub target: String,
    /// 上游协议 http/https
    pub upstream_scheme: String,
    /// 站点类型
    pub site_type: crate::domain::site::SiteType,
    /// 静态根目录
    pub static_root: String,
    /// 注入槽内容
    pub injection_slots:
        std::collections::HashMap<crate::template::config_parser::InjectionSlot, String>,
    /// 当前选中的注入槽
    pub current_slot: crate::template::config_parser::InjectionSlot,
    /// 注入槽标记是否完整
    pub markers_intact: bool,
    /// 原始编辑模式的光标行
    pub raw_cursor_line: usize,
    /// 原始编辑模式的光标列
    pub raw_cursor_col: usize,
    /// 原始编辑中每行的内容
    pub raw_lines: Vec<String>,
    /// 是否有未保存的修改
    pub dirty: bool,
    /// 是否正在保存
    pub saving: bool,
    /// 字段错误
    pub field_errors: std::collections::HashMap<String, String>,
    /// 选中模板索引
    pub template_index: usize,
    /// 待派发的保存请求
    pub pending_save: Option<crate::domain::site::SaveSiteInput>,
    /// 进入编辑时记录的目标文件 mtime，用于 §15.0 mtime 并发保护
    pub mtime_at_load: Option<std::time::SystemTime>,
    /// 进入编辑时密封的原始表单值，供 Ctrl+D 重置（design.md 子模式 C）
    pub original_snapshot: Option<Box<EditSnapshot>>,
    /// 原始模式撤销栈（design.md 子模式 D：Ctrl+Z）
    pub raw_undo_stack: Vec<RawSnapshot>,
    /// 原始模式重做栈（Ctrl+Y）
    pub raw_redo_stack: Vec<RawSnapshot>,
    /// 全屏槽位编辑：当前编辑的槽位（design.md 子模式 C：Ctrl+E）
    pub slot_edit_target: Option<crate::template::config_parser::InjectionSlot>,
    /// 全屏槽位编辑：当前文本（每行一个 String）
    pub slot_edit_lines: Vec<String>,
    /// 全屏槽位编辑：光标行
    pub slot_edit_cursor_line: usize,
    /// 全屏槽位编辑：光标列
    pub slot_edit_cursor_col: usize,
    /// 全屏槽位编辑：撤销栈
    pub slot_edit_undo: Vec<RawSnapshot>,
    /// 全屏槽位编辑：重做栈
    pub slot_edit_redo: Vec<RawSnapshot>,
}

/// 原始模式 / 全屏槽位编辑共用的撤销快照。
#[derive(Debug, Clone)]
pub struct RawSnapshot {
    pub lines: Vec<String>,
    pub cursor_line: usize,
    pub cursor_col: usize,
}

/// 表单模式重置（Ctrl+D）所用的快照：仅含会被表单修改的字段。
#[derive(Debug, Clone)]
pub struct EditSnapshot {
    pub domain: String,
    pub target: String,
    pub upstream_scheme: String,
    pub static_root: String,
    pub injection_slots:
        std::collections::HashMap<crate::template::config_parser::InjectionSlot, String>,
}

impl Default for SiteEditState {
    fn default() -> Self {
        Self {
            site_name: String::new(),
            focused: EditFocus::default(),
            domain: String::new(),
            target: String::new(),
            upstream_scheme: "http".into(),
            site_type: crate::domain::site::SiteType::Unknown,
            static_root: String::new(),
            injection_slots: std::collections::HashMap::new(),
            current_slot: crate::template::config_parser::InjectionSlot::BeforeLocation,
            markers_intact: true,
            raw_cursor_line: 0,
            raw_cursor_col: 0,
            raw_lines: Vec::new(),
            dirty: false,
            saving: false,
            field_errors: std::collections::HashMap::new(),
            template_index: 0,
            pending_save: None,
            mtime_at_load: None,
            original_snapshot: None,
            raw_undo_stack: Vec::new(),
            raw_redo_stack: Vec::new(),
            slot_edit_target: None,
            slot_edit_lines: Vec::new(),
            slot_edit_cursor_line: 0,
            slot_edit_cursor_col: 0,
            slot_edit_undo: Vec::new(),
            slot_edit_redo: Vec::new(),
        }
    }
}

impl SiteEditState {
    /// 从解析结果初始化编辑状态
    pub fn from_parsed(
        site_name: &str,
        parsed: &crate::template::config_parser::ParsedForEdit,
    ) -> Self {
        let raw_lines: Vec<String> = parsed.raw_content.lines().map(String::from).collect();
        Self {
            site_name: site_name.to_string(),
            domain: parsed.domains.first().cloned().unwrap_or_default(),
            target: parsed.upstream_target.clone().unwrap_or_default(),
            upstream_scheme: parsed
                .upstream_scheme
                .clone()
                .unwrap_or_else(|| "http".into()),
            site_type: parsed.site_type,
            static_root: parsed.static_root.clone().unwrap_or_default(),
            injection_slots: parsed.injection_slots.clone(),
            markers_intact: parsed.markers_intact,
            raw_lines,
            ..Default::default()
        }
    }

    pub fn set_error(&mut self, field: &str, msg: String) {
        self.field_errors.insert(field.into(), msg);
    }

    pub fn clear_errors(&mut self) {
        self.field_errors.clear();
    }

    pub fn has_errors(&self) -> bool {
        !self.field_errors.is_empty()
    }

    /// 验证编辑表单
    pub fn validate(&mut self) -> bool {
        self.clear_errors();
        if self.domain.trim().is_empty() {
            self.set_error("domain", "域名不能为空".into());
        } else if let Err(e) = crate::template::renderer::validate_domain(&self.domain) {
            self.set_error("domain", e);
        }
        if self.site_type != crate::domain::site::SiteType::Static {
            if self.target.trim().is_empty() {
                self.set_error("target", "目标地址不能为空".into());
            } else if let Err(e) = crate::template::renderer::parse_upstream(&self.target) {
                self.set_error("target", e);
            }
        }
        !self.has_errors()
    }

    /// 构建模板渲染参数
    pub fn build_render_params(&self) -> crate::template::renderer::RenderParams {
        let (upstream_scheme, upstream_target) =
            if self.site_type == crate::domain::site::SiteType::Static {
                (String::new(), String::new())
            } else {
                crate::template::renderer::parse_upstream(&self.target)
                    .unwrap_or_else(|_| (self.upstream_scheme.clone(), self.target.clone()))
            };

        crate::template::renderer::RenderParams {
            site_name: self.site_name.clone(),
            domain_name: self.domain.trim().to_string(),
            domain_aliases: String::new(),
            upstream_scheme,
            upstream_target,
            static_root: self.static_root.clone(),
            custom_before_location: self
                .injection_slots
                .get(&crate::template::config_parser::InjectionSlot::BeforeLocation)
                .cloned()
                .unwrap_or_default(),
            custom_inside_location: self
                .injection_slots
                .get(&crate::template::config_parser::InjectionSlot::InsideLocation)
                .cloned()
                .unwrap_or_default(),
            custom_after_location: self
                .injection_slots
                .get(&crate::template::config_parser::InjectionSlot::AfterLocation)
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// 获取模板类型
    pub fn site_kind(&self) -> crate::template::renderer::SiteKind {
        match self.site_type {
            crate::domain::site::SiteType::Emby => crate::template::renderer::SiteKind::Emby,
            crate::domain::site::SiteType::Static => crate::template::renderer::SiteKind::Static,
            _ => crate::template::renderer::SiteKind::Proxy,
        }
    }

    /// Tab 切换焦点
    pub fn move_focus_forward(&mut self) {
        self.focused = match self.focused {
            EditFocus::Domain => EditFocus::Target,
            EditFocus::Target => EditFocus::Scheme,
            EditFocus::Scheme => EditFocus::SlotSelector,
            EditFocus::SlotSelector => EditFocus::SlotContent,
            EditFocus::SlotContent => EditFocus::TemplateList,
            EditFocus::TemplateList => EditFocus::Domain,
        };
        // 静态站点跳过 Target 和 Scheme
        if self.site_type == crate::domain::site::SiteType::Static
            && (self.focused == EditFocus::Target || self.focused == EditFocus::Scheme)
        {
            self.focused = EditFocus::SlotSelector;
        }
    }

    pub fn move_focus_backward(&mut self) {
        self.focused = match self.focused {
            EditFocus::Domain => EditFocus::TemplateList,
            EditFocus::Target => EditFocus::Domain,
            EditFocus::Scheme => EditFocus::Target,
            EditFocus::SlotSelector => EditFocus::Scheme,
            EditFocus::SlotContent => EditFocus::SlotSelector,
            EditFocus::TemplateList => EditFocus::SlotContent,
        };
        if self.site_type == crate::domain::site::SiteType::Static
            && (self.focused == EditFocus::Scheme || self.focused == EditFocus::Target)
        {
            self.focused = EditFocus::Domain;
        }
    }

    /// 切换注入槽
    pub fn cycle_slot(&mut self, delta: i32) {
        let slots = crate::template::config_parser::InjectionSlot::ALL;
        let cur = slots
            .iter()
            .position(|s| *s == self.current_slot)
            .unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(slots.len() as i32) as usize;
        self.current_slot = slots[next];
    }

    /// 追加模板片段到当前注入槽
    pub fn append_snippet(&mut self, snippet: &str) {
        let slot = self.current_slot;
        let content = self.injection_slots.entry(slot).or_default();
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(snippet);
        self.dirty = true;
    }

    /// 替换当前注入槽内容为模板片段
    pub fn replace_with_snippet(&mut self, snippet: &str) {
        let slot = self.current_slot;
        self.injection_slots.insert(slot, snippet.to_string());
        self.dirty = true;
    }

    /// 进入编辑时密封当前表单字段，供 Ctrl+D 重置使用。
    pub fn seal_original(&mut self) {
        self.original_snapshot = Some(Box::new(EditSnapshot {
            domain: self.domain.clone(),
            target: self.target.clone(),
            upstream_scheme: self.upstream_scheme.clone(),
            static_root: self.static_root.clone(),
            injection_slots: self.injection_slots.clone(),
        }));
    }

    /// Ctrl+D：把表单字段恢复为进入编辑时的原值。
    /// 不影响 raw_lines（原始模式由 Ctrl+Z/Y 管理）。
    /// 返回是否真的执行了恢复（无原始快照时返回 false）。
    pub fn restore_original(&mut self) -> bool {
        let Some(snap) = self.original_snapshot.as_deref() else {
            return false;
        };
        self.domain = snap.domain.clone();
        self.target = snap.target.clone();
        self.upstream_scheme = snap.upstream_scheme.clone();
        self.static_root = snap.static_root.clone();
        self.injection_slots = snap.injection_slots.clone();
        self.field_errors.clear();
        self.dirty = false;
        true
    }

    /// 在原始模式做写操作前调用：把当前 raw_lines / 光标推入 undo 栈，并清空 redo 栈。
    /// 栈深上限 100，超过时丢弃栈底。
    pub fn push_raw_undo(&mut self) {
        const MAX: usize = 100;
        let snap = RawSnapshot {
            lines: self.raw_lines.clone(),
            cursor_line: self.raw_cursor_line,
            cursor_col: self.raw_cursor_col,
        };
        self.raw_undo_stack.push(snap);
        if self.raw_undo_stack.len() > MAX {
            self.raw_undo_stack.remove(0);
        }
        self.raw_redo_stack.clear();
    }

    /// Ctrl+Z：原始模式撤销
    pub fn raw_undo(&mut self) -> bool {
        if let Some(prev) = self.raw_undo_stack.pop() {
            let cur = RawSnapshot {
                lines: self.raw_lines.clone(),
                cursor_line: self.raw_cursor_line,
                cursor_col: self.raw_cursor_col,
            };
            self.raw_redo_stack.push(cur);
            self.raw_lines = prev.lines;
            self.raw_cursor_line = prev.cursor_line;
            self.raw_cursor_col = prev.cursor_col;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Ctrl+Y：原始模式重做
    pub fn raw_redo(&mut self) -> bool {
        if let Some(next) = self.raw_redo_stack.pop() {
            let cur = RawSnapshot {
                lines: self.raw_lines.clone(),
                cursor_line: self.raw_cursor_line,
                cursor_col: self.raw_cursor_col,
            };
            self.raw_undo_stack.push(cur);
            self.raw_lines = next.lines;
            self.raw_cursor_line = next.cursor_line;
            self.raw_cursor_col = next.cursor_col;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// 进入全屏槽位编辑：从 injection_slots 拷贝当前槽位内容到 slot_edit_lines。
    pub fn enter_slot_full(&mut self) {
        let slot = self.current_slot;
        let content = self.injection_slots.get(&slot).cloned().unwrap_or_default();
        self.slot_edit_lines = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(String::from).collect()
        };
        self.slot_edit_cursor_line = 0;
        self.slot_edit_cursor_col = 0;
        self.slot_edit_undo.clear();
        self.slot_edit_redo.clear();
        self.slot_edit_target = Some(slot);
    }

    /// 全屏槽位编辑：写操作前的快照
    pub fn push_slot_undo(&mut self) {
        const MAX: usize = 100;
        let snap = RawSnapshot {
            lines: self.slot_edit_lines.clone(),
            cursor_line: self.slot_edit_cursor_line,
            cursor_col: self.slot_edit_cursor_col,
        };
        self.slot_edit_undo.push(snap);
        if self.slot_edit_undo.len() > MAX {
            self.slot_edit_undo.remove(0);
        }
        self.slot_edit_redo.clear();
    }

    pub fn slot_undo(&mut self) -> bool {
        if let Some(prev) = self.slot_edit_undo.pop() {
            let cur = RawSnapshot {
                lines: self.slot_edit_lines.clone(),
                cursor_line: self.slot_edit_cursor_line,
                cursor_col: self.slot_edit_cursor_col,
            };
            self.slot_edit_redo.push(cur);
            self.slot_edit_lines = prev.lines;
            self.slot_edit_cursor_line = prev.cursor_line;
            self.slot_edit_cursor_col = prev.cursor_col;
            true
        } else {
            false
        }
    }

    pub fn slot_redo(&mut self) -> bool {
        if let Some(next) = self.slot_edit_redo.pop() {
            let cur = RawSnapshot {
                lines: self.slot_edit_lines.clone(),
                cursor_line: self.slot_edit_cursor_line,
                cursor_col: self.slot_edit_cursor_col,
            };
            self.slot_edit_undo.push(cur);
            self.slot_edit_lines = next.lines;
            self.slot_edit_cursor_line = next.cursor_line;
            self.slot_edit_cursor_col = next.cursor_col;
            true
        } else {
            false
        }
    }

    /// 全屏槽位编辑保存：把 slot_edit_lines 写回到 injection_slots，置 dirty。
    /// 返回写回的槽位（用于通知）；若 slot_edit_target 为空则返回 None。
    pub fn commit_slot_full(&mut self) -> Option<crate::template::config_parser::InjectionSlot> {
        let slot = self.slot_edit_target?;
        // 末尾空行去掉一个（编辑时保留的空收尾行不写回）
        let mut lines = self.slot_edit_lines.clone();
        if lines.last().map(|s| s.is_empty()).unwrap_or(false) && lines.len() > 1 {
            lines.pop();
        }
        let content = lines.join("\n");
        if content.is_empty() {
            self.injection_slots.remove(&slot);
        } else {
            self.injection_slots.insert(slot, content);
        }
        self.dirty = true;
        Some(slot)
    }
}

/// 全局应用状态。详见 architecture.md §7.1。
pub struct AppState {
    pub run_mode: RunMode,
    pub route: Route,
    pub focus: FocusArea,
    pub notification: Option<Notification>,
    pub modal: Option<Modal>,
    pub should_quit: bool,
    pub last_tick: Instant,
    pub ctx: Arc<AppContext>,
    pub dashboard: DashboardState,
    pub sites: SitesState,
    pub service: ServiceState,
    pub site_form: SiteFormState,
    pub logs: LogsState,
    pub site_edit: SiteEditState,
    pub certs: CertsState,
    pub backup: BackupState,
}

impl AppState {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        let run_mode = match &ctx.readonly_reason {
            Some(reason) => RunMode::ReadOnly {
                reason: reason.clone(),
            },
            None => RunMode::ReadWrite,
        };
        Self {
            run_mode,
            route: Route::Dashboard,
            focus: FocusArea::Sidebar,
            notification: None,
            modal: None,
            should_quit: false,
            last_tick: Instant::now(),
            ctx,
            dashboard: DashboardState {
                pending_refresh: true,
                ..Default::default()
            },
            sites: SitesState::default(),
            service: ServiceState::default(),
            site_form: SiteFormState::default(),
            logs: LogsState::default(),
            site_edit: SiteEditState::default(),
            certs: CertsState::default(),
            backup: BackupState::default(),
        }
    }

    pub fn current_menu(&self) -> MenuItem {
        match self.route {
            Route::Dashboard => MenuItem::Dashboard,
            Route::Sites(_) => MenuItem::Sites,
            Route::Certs => MenuItem::Certs,
            Route::Logs => MenuItem::Logs,
            Route::Service => MenuItem::Service,
            Route::Backup => MenuItem::Backup,
        }
    }

    pub fn handle_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Key(k) => self.handle_key(k),
            AppEvent::Tick => {
                self.last_tick = Instant::now();
                self.maybe_auto_refresh_dashboard();
                self.maybe_auto_refresh_sites();
                self.maybe_auto_refresh_certs();
            }
            AppEvent::Resize => {}
            AppEvent::QuitRequested => self.should_quit = true,
            AppEvent::DashboardSnapshot(s) => {
                self.dashboard.snapshot = Some(*s);
                self.dashboard.refreshing = false;
                self.dashboard.last_refresh = Some(Instant::now());
            }
            AppEvent::SitesLoaded(b) => {
                self.sites.refreshing = false;
                self.sites.last_refresh = Some(Instant::now());
                match *b {
                    Ok(list) => {
                        self.sites.last_error = None;
                        // 保持选中位置在范围内
                        if self.sites.selected >= list.len() {
                            self.sites.selected = list.len().saturating_sub(1);
                        }
                        self.sites.list = list;
                    }
                    Err(msg) => {
                        self.sites.last_error = Some(msg);
                    }
                }
            }
            AppEvent::SiteToggleResult {
                site,
                target_enabled,
                result,
            } => {
                self.sites.action_in_flight = None;
                match *result {
                    Ok(()) => {
                        let verb = if target_enabled {
                            "已启用"
                        } else {
                            "已停用"
                        };
                        self.notification =
                            Some(Notification::success(format!("站点 {} {}", site, verb)));
                        self.sites.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("操作失败：{}", e)));
                        self.sites.pending_refresh = true;
                    }
                }
            }
            AppEvent::SiteDeleteResult { site_name, result } => {
                self.sites.action_in_flight = None;
                match *result {
                    Ok(()) => {
                        self.notification =
                            Some(Notification::success(format!("站点 {} 已删除", site_name)));
                        self.sites.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("删除失败：{}", e)));
                        self.sites.pending_refresh = true;
                    }
                }
            }
            AppEvent::ServiceTestResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(out) => {
                        self.service.push_output(["── 测试配置 ──".into()]);
                        self.service.push_output(out.lines().map(String::from));
                        self.notification = Some(Notification::success("测试通过".to_string()));
                    }
                    Err(e) => {
                        self.service.push_output(["── 测试失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification =
                            Some(Notification::failure("nginx -t 失败".to_string()));
                    }
                }
            }
            AppEvent::ServiceReloadResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(out) => {
                        self.service.push_output(["── 重载完成 ──".into()]);
                        self.service.push_output(out.lines().map(String::from));
                        self.notification = Some(Notification::success("Nginx 已重载".to_string()));
                    }
                    Err(e) => {
                        self.service.push_output(["── 重载失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("重载失败".to_string()));
                    }
                }
            }
            AppEvent::ServiceRestartResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(()) => {
                        self.service.push_output(["── 重启完成 ──".into()]);
                        self.notification = Some(Notification::success("Nginx 已重启".to_string()));
                    }
                    Err(e) => {
                        self.service.push_output(["── 重启失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("重启失败".to_string()));
                    }
                }
            }
            AppEvent::ServiceStatusResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(out) => {
                        self.service.push_output(["── systemctl status ──".into()]);
                        self.service.push_output(out.lines().map(String::from));
                    }
                    Err(e) => {
                        self.service.push_output(["── 状态查询失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                    }
                }
            }
            AppEvent::ServiceUpdateCheckResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(info) => {
                        self.service.push_output(["── 版本检查 ──".into()]);
                        self.service.push_output([
                            format!("当前版本：{}", info.current_version),
                            format!("最新版本：{}", info.latest_version),
                            format!("发布页面：{}", info.release_url),
                        ]);
                        if let Some(published_at) = info.published_at.clone() {
                            self.service
                                .push_output([format!("发布时间：{}", published_at)]);
                        }
                        self.service.update_info = Some(info.clone());
                        if info.has_update {
                            self.modal = Some(crate::ui::modal::Modal::confirm_upgrade_tui(
                                &info.current_version,
                                &info.latest_version,
                            ));
                        } else {
                            self.notification =
                                Some(Notification::success("当前已是最新版本".to_string()));
                        }
                    }
                    Err(e) => {
                        self.service.push_output(["── 版本检查失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("检查更新失败".to_string()));
                    }
                }
            }
            AppEvent::ServiceUpgradeResult(b) => {
                self.service.running = None;
                match *b {
                    Ok(outcome) => {
                        self.service.update_info = Some(outcome.info.clone());
                        self.service.push_output(["── TUI 更新 ──".into()]);
                        if outcome.updated {
                            self.service.push_output([
                                format!(
                                    "已更新：{} -> {}",
                                    outcome.info.current_version, outcome.info.latest_version
                                ),
                                format!("二进制：{}", outcome.binary_path.display()),
                                "请退出并重新启动 ngtool 以运行新版本。".to_string(),
                            ]);
                            self.notification = Some(Notification::success(
                                "TUI 已更新，重启 ngtool 后生效".to_string(),
                            ));
                        } else {
                            self.service.push_output([
                                format!("当前版本：{}", outcome.info.current_version),
                                format!("最新版本：{}", outcome.info.latest_version),
                                "无需更新。".to_string(),
                            ]);
                            self.notification =
                                Some(Notification::success("当前已是最新版本".to_string()));
                        }
                    }
                    Err(e) => {
                        self.service.push_output(["── TUI 更新失败 ──".into()]);
                        self.service
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("TUI 更新失败".to_string()));
                    }
                }
            }
            AppEvent::SiteCreateResult { site_name, result } => {
                self.site_form.submitting = false;
                match *result {
                    Ok(crate::domain::site::CreateSiteOutcome::Ok { cert_requested }) => {
                        let msg = if cert_requested {
                            format!("站点 {} 已创建并已申请证书", site_name)
                        } else {
                            format!("站点 {} 已创建", site_name)
                        };
                        self.notification = Some(Notification::success(msg));
                        self.site_form = SiteFormState::default(); // 清空表单
                        self.route = Route::Sites(SitesRoute::List);
                        self.sites.pending_refresh = true;
                    }
                    Ok(crate::domain::site::CreateSiteOutcome::CertFailed { error }) => {
                        self.notification = Some(Notification::info(format!(
                            "站点 {} 已创建，但证书申请失败：{}",
                            site_name, error
                        )));
                        self.site_form = SiteFormState::default();
                        self.route = Route::Sites(SitesRoute::List);
                        self.sites.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("创建失败：{}", e)));
                    }
                }
            }
            AppEvent::LogTailLine { line } => {
                self.logs.push_line(line);
                // 如果有搜索，重新计算匹配
                let query = self.logs.search_query.clone();
                if let Some(q) = query {
                    self.logs.search(&q);
                }
            }
            AppEvent::SiteEditResult { site_name, result } => {
                self.site_edit.saving = false;
                match *result {
                    Ok(()) => {
                        self.notification =
                            Some(Notification::success(format!("站点 {} 已保存", site_name)));
                        self.site_edit.dirty = false;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("保存失败：{}", e)));
                    }
                }
            }
            AppEvent::CertsSnapshot(snap) => {
                self.certs.refreshing = false;
                self.certs.last_refresh = Some(Instant::now());
                let snap = *snap;
                self.certs.list = snap.items;
                self.certs.raw_output = snap.raw_output;
                self.certs.auto_renew = Some(snap.auto_renew);
                self.certs.last_error = snap.error;
                if !self.certs.list.is_empty() && self.certs.selected >= self.certs.list.len() {
                    self.certs.selected = self.certs.list.len() - 1;
                }
            }
            AppEvent::CertRequestResult { site_name, result } => {
                self.certs.running = None;
                match *result {
                    Ok(out) => {
                        self.certs.push_output(["── 证书申请输出 ──".into()]);
                        self.certs.push_output(out.lines().map(String::from));
                        self.notification = Some(Notification::success(format!(
                            "已为站点 {} 申请证书",
                            site_name
                        )));
                        self.certs.pending_refresh = true;
                    }
                    Err(e) => {
                        self.certs.push_output(["── 证书申请失败 ──".into()]);
                        self.certs
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure(format!("申请失败：{}", e)));
                    }
                }
            }
            AppEvent::CertRenewAllResult(b) => {
                self.certs.running = None;
                match *b {
                    Ok(out) => {
                        self.certs.push_output(["── certbot renew ──".into()]);
                        self.certs.push_output(out.lines().map(String::from));
                        self.notification =
                            Some(Notification::success("续期流程已完成".to_string()));
                        self.certs.pending_refresh = true;
                    }
                    Err(e) => {
                        self.certs.push_output(["── 续期失败 ──".into()]);
                        self.certs
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification = Some(Notification::failure("续期失败".to_string()));
                    }
                }
            }
            AppEvent::CertAutoRenewResult(b) => {
                self.certs.running = None;
                let status = *b;
                self.certs.push_output(["── 自动续签状态 ──".into()]);
                self.certs.push_output([
                    format!(
                        "{} 定时器：{}",
                        if status.timer_active { "✓" } else { "⚠" },
                        status.timer_unit
                    ),
                    format!(
                        "{} deploy hook：{}",
                        if status.deploy_hook_present {
                            "✓"
                        } else {
                            "⚠"
                        },
                        status.deploy_hook_path
                    ),
                ]);
                if let Some(next) = status.next_run.clone() {
                    self.certs.push_output([format!("  下次执行：{}", next)]);
                }
                if let Some(err) = status.last_check_error.clone() {
                    self.certs.push_output([format!("  探测错误：{}", err)]);
                }
                for tip in status.advice() {
                    self.certs.push_output([format!("• {}", tip)]);
                }
                self.certs.auto_renew = Some(status);
            }
            AppEvent::CertInstallHookResult(b) => {
                self.certs.running = None;
                match *b {
                    Ok(()) => {
                        self.certs.push_output([
                            "── 安装 deploy hook ──".into(),
                            "已创建 /etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh".into(),
                        ]);
                        self.notification =
                            Some(Notification::success("deploy hook 安装成功".to_string()));
                        // 立即刷新自动续签状态以更新 UI
                        self.certs.pending_refresh = true;
                    }
                    Err(e) => {
                        self.certs.push_output([
                            "── 安装 deploy hook 失败 ──".into(),
                        ]);
                        self.certs
                            .push_output(e.to_string().lines().map(String::from));
                        self.notification =
                            Some(Notification::failure("deploy hook 安装失败".to_string()));
                    }
                }
            }
            AppEvent::BackupListLoaded(b) => {
                self.backup.refreshing = false;
                self.backup.last_refresh = Some(Instant::now());
                match *b {
                    Ok(list) => {
                        self.backup.last_error = None;
                        if !list.is_empty() && self.backup.selected >= list.len() {
                            self.backup.selected = list.len() - 1;
                        }
                        self.backup.list = list;
                    }
                    Err(msg) => {
                        self.backup.last_error = Some(msg);
                    }
                }
            }
            AppEvent::BackupCreateResult(b) => {
                self.backup.running = false;
                match *b {
                    Ok(p) => {
                        let name = p
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("(unnamed)")
                            .to_string();
                        self.notification =
                            Some(Notification::success(format!("已创建备份 {}", name)));
                        self.backup.push_output([format!("✓ 已创建：{}", name)]);
                        self.backup.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("备份失败：{}", e)));
                        self.backup.push_output([format!("✗ 备份失败：{}", e)]);
                    }
                }
            }
            AppEvent::BackupDeleteResult(b) => {
                self.backup.running = false;
                match *b {
                    Ok(()) => {
                        self.notification = Some(Notification::success("已删除备份".to_string()));
                        self.backup.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("删除失败：{}", e)));
                    }
                }
            }
            AppEvent::BackupRestoreResult(b) => {
                self.backup.running = false;
                match *b {
                    Ok(crate::domain::backup::RestoreOutcome::Ok { pre_restore }) => {
                        self.notification = Some(Notification::success("还原完成".to_string()));
                        self.backup.push_output([
                            "✓ 还原成功".into(),
                            format!("  pre-restore 备份：{}", pre_restore.display()),
                        ]);
                        self.backup.pending_refresh = true;
                    }
                    Ok(crate::domain::backup::RestoreOutcome::TestFailedRolledBack {
                        error,
                        pre_restore,
                    }) => {
                        self.notification = Some(Notification::failure(
                            "还原后 nginx -t 失败，已回滚到 pre-restore".to_string(),
                        ));
                        self.backup.push_output([
                            "⚠ 还原后 nginx -t 失败，已自动回滚".into(),
                            format!("  错误：{}", error),
                            format!("  pre-restore 备份保留：{}", pre_restore.display()),
                        ]);
                        self.backup.pending_refresh = true;
                    }
                    Ok(crate::domain::backup::RestoreOutcome::TestFailedRollbackFailed {
                        error,
                        rollback_error,
                        pre_restore,
                    }) => {
                        self.notification = Some(Notification::failure(
                            "还原失败且回滚失败，需手工干预".to_string(),
                        ));
                        self.backup.push_output([
                            "✗ 还原失败且回滚也失败，请人工干预".into(),
                            format!("  原始错误：{}", error),
                            format!("  回滚错误：{}", rollback_error),
                            format!("  pre-restore 备份：{}", pre_restore.display()),
                        ]);
                        self.backup.pending_refresh = true;
                    }
                    Err(e) => {
                        self.notification = Some(Notification::failure(format!("还原失败：{}", e)));
                        self.backup.push_output([format!("✗ 还原失败：{}", e)]);
                    }
                }
            }
        }
    }

    fn maybe_auto_refresh_dashboard(&mut self) {
        if !matches!(self.route, Route::Dashboard) {
            return;
        }
        if self.dashboard.refreshing || self.dashboard.pending_refresh {
            return;
        }
        let due = match self.dashboard.last_refresh {
            None => true,
            Some(t) => t.elapsed() >= DASHBOARD_AUTO_REFRESH,
        };
        if due {
            self.dashboard.pending_refresh = true;
        }
    }

    fn maybe_auto_refresh_sites(&mut self) {
        if !matches!(self.route, Route::Sites(_)) {
            return;
        }
        if self.sites.refreshing || self.sites.pending_refresh {
            return;
        }
        let due = match self.sites.last_refresh {
            None => true,
            Some(t) => t.elapsed() >= SITES_AUTO_REFRESH,
        };
        if due {
            self.sites.pending_refresh = true;
        }
    }

    fn maybe_auto_refresh_certs(&mut self) {
        if !matches!(self.route, Route::Certs) {
            return;
        }
        if self.certs.refreshing || self.certs.pending_refresh {
            return;
        }
        let due = match self.certs.last_refresh {
            None => true,
            Some(t) => t.elapsed() >= CERTS_AUTO_REFRESH,
        };
        if due {
            self.certs.pending_refresh = true;
        }
    }

    /// 主循环消费：取出仪表盘待刷新意图。
    pub fn take_dashboard_refresh_request(&mut self) -> bool {
        if self.dashboard.pending_refresh && !self.dashboard.refreshing {
            self.dashboard.pending_refresh = false;
            self.dashboard.refreshing = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出站点待刷新意图。
    pub fn take_sites_refresh_request(&mut self) -> bool {
        if self.sites.pending_refresh && !self.sites.refreshing {
            self.sites.pending_refresh = false;
            self.sites.refreshing = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出站点启停意图。返回 (站点名, 目标启用状态)。
    pub fn take_site_toggle_request(&mut self) -> Option<(String, bool)> {
        self.sites.pending_toggle.take()
    }

    /// 主循环消费：取出待删除站点请求
    pub fn take_site_delete_request(&mut self) -> Option<String> {
        self.sites.pending_delete.take()
    }

    /// 当前焦点是否落在"文本输入字段"上。用于在 handle_key 顶部判断
    /// 是否应当让全局快捷键（q 退出 / 1-6 跳菜单）让位给字面输入。
    /// 详见 doc/design.md §五，全局快捷键不应吞掉用户在输入框里键入的字符。
    pub fn is_in_text_input(&self) -> bool {
        match &self.route {
            Route::Sites(SitesRoute::New) => matches!(
                self.site_form.focused,
                FormField::SiteName | FormField::Domain | FormField::Target
            ),
            Route::Sites(SitesRoute::EditForm { .. }) => matches!(
                self.site_edit.focused,
                EditFocus::Domain | EditFocus::Target | EditFocus::SlotContent
            ),
            // 原始模式与槽位全屏编辑：整页都在接收文本
            Route::Sites(SitesRoute::EditRaw { .. })
            | Route::Sites(SitesRoute::EditSlotFull { .. }) => true,
            // 日志页只在搜索框激活时算作文本输入
            Route::Logs => matches!(self.logs.focused, LogsFocus::SearchInput),
            _ => false,
        }
    }

    fn handle_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // 弹窗优先：当前有 modal 时所有按键先交给 modal
        if self.modal.is_some() {
            return self.handle_modal_key(k);
        }

        // Ctrl+C：弹出退出确认弹窗
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c')) {
            self.modal = Some(Modal::confirm_quit());
            return;
        }

        let in_text_input = self.is_in_text_input();

        // q：弹出退出确认弹窗（文本输入中仍作为字面字符传递）
        if !in_text_input
            && k.modifiers == KeyModifiers::NONE
            && matches!(k.code, KeyCode::Char('q'))
        {
            self.modal = Some(Modal::confirm_quit());
            return;
        }

        // 数字键 1-6：直接跳转到对应一级菜单。同样在文本输入中让位给字面输入。
        if !in_text_input && k.modifiers == KeyModifiers::NONE {
            if let KeyCode::Char(c @ '1'..='6') = k.code {
                let idx = (c as u8 - b'1') as usize;
                self.go_to_menu(MenuItem::ALL[idx]);
                return;
            }
        }

        // 焦点在侧边栏时，导航键（Up/Down/Enter/Right/Tab）应优先交给侧边栏处理。
        // 否则下方的路由相关 handler（Sites/List 的 Up/Down 移动站点光标、Service 的
        // Up/Down 切换按钮、Certs/Backup 的 Up/Down 等）会抢先吞掉方向键，导致用户
        // 在侧边栏按 ↑/↓ 卡住、无法继续切换菜单项。
        if matches!(self.focus, FocusArea::Sidebar)
            && !in_text_input
            && k.modifiers == KeyModifiers::NONE
            && matches!(
                k.code,
                KeyCode::Up | KeyCode::Down | KeyCode::Enter | KeyCode::Right | KeyCode::Tab
            )
        {
            return self.handle_sidebar_key(k);
        }

        // 路由相关全局快捷键（即使焦点在侧边栏也生效）
        if matches!(self.route, Route::Dashboard)
            && k.modifiers == KeyModifiers::NONE
            && matches!(k.code, KeyCode::Char('r'))
        {
            self.dashboard.pending_refresh = true;
            return;
        }
        if matches!(self.route, Route::Sites(SitesRoute::List)) && k.modifiers == KeyModifiers::NONE
        {
            match k.code {
                KeyCode::Char('r') => {
                    self.sites.pending_refresh = true;
                    return;
                }
                KeyCode::Up => {
                    self.sites.move_cursor(-1);
                    return;
                }
                KeyCode::Down => {
                    self.sites.move_cursor(1);
                    return;
                }
                KeyCode::Enter => {
                    self.request_site_toggle();
                    return;
                }
                KeyCode::Char('n') => {
                    if self.run_mode.is_readonly() {
                        self.notification = Some(Notification::failure(
                            "当前为只读模式，需要 root 权限执行此操作".to_string(),
                        ));
                    } else {
                        self.site_form = SiteFormState::default();
                        self.route = Route::Sites(SitesRoute::New);
                    }
                    return;
                }
                KeyCode::Char('e') => {
                    self.enter_site_edit();
                    return;
                }
                KeyCode::Char('d') => {
                    self.request_site_delete();
                    return;
                }
                KeyCode::Char('c') => {
                    self.request_cert_for_current_site();
                    return;
                }
                KeyCode::Char('l') => {
                    self.goto_site_log();
                    return;
                }
                _ => {}
            }
        }

        if matches!(self.route, Route::Service) && k.modifiers == KeyModifiers::NONE {
            match k.code {
                KeyCode::Tab | KeyCode::Right | KeyCode::Down => {
                    self.service.move_focus(1);
                    return;
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Up => {
                    self.service.move_focus(-1);
                    return;
                }
                KeyCode::Enter => {
                    self.request_service_action();
                    return;
                }
                KeyCode::Char('c') => {
                    self.service.clear_output();
                    return;
                }
                _ => {}
            }
        }

        if matches!(self.route, Route::Sites(SitesRoute::New)) {
            return self.handle_site_form_key(k);
        }

        // 站点编辑模式按键处理
        if matches!(self.route, Route::Sites(SitesRoute::EditForm { .. })) {
            return self.handle_site_edit_key(k);
        }

        // 原始配置编辑模式按键处理
        if matches!(self.route, Route::Sites(SitesRoute::EditRaw { .. })) {
            return self.handle_raw_edit_key(k);
        }

        // 注入槽全屏编辑模式按键处理（Ctrl+E 进入）
        if matches!(self.route, Route::Sites(SitesRoute::EditSlotFull { .. })) {
            return self.handle_slot_full_edit_key(k);
        }

        // 日志视图按键处理
        if matches!(self.route, Route::Logs) {
            return self.handle_logs_key(k);
        }

        // 证书视图按键处理
        if matches!(self.route, Route::Certs) {
            return self.handle_certs_key(k);
        }

        // 备份视图按键处理
        if matches!(self.route, Route::Backup) {
            return self.handle_backup_key(k);
        }

        match self.focus {
            FocusArea::Sidebar => self.handle_sidebar_key(k),
            FocusArea::Content => self.handle_content_key(k),
        }
    }

    fn handle_sidebar_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match k.code {
            KeyCode::Up => self.move_menu(-1),
            KeyCode::Down => self.move_menu(1),
            KeyCode::Enter | KeyCode::Right | KeyCode::Tab => {
                self.focus = FocusArea::Content;
            }
            _ => {}
        }
    }

    fn handle_content_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        let in_overlay = !matches!(
            self.route,
            Route::Dashboard
                | Route::Certs
                | Route::Logs
                | Route::Service
                | Route::Backup
                | Route::Sites(SitesRoute::List)
        );

        match k.code {
            KeyCode::Esc => {
                if in_overlay {
                    if matches!(self.route, Route::Sites(_)) {
                        self.route = Route::Sites(SitesRoute::List);
                    }
                } else {
                    self.focus = FocusArea::Sidebar;
                }
            }
            KeyCode::Tab => self.focus = FocusArea::Sidebar,
            KeyCode::Left if !in_overlay => self.focus = FocusArea::Sidebar,
            _ => {}
        }
    }

    fn handle_modal_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        let Some(modal) = self.modal.as_mut() else {
            return;
        };
        match k.code {
            KeyCode::Esc => self.modal = None,
            KeyCode::Tab | KeyCode::Left | KeyCode::Right => modal.toggle_focus(),
            KeyCode::Enter => {
                let action = modal.confirm_action();
                self.modal = None;
                self.execute_modal_action(action);
            }
            _ => {}
        }
    }

    fn execute_modal_action(&mut self, action: crate::ui::modal::ModalAction) {
        use crate::ui::modal::ModalAction;
        match action {
            ModalAction::None => {}
            ModalAction::Quit => self.should_quit = true,
            ModalAction::RestartNginx => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，无法重启服务".to_string(),
                    ));
                    return;
                }
                self.service.pending_action = Some(ServiceButton::Restart);
                self.service.running = Some(ServiceButton::Restart);
            }
            ModalAction::UpgradeTui => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，无法更新 TUI".to_string(),
                    ));
                    return;
                }
                // 使用一个特殊的 pending_action 值来标记升级
                self.service.pending_upgrade = true;
                self.service.running = Some(ServiceButton::CheckUpdate);
            }
            ModalAction::InstallDeployHook => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                self.certs.pending_install_hook = true;
                self.certs.running = Some(crate::app::state::CertsAction::InstallDeployHook);
            }
            ModalAction::DiscardSiteForm => {
                self.site_form = SiteFormState::default();
                self.route = Route::Sites(SitesRoute::List);
            }
            ModalAction::DiscardSiteEdit => {
                self.site_edit = SiteEditState::default();
                self.route = Route::Sites(SitesRoute::List);
                self.notification = Some(Notification::info("已放弃修改".to_string()));
            }
            ModalAction::RequestCertForSite { site_name, domains } => {
                self.certs.pending_request = Some((site_name, domains));
            }
            ModalAction::RenewAllCerts => {
                self.certs.pending_renew = true;
            }
            ModalAction::CreateBackup => {
                self.backup.pending_create = true;
            }
            ModalAction::DeleteBackup(p) => {
                self.backup.pending_delete = Some(p);
            }
            ModalAction::RestoreBackup(p) => {
                self.backup.pending_restore = Some(p);
            }
            ModalAction::DeleteSite { site_name } => {
                self.sites.action_in_flight = Some(site_name.clone());
                self.sites.pending_delete = Some(site_name);
            }
        }
    }

    fn go_to_menu(&mut self, item: MenuItem) {
        let prev_route = self.route.clone();
        self.route = item.default_route();
        self.focus = FocusArea::Sidebar;
        // 切回仪表盘时若数据陈旧主动触发一次刷新
        if !matches!(prev_route, Route::Dashboard) && matches!(self.route, Route::Dashboard) {
            let stale = self
                .dashboard
                .last_refresh
                .map(|t| t.elapsed() >= DASHBOARD_AUTO_REFRESH)
                .unwrap_or(true);
            if stale {
                self.dashboard.pending_refresh = true;
            }
        }
        // 切到站点列表时若从未加载也触发刷新
        if !matches!(prev_route, Route::Sites(_)) && matches!(self.route, Route::Sites(_)) {
            let stale = self
                .sites
                .last_refresh
                .map(|t| t.elapsed() >= SITES_AUTO_REFRESH)
                .unwrap_or(true);
            if stale {
                self.sites.pending_refresh = true;
            }
        }
        // 切到日志视图时启动 tail（或切换日志源）
        if !matches!(prev_route, Route::Logs) && matches!(self.route, Route::Logs) {
            self.logs.pending_tail_change = true;
        }
        // 离开日志视图时停止 tail
        if matches!(prev_route, Route::Logs) && !matches!(self.route, Route::Logs) {
            self.logs.stop_tail();
        }
        // 切到证书页时若数据陈旧主动触发刷新
        if !matches!(prev_route, Route::Certs) && matches!(self.route, Route::Certs) {
            let stale = self
                .certs
                .last_refresh
                .map(|t| t.elapsed() >= CERTS_AUTO_REFRESH)
                .unwrap_or(true);
            if stale {
                self.certs.pending_refresh = true;
            }
        }
        // 切到备份页时主动触发首次扫描
        if !matches!(prev_route, Route::Backup) && matches!(self.route, Route::Backup) {
            self.backup.pending_refresh = true;
        }
    }

    fn request_site_delete(&mut self) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        if self.sites.action_in_flight.is_some() {
            return;
        }
        let Some(site) = self.sites.current() else {
            return;
        };
        let name = site.name.clone();
        self.modal = Some(Modal::confirm(
            "⚠️  确认删除站点",
            vec![
                format!("即将删除站点 {} 的配置文件。", name),
                "该操作不可撤销。".into(),
                "确认删除？".into(),
            ],
            "确认删除",
            crate::ui::modal::ModalAction::DeleteSite { site_name: name },
        ));
    }

    /// 按 c 键：为当前选中站点申请证书，跳转到证书页
    fn request_cert_for_current_site(&mut self) {
        let Some(site) = self.sites.current() else {
            self.notification = Some(Notification::info("请先选中一个站点".to_string()));
            return;
        };
        if site.all_domains.is_empty() {
            self.notification = Some(Notification::failure(
                "该站点没有配置域名，无法申请证书".to_string(),
            ));
            return;
        }
        let site_name = site.name.clone();
        let domains = site.all_domains.clone();
        self.certs.pending_request = Some((site_name.clone(), domains.clone()));
        self.route = Route::Certs;
        self.focus = FocusArea::Content;
        self.certs.pending_refresh = true;
        self.certs.running = Some(crate::app::state::CertsAction::Request);
    }

    /// 按 l 键：跳转到日志页，自动选中当前站点的日志
    fn goto_site_log(&mut self) {
        let Some(site) = self.sites.current() else {
            self.notification = Some(Notification::info("请先选中一个站点".to_string()));
            return;
        };
        let name = site.name.clone();
        self.logs.source = crate::domain::log::LogSource::Site {
            name,
            kind: crate::domain::log::LogKind::Access,
        };
        self.logs.clear_buffer();
        self.logs.pending_tail_change = true;
        self.route = Route::Logs;
        self.focus = FocusArea::Content;
        self.logs.focused = crate::app::state::LogsFocus::LogContent;
    }

    fn request_site_toggle(&mut self) {
        // 只读：直接给出提示，不派发
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        if self.sites.action_in_flight.is_some() {
            return; // 已有操作在飞中
        }
        let Some(site) = self.sites.current() else {
            return;
        };
        let name = site.name.clone();
        let target_enabled = !site.enabled;
        self.sites.action_in_flight = Some(name.clone());
        self.sites.pending_toggle = Some((name, target_enabled));
    }

    fn request_service_action(&mut self) {
        if self.service.running.is_some() {
            return;
        }
        let btn = self.service.focused;
        let needs_root = matches!(btn, ServiceButton::Reload | ServiceButton::Restart);
        if needs_root && self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        match btn {
            ServiceButton::Restart => {
                self.modal = Some(Modal::confirm_restart_nginx());
            }
            ServiceButton::Test
            | ServiceButton::Reload
            | ServiceButton::Status
            | ServiceButton::CheckUpdate => {
                self.service.running = Some(btn);
                self.service.pending_action = Some(btn);
            }
        }
    }

    pub fn take_service_action(&mut self) -> Option<ServiceButton> {
        self.service.pending_action.take()
    }

    pub fn take_service_upgrade(&mut self) -> bool {
        if self.service.pending_upgrade {
            self.service.pending_upgrade = false;
            true
        } else {
            false
        }
    }

    fn move_menu(&mut self, delta: i32) {
        let cur = self.current_menu();
        let len = MenuItem::ALL.len() as i32;
        let cur_idx = MenuItem::ALL
            .iter()
            .position(|m| *m == cur)
            .map(|x| x as i32)
            .unwrap_or(0);
        let next_idx = (cur_idx + delta).rem_euclid(len) as usize;
        self.go_to_menu(MenuItem::ALL[next_idx]);
    }

    pub fn expire_notification_if_due(&mut self) {
        if let Some(n) = &self.notification {
            if Instant::now() >= n.expires_at {
                self.notification = None;
            }
        }
    }

    /// 处理新建站点表单的按键
    fn handle_site_form_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.site_form.submitting {
            return; // 提交中不接受输入
        }

        // Ctrl+Enter: 快速提交
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Enter) {
            self.submit_site_form();
            return;
        }

        // Esc: 返回列表（有内容时弹出确认框，详见 design.md 子模式 B）
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            if self.site_form.site_name.is_empty()
                && self.site_form.domain.is_empty()
                && self.site_form.target.is_empty()
            {
                self.route = Route::Sites(SitesRoute::List);
            } else {
                self.modal = Some(Modal::confirm_discard_site_form());
            }
            return;
        }

        // Tab/Shift+Tab: 字段切换
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Tab) {
            self.site_form.move_focus(1);
            return;
        }
        if k.modifiers.contains(KeyModifiers::SHIFT) && matches!(k.code, KeyCode::Tab) {
            self.site_form.move_focus(-1);
            return;
        }

        // 上下键：字段间导航（SiteType 焦点留给下方类型切换逻辑）
        if k.modifiers == KeyModifiers::NONE && self.site_form.focused != FormField::SiteType {
            match k.code {
                KeyCode::Up => {
                    self.site_form.move_focus(-1);
                    return;
                }
                KeyCode::Down => {
                    self.site_form.move_focus(1);
                    return;
                }
                _ => {}
            }
        }

        // 上下键: 类型选择器切换
        if k.modifiers == KeyModifiers::NONE && self.site_form.focused == FormField::SiteType {
            match k.code {
                KeyCode::Up => self.site_form.toggle_site_type(-1),
                KeyCode::Down => self.site_form.toggle_site_type(1),
                KeyCode::Enter => self.site_form.move_focus(1),
                _ => {}
            }
            return;
        }

        // Enter: 提交（在提交按钮上） / 下一个字段（在其他字段上）
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Enter) {
            match self.site_form.focused {
                FormField::SubmitButton => self.submit_site_form(),
                FormField::SiteType => self.site_form.move_focus(1),
                FormField::EnableCheckbox => {
                    self.site_form.enable_now = !self.site_form.enable_now;
                }
                FormField::CertCheckbox => {
                    // 证书申请依赖立即启用
                    if self.site_form.enable_now {
                        self.site_form.request_cert = !self.site_form.request_cert;
                    }
                }
                _ => self.site_form.move_focus(1),
            }
            return;
        }

        // Space: 复选框切换
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char(' ')) {
            match (self.site_form.focused, self.site_form.enable_now) {
                (FormField::EnableCheckbox, _) => {
                    self.site_form.enable_now = !self.site_form.enable_now;
                }
                (FormField::CertCheckbox, true) => {
                    self.site_form.request_cert = !self.site_form.request_cert;
                }
                _ => {}
            }
            return;
        }

        // 文本输入处理
        if k.modifiers == KeyModifiers::NONE || k.modifiers.contains(KeyModifiers::SHIFT) {
            match self.site_form.focused {
                FormField::SiteName | FormField::Domain | FormField::DomainAliases | FormField::Target => {
                    match k.code {
                        KeyCode::Char(c) => {
                            let field = match self.site_form.focused {
                                FormField::SiteName => &mut self.site_form.site_name,
                                FormField::Domain => &mut self.site_form.domain,
                                FormField::DomainAliases => &mut self.site_form.domain_aliases,
                                FormField::Target => &mut self.site_form.target,
                                _ => return,
                            };
                            field.push(c);
                            // 清除该字段错误
                            let key = match self.site_form.focused {
                                FormField::SiteName => "site_name",
                                FormField::Domain => "domain",
                                FormField::DomainAliases => "domain_aliases",
                                FormField::Target => "target",
                                _ => "",
                            };
                            self.site_form.field_errors.remove(key);
                        }
                        KeyCode::Backspace => {
                            let field = match self.site_form.focused {
                                FormField::SiteName => &mut self.site_form.site_name,
                                FormField::Domain => &mut self.site_form.domain,
                                FormField::DomainAliases => &mut self.site_form.domain_aliases,
                                FormField::Target => &mut self.site_form.target,
                                _ => return,
                            };
                            field.pop();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// 提交新建站点表单
    fn submit_site_form(&mut self) {
        // 只读模式禁用
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }

        // 验证字段
        if !self.site_form.validate() {
            // 有错误时聚焦第一个错误字段
            if let Some(first_err_field) = self.site_form.field_errors.keys().next() {
                self.site_form.focused = match first_err_field.as_str() {
                    "site_name" => FormField::SiteName,
                    "domain" => FormField::Domain,
                    "target" => FormField::Target,
                    "cert_checkbox" => FormField::CertCheckbox,
                    _ => FormField::SiteName,
                };
            }
            return;
        }

        // 构建输入
        let input = self.site_form.build_input();
        if input.is_none() {
            self.notification = Some(Notification::failure("无法构建创建参数".to_string()));
            return;
        }

        self.site_form.submitting = true;
        self.site_form.pending_create = input;
    }

    /// 主循环消费：取出待创建站点请求
    pub fn take_site_create_request(&mut self) -> Option<crate::domain::site::CreateSiteInput> {
        self.site_form.pending_create.take()
    }

    /// 主循环消费：取出待保存站点请求
    pub fn take_site_save_request(&mut self) -> Option<crate::domain::site::SaveSiteInput> {
        self.site_edit.pending_save.take()
    }

    /// 进入站点编辑模式
    fn enter_site_edit(&mut self) {
        let Some(site) = self.sites.current() else {
            return;
        };
        let name = site.name.clone();
        let config_path = site.config_path.clone();

        // 记录目标文件的 mtime，用于保存时的并发保护（架构 §15.0）
        let mtime_at_load = std::fs::metadata(&config_path)
            .ok()
            .and_then(|m| m.modified().ok());

        // 读取配置文件
        let content = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(e) => {
                self.notification = Some(Notification::failure(format!("无法读取配置文件：{}", e)));
                return;
            }
        };

        // 解析配置
        let parsed = crate::template::config_parser::parse_for_edit(&content);
        self.site_edit = SiteEditState::from_parsed(&name, &parsed);
        self.site_edit.mtime_at_load = mtime_at_load;
        self.site_edit.seal_original();
        self.route = Route::Sites(SitesRoute::EditForm { site_name: name });
    }

    /// 处理站点编辑（表单模式）按键
    fn handle_site_edit_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.site_edit.saving {
            return;
        }

        // Esc: 返回列表（dirty 时弹确认，详见 design.md 子模式 C）
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            if self.site_edit.dirty {
                self.modal = Some(Modal::confirm_discard_site_edit());
            } else {
                self.route = Route::Sites(SitesRoute::List);
            }
            return;
        }

        // Tab/Shift+Tab: 切换焦点
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Tab) {
            self.site_edit.move_focus_forward();
            return;
        }
        if k.modifiers.contains(KeyModifiers::SHIFT) && matches!(k.code, KeyCode::Tab) {
            self.site_edit.move_focus_backward();
            return;
        }

        // 上下键：焦点切换（TemplateList 和 SlotSelector 有自己的方向键行为）
        if k.modifiers == KeyModifiers::NONE
            && !matches!(
                self.site_edit.focused,
                EditFocus::TemplateList | EditFocus::SlotSelector
            )
        {
            match k.code {
                KeyCode::Up => {
                    self.site_edit.move_focus_backward();
                    return;
                }
                KeyCode::Down => {
                    self.site_edit.move_focus_forward();
                    return;
                }
                _ => {}
            }
        }

        // o: 切换到原始配置模式
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('o')) {
            if !self.site_edit.markers_intact {
                self.notification = Some(Notification::failure(
                    "注入槽标记已修改，无法切回表单模式".to_string(),
                ));
                return;
            }
            let name = self.site_edit.site_name.clone();
            self.route = Route::Sites(SitesRoute::EditRaw { site_name: name });
            return;
        }

        // Ctrl+S: 保存并测试
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('s')) {
            self.save_site_edit(true);
            return;
        }

        // Ctrl+W: 仅保存
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('w')) {
            self.save_site_edit(false);
            return;
        }

        // Ctrl+D: 重置表单为加载时的原始值（design.md 子模式 C）
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('d')) {
            if self.site_edit.restore_original() {
                self.notification = Some(Notification::success("已重置为加载时的值".to_string()));
            } else {
                self.notification = Some(Notification::failure("无可恢复的原始值".to_string()));
            }
            return;
        }

        // Ctrl+R: 用当前选中的模板替换槽位（design.md 子模式 C）
        if k.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(k.code, KeyCode::Char('r'))
            && self.site_edit.focused == EditFocus::TemplateList
        {
            let snippets =
                crate::template::snippets::get_snippets_for_slot(self.site_edit.current_slot);
            if let Some(snippet) = snippets.get(self.site_edit.template_index) {
                self.site_edit.replace_with_snippet(snippet.content);
                self.notification = Some(Notification::success("已替换槽位".to_string()));
            }
            return;
        }

        // Ctrl+E: 进入当前注入槽的全屏编辑模式（design.md 子模式 C，state.rs 原 TODO）
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('e')) {
            // 任何焦点都允许进入全屏编辑当前槽位（也兼容 SlotContent 焦点）
            self.site_edit.enter_slot_full();
            let slot = self.site_edit.current_slot;
            let name = self.site_edit.site_name.clone();
            self.route = Route::Sites(SitesRoute::EditSlotFull {
                site_name: name,
                slot,
            });
            return;
        }

        // 左右键：切换注入槽
        if k.modifiers == KeyModifiers::NONE && self.site_edit.focused == EditFocus::SlotSelector {
            match k.code {
                KeyCode::Left => {
                    self.site_edit.cycle_slot(-1);
                    return;
                }
                KeyCode::Right => {
                    self.site_edit.cycle_slot(1);
                    return;
                }
                _ => {}
            }
        }

        // Space: 追加模板到注入槽
        if k.modifiers == KeyModifiers::NONE
            && matches!(k.code, KeyCode::Char(' '))
            && self.site_edit.focused == EditFocus::TemplateList
        {
            let snippets =
                crate::template::snippets::get_snippets_for_slot(self.site_edit.current_slot);
            if let Some(snippet) = snippets.get(self.site_edit.template_index) {
                self.site_edit.append_snippet(snippet.content);
                self.notification = Some(Notification::success("已追加模板".to_string()));
            }
            return;
        }

        // 上下键：切换模板
        if k.modifiers == KeyModifiers::NONE && self.site_edit.focused == EditFocus::TemplateList {
            match k.code {
                KeyCode::Up => {
                    let snippets = crate::template::snippets::get_snippets_for_slot(
                        self.site_edit.current_slot,
                    );
                    if !snippets.is_empty() {
                        self.site_edit.template_index =
                            self.site_edit.template_index.saturating_sub(1);
                    }
                    return;
                }
                KeyCode::Down => {
                    let snippets = crate::template::snippets::get_snippets_for_slot(
                        self.site_edit.current_slot,
                    );
                    if self.site_edit.template_index + 1 < snippets.len() {
                        self.site_edit.template_index += 1;
                    }
                    return;
                }
                _ => {}
            }
        }

        // Enter: 在模板列表上追加模板
        if k.modifiers == KeyModifiers::NONE
            && matches!(k.code, KeyCode::Enter)
            && self.site_edit.focused == EditFocus::TemplateList
        {
            let snippets =
                crate::template::snippets::get_snippets_for_slot(self.site_edit.current_slot);
            if let Some(snippet) = snippets.get(self.site_edit.template_index) {
                self.site_edit.append_snippet(snippet.content);
                self.notification = Some(Notification::success("已追加模板".to_string()));
            }
            return;
        }

        // 文本输入
        if k.modifiers == KeyModifiers::NONE || k.modifiers.contains(KeyModifiers::SHIFT) {
            match self.site_edit.focused {
                EditFocus::Domain | EditFocus::Target => match k.code {
                    KeyCode::Char(c) => {
                        let field = match self.site_edit.focused {
                            EditFocus::Domain => &mut self.site_edit.domain,
                            EditFocus::Target => &mut self.site_edit.target,
                            _ => return,
                        };
                        field.push(c);
                        self.site_edit.dirty = true;
                        self.site_edit
                            .field_errors
                            .remove(match self.site_edit.focused {
                                EditFocus::Domain => "domain",
                                EditFocus::Target => "target",
                                _ => "",
                            });
                    }
                    KeyCode::Backspace => {
                        let field = match self.site_edit.focused {
                            EditFocus::Domain => &mut self.site_edit.domain,
                            EditFocus::Target => &mut self.site_edit.target,
                            _ => return,
                        };
                        field.pop();
                        self.site_edit.dirty = true;
                    }
                    _ => {}
                },
                EditFocus::Scheme => {
                    // 协议切换：h / http / https
                    if matches!(k.code, KeyCode::Char('h') | KeyCode::Char('s')) {
                        self.site_edit.upstream_scheme = if self.site_edit.upstream_scheme == "http"
                        {
                            "https".into()
                        } else {
                            "http".into()
                        };
                        self.site_edit.dirty = true;
                    }
                }
                EditFocus::SlotContent => {
                    // 注入槽内容编辑：常驻框直接打字（design.md 子模式 C）
                    // 全屏编辑入口走 Ctrl+E，由上方分支处理
                    match k.code {
                        KeyCode::Char(c) => {
                            let slot = self.site_edit.current_slot;
                            let content = self.site_edit.injection_slots.entry(slot).or_default();
                            content.push(c);
                            self.site_edit.dirty = true;
                        }
                        KeyCode::Backspace => {
                            let slot = self.site_edit.current_slot;
                            if let Some(content) = self.site_edit.injection_slots.get_mut(&slot) {
                                content.pop();
                                self.site_edit.dirty = true;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    /// 处理原始配置编辑按键
    fn handle_raw_edit_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.site_edit.saving {
            return;
        }

        // Esc: 返回列表（dirty 时弹确认，详见 design.md 子模式 D）
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            if self.site_edit.dirty {
                self.modal = Some(Modal::confirm_discard_site_edit());
            } else {
                self.route = Route::Sites(SitesRoute::List);
            }
            return;
        }

        // o: 切换到表单模式
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('o')) {
            // 检查标记是否完整
            let content = self.site_edit.raw_lines.join("\n");
            let (_, markers_intact) =
                crate::template::config_parser::extract_injection_slots(&content);
            if !markers_intact {
                self.notification = Some(Notification::failure(
                    "注入槽标记已修改，无法切回表单模式".to_string(),
                ));
                return;
            }
            // 重新解析；保留 mtime_at_load（模式切换不重读文件）和 dirty 状态
            let parsed = crate::template::config_parser::parse_for_edit(&content);
            let name = self.site_edit.site_name.clone();
            let mtime = self.site_edit.mtime_at_load;
            let dirty = self.site_edit.dirty;
            self.site_edit = SiteEditState::from_parsed(&name, &parsed);
            self.site_edit.mtime_at_load = mtime;
            self.site_edit.dirty = dirty;
            self.route = Route::Sites(SitesRoute::EditForm { site_name: name });
            return;
        }

        // Ctrl+S: 保存并测试
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('s')) {
            self.save_raw_edit(true);
            return;
        }

        // Ctrl+W: 仅保存
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('w')) {
            self.save_raw_edit(false);
            return;
        }

        // Ctrl+Z: 撤销（design.md 子模式 D）
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('z')) {
            if !self.site_edit.raw_undo() {
                self.notification = Some(Notification::info("无可撤销的操作".to_string()));
            }
            return;
        }

        // Ctrl+Y: 重做
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('y')) {
            if !self.site_edit.raw_redo() {
                self.notification = Some(Notification::info("无可重做的操作".to_string()));
            }
            return;
        }

        // 光标移动
        if k.modifiers == KeyModifiers::NONE {
            match k.code {
                KeyCode::Up => {
                    if self.site_edit.raw_cursor_line > 0 {
                        self.site_edit.raw_cursor_line -= 1;
                        self.site_edit.raw_cursor_col = self.site_edit.raw_cursor_col.min(
                            self.site_edit
                                .raw_lines
                                .get(self.site_edit.raw_cursor_line)
                                .map_or(0, |l| l.len()),
                        );
                    }
                    return;
                }
                KeyCode::Down => {
                    if self.site_edit.raw_cursor_line + 1 < self.site_edit.raw_lines.len() {
                        self.site_edit.raw_cursor_line += 1;
                        self.site_edit.raw_cursor_col = self.site_edit.raw_cursor_col.min(
                            self.site_edit
                                .raw_lines
                                .get(self.site_edit.raw_cursor_line)
                                .map_or(0, |l| l.len()),
                        );
                    }
                    return;
                }
                KeyCode::Left => {
                    if self.site_edit.raw_cursor_col > 0 {
                        self.site_edit.raw_cursor_col -= 1;
                    } else if self.site_edit.raw_cursor_line > 0 {
                        self.site_edit.raw_cursor_line -= 1;
                        self.site_edit.raw_cursor_col = self
                            .site_edit
                            .raw_lines
                            .get(self.site_edit.raw_cursor_line)
                            .map_or(0, |l| l.len());
                    }
                    return;
                }
                KeyCode::Right => {
                    let current_len = self
                        .site_edit
                        .raw_lines
                        .get(self.site_edit.raw_cursor_line)
                        .map_or(0, |l| l.len());
                    if self.site_edit.raw_cursor_col < current_len {
                        self.site_edit.raw_cursor_col += 1;
                    } else if self.site_edit.raw_cursor_line + 1 < self.site_edit.raw_lines.len() {
                        self.site_edit.raw_cursor_line += 1;
                        self.site_edit.raw_cursor_col = 0;
                    }
                    return;
                }
                _ => {}
            }
        }

        // 文本输入
        if k.modifiers == KeyModifiers::NONE || k.modifiers.contains(KeyModifiers::SHIFT) {
            match k.code {
                KeyCode::Char(c) => {
                    self.site_edit.push_raw_undo();
                    if let Some(line) = self
                        .site_edit
                        .raw_lines
                        .get_mut(self.site_edit.raw_cursor_line)
                    {
                        line.insert(self.site_edit.raw_cursor_col, c);
                        self.site_edit.raw_cursor_col += 1;
                        self.site_edit.dirty = true;
                    }
                }
                KeyCode::Backspace => {
                    if self.site_edit.raw_cursor_col > 0 {
                        self.site_edit.push_raw_undo();
                        if let Some(line) = self
                            .site_edit
                            .raw_lines
                            .get_mut(self.site_edit.raw_cursor_line)
                        {
                            line.remove(self.site_edit.raw_cursor_col - 1);
                            self.site_edit.raw_cursor_col -= 1;
                            self.site_edit.dirty = true;
                        }
                    } else if self.site_edit.raw_cursor_line > 0 {
                        self.site_edit.push_raw_undo();
                        // 合并到上一行
                        let current = self
                            .site_edit
                            .raw_lines
                            .remove(self.site_edit.raw_cursor_line);
                        self.site_edit.raw_cursor_line -= 1;
                        let prev_len = self
                            .site_edit
                            .raw_lines
                            .get(self.site_edit.raw_cursor_line)
                            .map_or(0, |l| l.len());
                        if let Some(prev) = self
                            .site_edit
                            .raw_lines
                            .get_mut(self.site_edit.raw_cursor_line)
                        {
                            prev.push_str(&current);
                        }
                        self.site_edit.raw_cursor_col = prev_len;
                        self.site_edit.dirty = true;
                    }
                }
                KeyCode::Enter => {
                    self.site_edit.push_raw_undo();
                    let rest = if let Some(line) = self
                        .site_edit
                        .raw_lines
                        .get_mut(self.site_edit.raw_cursor_line)
                    {
                        let rest: String = line[self.site_edit.raw_cursor_col..].to_string();
                        line.truncate(self.site_edit.raw_cursor_col);
                        rest
                    } else {
                        String::new()
                    };
                    self.site_edit
                        .raw_lines
                        .insert(self.site_edit.raw_cursor_line + 1, rest);
                    self.site_edit.raw_cursor_line += 1;
                    self.site_edit.raw_cursor_col = 0;
                    self.site_edit.dirty = true;
                }
                _ => {}
            }
        }
    }

    /// 处理注入槽全屏编辑按键（design.md 子模式 C，由 Ctrl+E 进入）
    fn handle_slot_full_edit_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Ctrl+S: 完成编辑，写回 injection_slots，回到表单模式
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('s')) {
            if self.site_edit.commit_slot_full().is_some() {
                self.notification = Some(Notification::success("已完成槽位编辑".to_string()));
            }
            let name = self.site_edit.site_name.clone();
            self.route = Route::Sites(SitesRoute::EditForm { site_name: name });
            return;
        }

        // Esc: 取消，丢弃槽位编辑缓冲
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            let name = self.site_edit.site_name.clone();
            self.site_edit.slot_edit_target = None;
            self.site_edit.slot_edit_lines.clear();
            self.site_edit.slot_edit_undo.clear();
            self.site_edit.slot_edit_redo.clear();
            self.notification = Some(Notification::info("已取消槽位编辑".to_string()));
            self.route = Route::Sites(SitesRoute::EditForm { site_name: name });
            return;
        }

        // Ctrl+D: 清空整个槽位
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('d')) {
            self.site_edit.push_slot_undo();
            self.site_edit.slot_edit_lines = vec![String::new()];
            self.site_edit.slot_edit_cursor_line = 0;
            self.site_edit.slot_edit_cursor_col = 0;
            return;
        }

        // Ctrl+Z: 撤销
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('z')) {
            if !self.site_edit.slot_undo() {
                self.notification = Some(Notification::info("无可撤销的操作".to_string()));
            }
            return;
        }

        // Ctrl+Y: 重做
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('y')) {
            if !self.site_edit.slot_redo() {
                self.notification = Some(Notification::info("无可重做的操作".to_string()));
            }
            return;
        }

        // 光标移动
        if k.modifiers == KeyModifiers::NONE {
            match k.code {
                KeyCode::Up => {
                    if self.site_edit.slot_edit_cursor_line > 0 {
                        self.site_edit.slot_edit_cursor_line -= 1;
                        let len = self
                            .site_edit
                            .slot_edit_lines
                            .get(self.site_edit.slot_edit_cursor_line)
                            .map_or(0, |l| l.chars().count());
                        self.site_edit.slot_edit_cursor_col =
                            self.site_edit.slot_edit_cursor_col.min(len);
                    }
                    return;
                }
                KeyCode::Down => {
                    if self.site_edit.slot_edit_cursor_line + 1
                        < self.site_edit.slot_edit_lines.len()
                    {
                        self.site_edit.slot_edit_cursor_line += 1;
                        let len = self
                            .site_edit
                            .slot_edit_lines
                            .get(self.site_edit.slot_edit_cursor_line)
                            .map_or(0, |l| l.chars().count());
                        self.site_edit.slot_edit_cursor_col =
                            self.site_edit.slot_edit_cursor_col.min(len);
                    }
                    return;
                }
                KeyCode::Left => {
                    if self.site_edit.slot_edit_cursor_col > 0 {
                        self.site_edit.slot_edit_cursor_col -= 1;
                    } else if self.site_edit.slot_edit_cursor_line > 0 {
                        self.site_edit.slot_edit_cursor_line -= 1;
                        self.site_edit.slot_edit_cursor_col = self
                            .site_edit
                            .slot_edit_lines
                            .get(self.site_edit.slot_edit_cursor_line)
                            .map_or(0, |l| l.chars().count());
                    }
                    return;
                }
                KeyCode::Right => {
                    let len = self
                        .site_edit
                        .slot_edit_lines
                        .get(self.site_edit.slot_edit_cursor_line)
                        .map_or(0, |l| l.chars().count());
                    if self.site_edit.slot_edit_cursor_col < len {
                        self.site_edit.slot_edit_cursor_col += 1;
                    } else if self.site_edit.slot_edit_cursor_line + 1
                        < self.site_edit.slot_edit_lines.len()
                    {
                        self.site_edit.slot_edit_cursor_line += 1;
                        self.site_edit.slot_edit_cursor_col = 0;
                    }
                    return;
                }
                _ => {}
            }
        }

        // 文本输入
        if k.modifiers == KeyModifiers::NONE || k.modifiers.contains(KeyModifiers::SHIFT) {
            match k.code {
                KeyCode::Char(c) => {
                    self.site_edit.push_slot_undo();
                    if let Some(line) = self
                        .site_edit
                        .slot_edit_lines
                        .get_mut(self.site_edit.slot_edit_cursor_line)
                    {
                        // 按字符位置插入（支持 CJK，但目前 col 用 char 计数）
                        let byte_idx = char_to_byte(line, self.site_edit.slot_edit_cursor_col);
                        line.insert(byte_idx, c);
                        self.site_edit.slot_edit_cursor_col += 1;
                    }
                }
                KeyCode::Backspace => {
                    if self.site_edit.slot_edit_cursor_col > 0 {
                        self.site_edit.push_slot_undo();
                        if let Some(line) = self
                            .site_edit
                            .slot_edit_lines
                            .get_mut(self.site_edit.slot_edit_cursor_line)
                        {
                            let prev_byte =
                                char_to_byte(line, self.site_edit.slot_edit_cursor_col - 1);
                            let cur_byte = char_to_byte(line, self.site_edit.slot_edit_cursor_col);
                            line.replace_range(prev_byte..cur_byte, "");
                            self.site_edit.slot_edit_cursor_col -= 1;
                        }
                    } else if self.site_edit.slot_edit_cursor_line > 0 {
                        self.site_edit.push_slot_undo();
                        let current = self
                            .site_edit
                            .slot_edit_lines
                            .remove(self.site_edit.slot_edit_cursor_line);
                        self.site_edit.slot_edit_cursor_line -= 1;
                        let prev_len = self
                            .site_edit
                            .slot_edit_lines
                            .get(self.site_edit.slot_edit_cursor_line)
                            .map_or(0, |l| l.chars().count());
                        if let Some(prev) = self
                            .site_edit
                            .slot_edit_lines
                            .get_mut(self.site_edit.slot_edit_cursor_line)
                        {
                            prev.push_str(&current);
                        }
                        self.site_edit.slot_edit_cursor_col = prev_len;
                    }
                }
                KeyCode::Enter => {
                    self.site_edit.push_slot_undo();
                    let rest = if let Some(line) = self
                        .site_edit
                        .slot_edit_lines
                        .get_mut(self.site_edit.slot_edit_cursor_line)
                    {
                        let byte_idx = char_to_byte(line, self.site_edit.slot_edit_cursor_col);
                        let rest: String = line[byte_idx..].to_string();
                        line.truncate(byte_idx);
                        rest
                    } else {
                        String::new()
                    };
                    self.site_edit
                        .slot_edit_lines
                        .insert(self.site_edit.slot_edit_cursor_line + 1, rest);
                    self.site_edit.slot_edit_cursor_line += 1;
                    self.site_edit.slot_edit_cursor_col = 0;
                }
                _ => {}
            }
        }
    }

    /// 保存表单模式编辑
    fn save_site_edit(&mut self, test_and_reload: bool) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }

        if !self.site_edit.validate() {
            return;
        }

        let params = self.site_edit.build_render_params();
        let kind = self.site_edit.site_kind();
        let content = match crate::template::renderer::render(kind, &params) {
            Ok(c) => c,
            Err(e) => {
                self.notification = Some(Notification::failure(format!("渲染失败：{}", e)));
                return;
            }
        };

        let site_name = self.site_edit.site_name.clone();
        let expected_mtime = self.site_edit.mtime_at_load;
        self.site_edit.saving = true;
        self.site_edit.pending_save = Some(crate::domain::site::SaveSiteInput {
            name: site_name,
            content,
            test_and_reload,
            expected_mtime,
        });
        self.notification = Some(Notification::info(if test_and_reload {
            "正在保存、测试并重载 Nginx".to_string()
        } else {
            "正在保存配置".to_string()
        }));
    }

    /// 保存原始配置编辑
    fn save_raw_edit(&mut self, test_and_reload: bool) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }

        let content = self.site_edit.raw_lines.join("\n");
        let site_name = self.site_edit.site_name.clone();
        let expected_mtime = self.site_edit.mtime_at_load;
        self.site_edit.saving = true;
        self.site_edit.pending_save = Some(crate::domain::site::SaveSiteInput {
            name: site_name,
            content,
            test_and_reload,
            expected_mtime,
        });
        self.notification = Some(Notification::info(if test_and_reload {
            "正在保存、测试并重载 Nginx".to_string()
        } else {
            "正在保存配置".to_string()
        }));
    }

    /// 处理日志视图按键
    fn handle_logs_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // 搜索模式优先
        if self.logs.focused == LogsFocus::SearchInput {
            match k.code {
                KeyCode::Esc => {
                    self.logs.clear_search();
                    self.logs.focused = LogsFocus::LogContent;
                    return;
                }
                KeyCode::Enter => {
                    // 执行搜索
                    let query = self.logs.search_query.clone();
                    if let Some(q) = query {
                        self.logs.search(&q);
                    }
                    self.logs.focused = LogsFocus::LogContent;
                    return;
                }
                KeyCode::Char(c) => {
                    if let Some(ref mut q) = self.logs.search_query {
                        q.push(c);
                    } else {
                        self.logs.search_query = Some(c.to_string());
                    }
                    return;
                }
                KeyCode::Backspace => {
                    if let Some(ref mut q) = self.logs.search_query {
                        q.pop();
                        if q.is_empty() {
                            self.logs.search_query = None;
                        }
                    }
                    return;
                }
                _ => {}
            }
            return;
        }

        // 搜索输入模式：按 / 进入
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('/')) {
            self.logs.search_query = Some(String::new());
            self.logs.focused = LogsFocus::SearchInput;
            return;
        }

        // Space: 暂停/继续
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char(' ')) {
            self.logs.toggle_pause();
            return;
        }

        // c: 清屏
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('c')) {
            self.logs.clear_buffer();
            return;
        }

        // Tab: 切换焦点区域
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Tab) {
            self.logs.focused = match self.logs.focused {
                LogsFocus::SiteSelector => LogsFocus::KindSelector,
                LogsFocus::KindSelector => LogsFocus::LogContent,
                LogsFocus::LogContent => LogsFocus::SiteSelector,
                LogsFocus::SearchInput => LogsFocus::LogContent,
            };
            return;
        }

        // n/N: 搜索结果导航（有搜索结果时）
        if self.logs.search_query.is_some() && !self.logs.match_lines.is_empty() {
            if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Char('n')) {
                self.logs.next_match();
                return;
            }
            if k.modifiers.contains(KeyModifiers::SHIFT) && matches!(k.code, KeyCode::Char('N')) {
                self.logs.prev_match();
                return;
            }
        }

        // Esc: 清除搜索或返回侧边栏
        if k.modifiers == KeyModifiers::NONE && matches!(k.code, KeyCode::Esc) {
            if self.logs.search_query.is_some() {
                self.logs.clear_search();
            } else {
                self.focus = FocusArea::Sidebar;
            }
            return;
        }

        // 左右键：在站点选择器切换站点 / 类型选择器切换类型
        if k.modifiers == KeyModifiers::NONE {
            match self.logs.focused {
                LogsFocus::SiteSelector => {
                    match (k.code, !self.sites.list.is_empty()) {
                        (KeyCode::Left | KeyCode::Char('h'), true) => {
                            // 切换到上一个站点（从站点列表）
                            let cur_site = match &self.logs.source {
                                LogSource::Global(_) => None,
                                LogSource::Site { name, .. } => Some(name.clone()),
                            };
                            let cur_idx = self
                                .sites
                                .list
                                .iter()
                                .position(|s| Some(&s.name) == cur_site.as_ref());
                            let prev_idx = match cur_idx {
                                Some(i) => {
                                    if i == 0 {
                                        None
                                    } else {
                                        Some(i - 1)
                                    }
                                }
                                None => Some(self.sites.list.len() - 1),
                            };
                            let next_site = prev_idx.map(|i| self.sites.list[i].name.clone());
                            self.logs.set_site(next_site);
                            self.logs.clear_buffer();
                            self.logs.pending_tail_change = true;
                        }
                        (KeyCode::Right | KeyCode::Char('l'), true) => {
                            // 切换到下一个站点
                            let cur_site = match &self.logs.source {
                                LogSource::Global(_) => None,
                                LogSource::Site { name, .. } => Some(name.clone()),
                            };
                            let cur_idx = self
                                .sites
                                .list
                                .iter()
                                .position(|s| Some(&s.name) == cur_site.as_ref());
                            let next_idx = match cur_idx {
                                Some(i) => {
                                    if i >= self.sites.list.len() - 1 {
                                        None
                                    } else {
                                        Some(i + 1)
                                    }
                                }
                                None => Some(0),
                            };
                            let next_site = next_idx.map(|i| self.sites.list[i].name.clone());
                            self.logs.set_site(next_site);
                            self.logs.clear_buffer();
                            self.logs.pending_tail_change = true;
                        }
                        (KeyCode::Enter, _) => {
                            self.logs.focused = LogsFocus::KindSelector;
                        }
                        _ => {}
                    }
                }
                LogsFocus::KindSelector => match k.code {
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('t') => {
                        self.logs.toggle_kind();
                        self.logs.clear_buffer();
                        self.logs.pending_tail_change = true;
                    }
                    KeyCode::Enter => {
                        self.logs.focused = LogsFocus::LogContent;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// 处理证书页按键
    fn handle_certs_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        if k.modifiers != KeyModifiers::NONE && !k.modifiers.contains(KeyModifiers::SHIFT) {
            return;
        }

        match k.code {
            KeyCode::Esc => {
                self.focus = FocusArea::Sidebar;
                return;
            }
            KeyCode::Tab => {
                self.certs.focused = match self.certs.focused {
                    CertsFocus::Table => CertsFocus::SiteSelector,
                    CertsFocus::SiteSelector => CertsFocus::ActionButtons,
                    CertsFocus::ActionButtons => CertsFocus::Table,
                };
                return;
            }
            KeyCode::Char('r') => {
                self.certs.pending_refresh = true;
                return;
            }
            KeyCode::Char('c') => {
                self.certs.clear_output();
                return;
            }
            _ => {}
        }

        match self.certs.focused {
            CertsFocus::Table => match k.code {
                KeyCode::Up => self.certs.move_cursor(-1),
                KeyCode::Down => self.certs.move_cursor(1),
                _ => {}
            },
            CertsFocus::SiteSelector => match k.code {
                KeyCode::Up => self.certs_site_selector_move(-1),
                KeyCode::Down => self.certs_site_selector_move(1),
                KeyCode::Enter => {
                    self.certs.focused = CertsFocus::ActionButtons;
                    self.certs.action_focus = CertsAction::Request;
                }
                _ => {}
            },
            CertsFocus::ActionButtons => match k.code {
                KeyCode::Left | KeyCode::Up => self.certs.cycle_action(-1),
                KeyCode::Right | KeyCode::Down => self.certs.cycle_action(1),
                KeyCode::Enter => self.request_certs_action(),
                _ => {}
            },
        }
    }

    fn certs_site_selector_move(&mut self, delta: i32) {
        if self.sites.list.is_empty() {
            return;
        }
        let len = self.sites.list.len() as i32;
        let mut idx = self.certs.site_selector_index as i32 + delta;
        if idx < 0 {
            idx = len - 1;
        } else if idx >= len {
            idx = 0;
        }
        self.certs.site_selector_index = idx as usize;
    }

    fn request_certs_action(&mut self) {
        if self.certs.running.is_some() {
            return;
        }
        match self.certs.action_focus {
            CertsAction::Request => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                if !self.ctx.deps().certbot {
                    self.notification = Some(Notification::failure("certbot 未安装".to_string()));
                    return;
                }
                let Some(site) = self.sites.list.get(self.certs.site_selector_index) else {
                    self.notification = Some(Notification::failure(
                        "请先在站点选择器中选定一个有 server_name 的站点",
                    ));
                    return;
                };
                if site.all_domains.is_empty() {
                    self.notification = Some(Notification::failure(format!(
                        "站点 {} 未配置 server_name，无法申请证书",
                        site.name
                    )));
                    return;
                }
                let modal = Modal::confirm(
                    "🔐 申请证书",
                    vec![
                        format!("站点: {}", site.name),
                        format!("域名: {}", site.all_domains.join(", ")),
                        "".into(),
                        "将使用 certbot --nginx 一次性申请上述域名。".into(),
                    ],
                    "确认申请",
                    crate::ui::modal::ModalAction::RequestCertForSite {
                        site_name: site.name.clone(),
                        domains: site.all_domains.clone(),
                    },
                );
                self.modal = Some(modal);
            }
            CertsAction::RenewAll => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                if !self.ctx.deps().certbot {
                    self.notification = Some(Notification::failure("certbot 未安装".to_string()));
                    return;
                }
                let modal = Modal::confirm(
                    "🔄 续期所有证书",
                    vec![
                        "将执行 certbot renew，对全部到期证书统一续期。".into(),
                        "进度会输出到下方操作输出区。".into(),
                    ],
                    "确认续期",
                    crate::ui::modal::ModalAction::RenewAllCerts,
                );
                self.modal = Some(modal);
            }
            CertsAction::CheckAutoRenew => {
                self.certs.pending_check_renew = true;
            }
            CertsAction::InstallDeployHook => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                if self
                    .certs
                    .auto_renew
                    .as_ref()
                    .is_some_and(|s| s.deploy_hook_present)
                {
                    self.notification =
                        Some(Notification::info("deploy hook 已安装".to_string()));
                    return;
                }
                self.modal = Some(crate::ui::modal::Modal::confirm_install_deploy_hook());
            }
        }
    }

    /// 主循环消费：取出日志源变更请求（用于启动新的 tail 任务）
    pub fn take_logs_tail_change_request(&mut self) -> bool {
        if self.logs.pending_tail_change {
            self.logs.pending_tail_change = false;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出证书页刷新意图。
    pub fn take_certs_refresh_request(&mut self) -> bool {
        if self.certs.pending_refresh && !self.certs.refreshing {
            self.certs.pending_refresh = false;
            self.certs.refreshing = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出证书申请请求 (站点名, 域名列表)
    pub fn take_cert_request(&mut self) -> Option<(String, Vec<String>)> {
        if self.certs.pending_request.is_some() {
            self.certs.running = Some(CertsAction::Request);
        }
        self.certs.pending_request.take()
    }

    /// 主循环消费：取出续期所有证书请求
    pub fn take_cert_renew_all(&mut self) -> bool {
        if self.certs.pending_renew {
            self.certs.pending_renew = false;
            self.certs.running = Some(CertsAction::RenewAll);
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出自动续签状态检查请求
    pub fn take_cert_check_auto_renew(&mut self) -> bool {
        if self.certs.pending_check_renew {
            self.certs.pending_check_renew = false;
            self.certs.running = Some(CertsAction::CheckAutoRenew);
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出安装 deploy hook 请求
    pub fn take_cert_install_hook(&mut self) -> bool {
        if self.certs.pending_install_hook {
            self.certs.pending_install_hook = false;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出备份页刷新意图。
    pub fn take_backup_refresh_request(&mut self) -> bool {
        if self.backup.pending_refresh && !self.backup.refreshing {
            self.backup.pending_refresh = false;
            self.backup.refreshing = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出创建备份请求
    pub fn take_backup_create(&mut self) -> bool {
        if self.backup.pending_create {
            self.backup.pending_create = false;
            self.backup.running = true;
            true
        } else {
            false
        }
    }

    /// 主循环消费：取出删除备份请求
    pub fn take_backup_delete(&mut self) -> Option<std::path::PathBuf> {
        if self.backup.pending_delete.is_some() {
            self.backup.running = true;
        }
        self.backup.pending_delete.take()
    }

    /// 主循环消费：取出还原备份请求
    pub fn take_backup_restore(&mut self) -> Option<std::path::PathBuf> {
        if self.backup.pending_restore.is_some() {
            self.backup.running = true;
        }
        self.backup.pending_restore.take()
    }

    /// 处理备份页按键
    fn handle_backup_key(&mut self, k: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        if k.modifiers != KeyModifiers::NONE && !k.modifiers.contains(KeyModifiers::SHIFT) {
            return;
        }
        match k.code {
            KeyCode::Esc => self.focus = FocusArea::Sidebar,
            KeyCode::Up => self.backup.move_cursor(-1),
            KeyCode::Down => self.backup.move_cursor(1),
            KeyCode::Char('R') => {
                // 大写 R：刷新列表（保留 r 给还原）
                self.backup.pending_refresh = true;
            }
            KeyCode::Char('r') => {
                self.request_backup_restore();
            }
            KeyCode::Char('n') => {
                if self.run_mode.is_readonly() {
                    self.notification = Some(Notification::failure(
                        "当前为只读模式，需要 root 权限执行此操作".to_string(),
                    ));
                    return;
                }
                let modal = Modal::confirm(
                    "💾 创建备份",
                    vec![
                        "范围限定：".into(),
                        "  /etc/nginx/nginx.conf".into(),
                        "  /etc/nginx/sites-available/*.conf".into(),
                        "  /etc/nginx/sites-enabled 启用关系".into(),
                        "".into(),
                        "不会备份 conf.d/、snippets/、modules-enabled/ 等".into(),
                    ],
                    "确认创建",
                    crate::ui::modal::ModalAction::CreateBackup,
                );
                self.modal = Some(modal);
            }
            KeyCode::Char('d') => {
                self.request_backup_delete();
            }
            KeyCode::Char('c') => {
                self.backup.clear_output_buffer();
            }
            // 备份页设计未定义视图内 Tab/Left/Right 子区域循环（design.md §四 视图 6）。
            // 这里显式忽略，避免与"Esc 返回侧栏"的统一约定冲突；状态栏会持续提示 [Esc] 返回侧栏。
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {}
            _ => {}
        }
    }

    fn request_backup_delete(&mut self) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        let Some(b) = self.backup.current() else {
            return;
        };
        let path = b.path.clone();
        let name = b.name.clone();
        let modal = Modal::confirm(
            "⚠️  确认删除备份",
            vec![format!("即将删除：{}", name), "此操作不可撤销".into()],
            "确认删除",
            crate::ui::modal::ModalAction::DeleteBackup(path),
        );
        self.modal = Some(modal);
    }

    fn request_backup_restore(&mut self) {
        if self.run_mode.is_readonly() {
            self.notification = Some(Notification::failure(
                "当前为只读模式，需要 root 权限执行此操作".to_string(),
            ));
            return;
        }
        let Some(b) = self.backup.current() else {
            return;
        };
        if !b.restorable() {
            self.notification = Some(Notification::failure(
                "该备份缺少 manifest 或 schema 不兼容，仅可查看不可还原".to_string(),
            ));
            return;
        }
        let manifest = b.manifest.clone().unwrap();
        let path = b.path.clone();
        let source_label = b.source_label().to_string();
        let impact = match crate::domain::backup::impact_for_restore(&self.ctx, &manifest) {
            Ok(i) => i,
            Err(e) => {
                self.notification = Some(Notification::failure(format!("无法计算影响摘要：{}", e)));
                return;
            }
        };

        let mut body: Vec<String> = Vec::new();
        body.push(format!("时间：{}", manifest.created_at));
        body.push(format!("来源：{}", source_label));
        body.push(String::new());
        body.push("将覆盖：".into());
        for f in &impact.will_overwrite {
            body.push(format!("  · {}", f));
        }
        if !impact.will_enable.is_empty() {
            body.push(format!("将启用：{}", impact.will_enable.join(", ")));
        }
        if !impact.will_disable.is_empty() {
            body.push(format!("将停用：{}", impact.will_disable.join(", ")));
        }
        if !impact.missing_in_backup.is_empty() {
            body.push(format!(
                "⚠ 备份中标记启用但 conf 缺失：{}",
                impact.missing_in_backup.join(", ")
            ));
        }
        body.push(String::new());
        body.push("将自动创建 pre-restore 备份。".into());
        body.push("不在范围内的文件不会被修改。".into());

        let modal = Modal::confirm(
            "⚠️  确认还原备份",
            body,
            "确认还原",
            crate::ui::modal::ModalAction::RestoreBackup(path),
        );
        self.modal = Some(modal);
    }
}

/// 把字符索引转换为字节索引；支持 CJK / emoji 多字节字符。
/// 若 char_idx 超过字符数则返回字符串末尾的字节长度。
fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    // 测试中使用 Default::default() 然后逐字段赋值更直观，关闭对应 lint。
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use crate::template::config_parser::InjectionSlot;

    #[test]
    fn char_to_byte_ascii() {
        assert_eq!(char_to_byte("hello", 0), 0);
        assert_eq!(char_to_byte("hello", 3), 3);
        assert_eq!(char_to_byte("hello", 5), 5);
        assert_eq!(char_to_byte("hello", 999), 5);
    }

    #[test]
    fn char_to_byte_cjk() {
        // 中文每字 3 字节
        let s = "应用代理";
        assert_eq!(char_to_byte(s, 0), 0);
        assert_eq!(char_to_byte(s, 1), 3);
        assert_eq!(char_to_byte(s, 2), 6);
        assert_eq!(char_to_byte(s, 4), 12);
        // 越界回到字节末尾
        assert_eq!(char_to_byte(s, 99), 12);
    }

    #[test]
    fn replace_with_snippet_overrides_slot() {
        let mut s = SiteEditState::default();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "old".into());
        s.current_slot = InjectionSlot::BeforeLocation;
        s.replace_with_snippet("new value");
        assert_eq!(
            s.injection_slots.get(&InjectionSlot::BeforeLocation),
            Some(&"new value".to_string())
        );
        assert!(s.dirty);
    }

    #[test]
    fn append_snippet_concatenates_with_newline() {
        let mut s = SiteEditState::default();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "first;".into());
        s.current_slot = InjectionSlot::BeforeLocation;
        s.append_snippet("second;");
        assert_eq!(
            s.injection_slots.get(&InjectionSlot::BeforeLocation),
            Some(&"first;\nsecond;".to_string())
        );
    }

    #[test]
    fn seal_and_restore_original_recovers_form_fields() {
        let mut s = SiteEditState::default();
        s.domain = "app.example.com".into();
        s.target = "127.0.0.1:8080".into();
        s.upstream_scheme = "http".into();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "add_header X-K v;".into());
        s.seal_original();

        // 用户修改字段
        s.domain = "messy.example.com".into();
        s.target = "0.0.0.0:80".into();
        s.upstream_scheme = "https".into();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "modified".into());
        s.dirty = true;

        // Ctrl+D 恢复
        assert!(s.restore_original());
        assert_eq!(s.domain, "app.example.com");
        assert_eq!(s.target, "127.0.0.1:8080");
        assert_eq!(s.upstream_scheme, "http");
        assert_eq!(
            s.injection_slots.get(&InjectionSlot::BeforeLocation),
            Some(&"add_header X-K v;".to_string())
        );
        assert!(!s.dirty);
    }

    #[test]
    fn restore_original_returns_false_when_not_sealed() {
        let mut s = SiteEditState::default();
        assert!(!s.restore_original());
    }

    #[test]
    fn raw_undo_redo_round_trip() {
        let mut s = SiteEditState::default();
        s.raw_lines = vec!["abc".into()];
        s.raw_cursor_line = 0;
        s.raw_cursor_col = 3;

        // 模拟一次写操作：先 push undo，再修改
        s.push_raw_undo();
        s.raw_lines[0].push('d');
        s.raw_cursor_col = 4;

        // undo 回到 "abc"
        assert!(s.raw_undo());
        assert_eq!(s.raw_lines, vec!["abc".to_string()]);
        assert_eq!(s.raw_cursor_col, 3);

        // redo 回到 "abcd"
        assert!(s.raw_redo());
        assert_eq!(s.raw_lines, vec!["abcd".to_string()]);
        assert_eq!(s.raw_cursor_col, 4);

        // 下次写操作清空 redo
        s.push_raw_undo();
        s.raw_lines[0].push('e');
        assert!(s.raw_redo_stack.is_empty());
    }

    #[test]
    fn raw_undo_empty_stack_returns_false() {
        let mut s = SiteEditState::default();
        assert!(!s.raw_undo());
        assert!(!s.raw_redo());
    }

    #[test]
    fn enter_slot_full_seeds_lines_from_injection_map() {
        let mut s = SiteEditState::default();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "line1;\nline2;".into());
        s.current_slot = InjectionSlot::BeforeLocation;
        s.enter_slot_full();

        assert_eq!(s.slot_edit_target, Some(InjectionSlot::BeforeLocation));
        assert_eq!(
            s.slot_edit_lines,
            vec!["line1;".to_string(), "line2;".to_string()]
        );
        assert_eq!(s.slot_edit_cursor_line, 0);
        assert_eq!(s.slot_edit_cursor_col, 0);
    }

    #[test]
    fn enter_slot_full_with_empty_slot_seeds_one_blank_line() {
        let mut s = SiteEditState::default();
        s.current_slot = InjectionSlot::InsideLocation;
        s.enter_slot_full();
        assert_eq!(s.slot_edit_lines, vec![String::new()]);
    }

    #[test]
    fn commit_slot_full_writes_back_and_marks_dirty() {
        let mut s = SiteEditState::default();
        s.current_slot = InjectionSlot::AfterLocation;
        s.enter_slot_full();
        s.slot_edit_lines = vec!["location /api { return 200; }".into()];

        let committed = s.commit_slot_full();
        assert_eq!(committed, Some(InjectionSlot::AfterLocation));
        assert_eq!(
            s.injection_slots.get(&InjectionSlot::AfterLocation),
            Some(&"location /api { return 200; }".to_string())
        );
        assert!(s.dirty);
    }

    #[test]
    fn commit_slot_full_empty_removes_slot() {
        let mut s = SiteEditState::default();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "old".into());
        s.current_slot = InjectionSlot::BeforeLocation;
        s.enter_slot_full();
        s.slot_edit_lines = vec![String::new()];
        s.commit_slot_full();
        assert!(!s
            .injection_slots
            .contains_key(&InjectionSlot::BeforeLocation));
    }

    #[test]
    fn slot_undo_redo_round_trip() {
        let mut s = SiteEditState::default();
        s.slot_edit_lines = vec!["abc".into()];
        s.push_slot_undo();
        s.slot_edit_lines[0].push('d');

        assert!(s.slot_undo());
        assert_eq!(s.slot_edit_lines, vec!["abc".to_string()]);
        assert!(s.slot_redo());
        assert_eq!(s.slot_edit_lines, vec!["abcd".to_string()]);
    }
}
