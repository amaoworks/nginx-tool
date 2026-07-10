/// 路由：一级菜单 + 子路由。详见 architecture.md §7.3。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Dashboard,
    Sites(SitesRoute),
    Certs,
    Logs,
    Service,
    Backup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SitesRoute {
    List,
    #[allow(dead_code)]
    New,
    EditManaged {
        site_name: String,
    },
    EditAdvanced {
        site_name: String,
    },
    EditRaw {
        site_name: String,
    },
    /// 注入槽全屏编辑模式（design.md 子模式 C，由 Ctrl+E 进入）
    EditSlotFull {
        site_name: String,
        slot: crate::template::config_parser::InjectionSlot,
    },
}

/// 一级菜单项。固定 6 个，对应 design.md §六 菜单结构树。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Dashboard,
    Sites,
    Certs,
    Logs,
    Service,
    Backup,
}

impl MenuItem {
    pub const ALL: [MenuItem; 6] = [
        MenuItem::Dashboard,
        MenuItem::Sites,
        MenuItem::Certs,
        MenuItem::Logs,
        MenuItem::Service,
        MenuItem::Backup,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            MenuItem::Dashboard => "📊 总览",
            MenuItem::Sites => "📁 站点",
            MenuItem::Certs => "🔐 证书",
            MenuItem::Logs => "📋 日志",
            MenuItem::Service => "⚙️ 服务",
            MenuItem::Backup => "💾 备份",
        }
    }

    #[allow(dead_code)]
    pub fn shortcut(&self) -> char {
        match self {
            MenuItem::Dashboard => '1',
            MenuItem::Sites => '2',
            MenuItem::Certs => '3',
            MenuItem::Logs => '4',
            MenuItem::Service => '5',
            MenuItem::Backup => '6',
        }
    }

    pub fn default_route(&self) -> Route {
        match self {
            MenuItem::Dashboard => Route::Dashboard,
            MenuItem::Sites => Route::Sites(SitesRoute::List),
            MenuItem::Certs => Route::Certs,
            MenuItem::Logs => Route::Logs,
            MenuItem::Service => Route::Service,
            MenuItem::Backup => Route::Backup,
        }
    }
}
