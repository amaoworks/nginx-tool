//! 新建站点表单状态。

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
            SiteTypeChoice::Proxy => "反向代理",
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
    ProxyFeatureStreaming,
    ProxyFeatureWebsocket,
    ProxyFeatureLargeBody,
    ProxyFeatureCors,
    ProxyFeatureLongTimeout,
    StaticMode,
    StaticFeatureCache,
    StaticFeatureBlockSensitive,
    EnableCheckbox,
    CertCheckbox,
    SubmitButton,
}

impl FormField {
    pub const ORDER: [FormField; 16] = [
        FormField::SiteName,
        FormField::Domain,
        FormField::DomainAliases,
        FormField::SiteType,
        FormField::Target,
        FormField::ProxyFeatureStreaming,
        FormField::ProxyFeatureWebsocket,
        FormField::ProxyFeatureLargeBody,
        FormField::ProxyFeatureCors,
        FormField::ProxyFeatureLongTimeout,
        FormField::StaticMode,
        FormField::StaticFeatureCache,
        FormField::StaticFeatureBlockSensitive,
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
    pub feature_streaming: bool,
    pub feature_websocket: bool,
    pub feature_large_body: bool,
    pub feature_cors: bool,
    pub feature_long_timeout: bool,
    pub static_spa_mode: bool,
    pub static_cache: bool,
    pub static_block_sensitive: bool,
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
    fn is_field_visible(&self, field: FormField) -> bool {
        match field {
            FormField::Target => self.site_type != SiteTypeChoice::Static,
            FormField::ProxyFeatureStreaming
            | FormField::ProxyFeatureWebsocket
            | FormField::ProxyFeatureLargeBody
            | FormField::ProxyFeatureCors
            | FormField::ProxyFeatureLongTimeout => self.site_type == SiteTypeChoice::Proxy,
            FormField::StaticMode
            | FormField::StaticFeatureCache
            | FormField::StaticFeatureBlockSensitive => self.site_type == SiteTypeChoice::Static,
            _ => true,
        }
    }

    pub fn move_focus(&mut self, delta: i32) {
        let len = FormField::ORDER.len() as i32;
        let cur = FormField::ORDER
            .iter()
            .position(|f| *f == self.focused)
            .map(|x| x as i32)
            .unwrap_or(0);
        let mut next_idx = cur;
        for _ in 0..len {
            next_idx = (next_idx + delta).rem_euclid(len);
            let field = FormField::ORDER[next_idx as usize];
            if self.is_field_visible(field) {
                self.focused = field;
                return;
            }
        }
        self.focused = FormField::ORDER[cur as usize];
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
        if !self.is_field_visible(self.focused) {
            self.move_focus(1);
        }
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
            feature_streaming: self.site_type == SiteTypeChoice::Proxy && self.feature_streaming,
            feature_websocket: self.site_type == SiteTypeChoice::Proxy && self.feature_websocket,
            feature_large_body: match self.site_type {
                SiteTypeChoice::Proxy => self.feature_large_body,
                SiteTypeChoice::Emby => true,
                SiteTypeChoice::Static => false,
            },
            feature_cors: self.site_type == SiteTypeChoice::Proxy && self.feature_cors,
            feature_long_timeout: match self.site_type {
                SiteTypeChoice::Proxy => self.feature_long_timeout,
                SiteTypeChoice::Emby => true,
                SiteTypeChoice::Static => false,
            },
            feature_spa_mode: self.site_type == SiteTypeChoice::Static && self.static_spa_mode,
            feature_static_cache: self.site_type == SiteTypeChoice::Static && self.static_cache,
            feature_block_sensitive: self.site_type == SiteTypeChoice::Static
                && self.static_block_sensitive,
            enable_now: self.enable_now,
            request_cert: self.request_cert,
        })
    }
}

/// 切分逗号或空格分隔的域名字符串，返回非空域名列表。
pub(crate) fn split_aliases(input: &str) -> Vec<&str> {
    input
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}
