//! 站点编辑（托管 / 高级 / 原始 / 槽位）状态。

use crate::app::state::site_form::split_aliases;

/// 编辑器表单模式焦点区域
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditFocus {
    #[default]
    Domain,
    DomainAliases,
    Target,
    Scheme,
    ProxyFeatureStreaming,
    ProxyFeatureWebsocket,
    ProxyFeatureLargeBody,
    ProxyFeatureCors,
    ProxyFeatureLongTimeout,
    StaticMode,
    StaticFeatureCache,
    StaticFeatureBlockSensitive,
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
    /// 附加域名（逗号或空格分隔）
    pub domain_aliases: String,
    /// 代理目标（显示格式，如 "127.0.0.1:8080"）
    pub target: String,
    /// 上游协议 http/https
    pub upstream_scheme: String,
    /// 站点类型
    pub site_type: crate::domain::site::SiteType,
    /// 静态根目录
    pub static_root: String,
    /// 反向代理：流式响应 / AI API
    pub feature_streaming: bool,
    /// 反向代理：WebSocket
    pub feature_websocket: bool,
    /// 反向代理：大请求体 / 上传
    pub feature_large_body: bool,
    /// 反向代理：CORS
    pub feature_cors: bool,
    /// 反向代理：长超时
    pub feature_long_timeout: bool,
    /// 静态站点：SPA 模式
    pub feature_spa_mode: bool,
    /// 静态站点：静态缓存
    pub feature_static_cache: bool,
    /// 静态站点：敏感路径保护
    pub feature_block_sensitive: bool,
    /// 是否保留 SSL 模板配置
    pub ssl_enabled: bool,
    /// SSL 证书路径
    pub ssl_cert_path: String,
    /// SSL 私钥路径
    pub ssl_key_path: String,
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
    /// 保存成功后是否退出
    pub exit_after_save: bool,
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
    pub domain_aliases: String,
    pub target: String,
    pub upstream_scheme: String,
    pub static_root: String,
    pub feature_streaming: bool,
    pub feature_websocket: bool,
    pub feature_large_body: bool,
    pub feature_cors: bool,
    pub feature_long_timeout: bool,
    pub feature_spa_mode: bool,
    pub feature_static_cache: bool,
    pub feature_block_sensitive: bool,
    pub injection_slots:
        std::collections::HashMap<crate::template::config_parser::InjectionSlot, String>,
}

impl Default for SiteEditState {
    fn default() -> Self {
        Self {
            site_name: String::new(),
            focused: EditFocus::default(),
            domain: String::new(),
            domain_aliases: String::new(),
            target: String::new(),
            upstream_scheme: "http".into(),
            site_type: crate::domain::site::SiteType::Unknown,
            static_root: String::new(),
            feature_streaming: false,
            feature_websocket: false,
            feature_large_body: false,
            feature_cors: false,
            feature_long_timeout: false,
            feature_spa_mode: false,
            feature_static_cache: true,
            feature_block_sensitive: false,
            ssl_enabled: false,
            ssl_cert_path: String::new(),
            ssl_key_path: String::new(),
            injection_slots: std::collections::HashMap::new(),
            current_slot: crate::template::config_parser::InjectionSlot::BeforeLocation,
            markers_intact: true,
            raw_cursor_line: 0,
            raw_cursor_col: 0,
            raw_lines: Vec::new(),
            dirty: false,
            saving: false,
            exit_after_save: false,
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
        let feature_enabled = |name: &str| parsed.managed_features.iter().any(|item| item == name);
        let site_type = parsed.managed_type.unwrap_or(parsed.site_type);
        Self {
            site_name: site_name.to_string(),
            domain: parsed.domains.first().cloned().unwrap_or_default(),
            domain_aliases: parsed
                .domains
                .iter()
                .skip(1)
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            target: parsed.upstream_target.clone().unwrap_or_default(),
            upstream_scheme: parsed
                .upstream_scheme
                .clone()
                .unwrap_or_else(|| "http".into()),
            site_type,
            static_root: parsed.static_root.clone().unwrap_or_default(),
            feature_streaming: site_type == crate::domain::site::SiteType::Proxy
                && feature_enabled("streaming"),
            feature_websocket: (site_type == crate::domain::site::SiteType::Proxy
                && feature_enabled("websocket"))
                || site_type == crate::domain::site::SiteType::Emby,
            feature_large_body: (site_type == crate::domain::site::SiteType::Proxy
                && feature_enabled("large_body"))
                || site_type == crate::domain::site::SiteType::Emby,
            feature_cors: site_type == crate::domain::site::SiteType::Proxy
                && feature_enabled("cors"),
            feature_long_timeout: (site_type == crate::domain::site::SiteType::Proxy
                && feature_enabled("long_timeout"))
                || site_type == crate::domain::site::SiteType::Emby,
            feature_spa_mode: site_type == crate::domain::site::SiteType::Static
                && feature_enabled("spa_mode"),
            feature_static_cache: if site_type == crate::domain::site::SiteType::Static {
                feature_enabled("static_cache")
            } else {
                true
            },
            feature_block_sensitive: site_type == crate::domain::site::SiteType::Static
                && feature_enabled("block_sensitive"),
            ssl_enabled: parsed.ssl_cert_path.is_some() && parsed.ssl_key_path.is_some(),
            ssl_cert_path: parsed.ssl_cert_path.clone().unwrap_or_default(),
            ssl_key_path: parsed.ssl_key_path.clone().unwrap_or_default(),
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
        for alias in split_aliases(&self.domain_aliases) {
            if let Err(e) = crate::template::renderer::validate_domain(alias) {
                self.set_error("domain_aliases", format!("附加域名 '{}': {}", alias, e));
                break;
            }
        }
        if self.site_type != crate::domain::site::SiteType::Static {
            if self.target.trim().is_empty() {
                self.set_error("target", "目标地址不能为空".into());
            } else if let Err(e) = crate::template::renderer::parse_upstream(&self.target) {
                self.set_error("target", e);
            }
        }
        if self.site_type == crate::domain::site::SiteType::Static
            && self.static_root.trim().is_empty()
        {
            self.set_error("static_root", "静态目录不能为空".into());
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
                    .map(|(scheme, target)| {
                        if self.target.contains("://") {
                            (scheme, target)
                        } else {
                            (self.upstream_scheme.clone(), target)
                        }
                    })
                    .unwrap_or_else(|_| (self.upstream_scheme.clone(), self.target.clone()))
            };

        crate::template::renderer::RenderParams {
            site_name: self.site_name.clone(),
            domain_name: self.domain.trim().to_string(),
            domain_aliases: split_aliases(&self.domain_aliases).join(" "),
            upstream_scheme,
            upstream_target,
            static_root: self.static_root.clone(),
            feature_streaming: self.site_type == crate::domain::site::SiteType::Proxy
                && self.feature_streaming,
            feature_websocket: matches!(
                self.site_type,
                crate::domain::site::SiteType::Proxy | crate::domain::site::SiteType::Emby
            ) && self.feature_websocket,
            feature_large_body: matches!(
                self.site_type,
                crate::domain::site::SiteType::Proxy | crate::domain::site::SiteType::Emby
            ) && self.feature_large_body,
            feature_cors: self.site_type == crate::domain::site::SiteType::Proxy
                && self.feature_cors,
            feature_long_timeout: matches!(
                self.site_type,
                crate::domain::site::SiteType::Proxy | crate::domain::site::SiteType::Emby
            ) && self.feature_long_timeout,
            feature_spa_mode: self.site_type == crate::domain::site::SiteType::Static
                && self.feature_spa_mode,
            feature_static_cache: self.site_type == crate::domain::site::SiteType::Static
                && self.feature_static_cache,
            feature_block_sensitive: self.site_type == crate::domain::site::SiteType::Static
                && self.feature_block_sensitive,
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
            ssl_enabled: self.ssl_enabled,
            ssl_cert_path: self.ssl_cert_path.clone(),
            ssl_key_path: self.ssl_key_path.clone(),
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
        let order = self.focus_order();
        let idx = order.iter().position(|f| *f == self.focused).unwrap_or(0);
        self.focused = order[(idx + 1) % order.len()];
    }

    pub fn move_focus_backward(&mut self) {
        let order = self.focus_order();
        let idx = order.iter().position(|f| *f == self.focused).unwrap_or(0);
        self.focused = order[(idx + order.len() - 1) % order.len()];
    }

    fn focus_order(&self) -> &'static [EditFocus] {
        const PROXY_ORDER: &[EditFocus] = &[
            EditFocus::Domain,
            EditFocus::DomainAliases,
            EditFocus::Target,
            EditFocus::Scheme,
            EditFocus::ProxyFeatureStreaming,
            EditFocus::ProxyFeatureWebsocket,
            EditFocus::ProxyFeatureLargeBody,
            EditFocus::ProxyFeatureCors,
            EditFocus::ProxyFeatureLongTimeout,
        ];
        const STATIC_ORDER: &[EditFocus] = &[
            EditFocus::Domain,
            EditFocus::DomainAliases,
            EditFocus::StaticMode,
            EditFocus::StaticFeatureCache,
            EditFocus::StaticFeatureBlockSensitive,
        ];
        const EMBY_ORDER: &[EditFocus] = &[
            EditFocus::Domain,
            EditFocus::DomainAliases,
            EditFocus::Target,
            EditFocus::Scheme,
        ];

        match self.site_type {
            crate::domain::site::SiteType::Static => STATIC_ORDER,
            crate::domain::site::SiteType::Emby => EMBY_ORDER,
            _ => PROXY_ORDER,
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
            domain_aliases: self.domain_aliases.clone(),
            target: self.target.clone(),
            upstream_scheme: self.upstream_scheme.clone(),
            static_root: self.static_root.clone(),
            feature_streaming: self.feature_streaming,
            feature_websocket: self.feature_websocket,
            feature_large_body: self.feature_large_body,
            feature_cors: self.feature_cors,
            feature_long_timeout: self.feature_long_timeout,
            feature_spa_mode: self.feature_spa_mode,
            feature_static_cache: self.feature_static_cache,
            feature_block_sensitive: self.feature_block_sensitive,
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
        self.domain_aliases = snap.domain_aliases.clone();
        self.target = snap.target.clone();
        self.upstream_scheme = snap.upstream_scheme.clone();
        self.static_root = snap.static_root.clone();
        self.feature_streaming = snap.feature_streaming;
        self.feature_websocket = snap.feature_websocket;
        self.feature_large_body = snap.feature_large_body;
        self.feature_cors = snap.feature_cors;
        self.feature_long_timeout = snap.feature_long_timeout;
        self.feature_spa_mode = snap.feature_spa_mode;
        self.feature_static_cache = snap.feature_static_cache;
        self.feature_block_sensitive = snap.feature_block_sensitive;
        self.injection_slots = snap.injection_slots.clone();
        self.field_errors.clear();
        self.dirty = false;
        true
    }

    /// 保存成功后把当前编辑态与已落盘内容重新对齐，避免连续保存触发旧 mtime 校验，
    /// 同时让原始模式内容与 Ctrl+D 快照都更新到最新版本。
    pub fn mark_saved(
        &mut self,
        saved_content: &str,
        mtime_at_save: Option<std::time::SystemTime>,
    ) {
        self.raw_lines = saved_content.lines().map(String::from).collect();
        if saved_content.ends_with('\n') {
            self.raw_lines.push(String::new());
        }
        if self.raw_cursor_line >= self.raw_lines.len() {
            self.raw_cursor_line = self.raw_lines.len().saturating_sub(1);
        }
        let current_line_len = self
            .raw_lines
            .get(self.raw_cursor_line)
            .map(|line| line.chars().count())
            .unwrap_or(0);
        self.raw_cursor_col = self.raw_cursor_col.min(current_line_len);
        self.mtime_at_load = mtime_at_save;
        self.field_errors.clear();
        self.dirty = false;
        self.seal_original();
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


/// 把字符索引转换为字节索引；支持 CJK / emoji 多字节字符。
/// 若 char_idx 超过字符数则返回字符串末尾的字节长度。
pub(crate) fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}
