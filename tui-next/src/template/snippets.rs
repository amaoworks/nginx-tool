// 注入槽预定义片段库，详见 design.md §九 模板快插片段库

use crate::template::config_parser::InjectionSlot;

/// 预定义片段
#[derive(Debug, Clone)]
pub struct Snippet {
    pub name: &'static str,
    pub content: &'static str,
    pub slot: InjectionSlot,
}

/// 安全响应头
const SECURITY_HEADERS: &str = "\
add_header X-Frame-Options DENY;
add_header X-Content-Type-Options nosniff;
add_header X-XSS-Protection \"1; mode=block\";";

/// Gzip 压缩配置
const GZIP_CONFIG: &str = "\
gzip on;
gzip_vary on;
gzip_min_length 1024;
gzip_types text/plain text/css application/json application/javascript text/xml application/xml;
gzip_comp_level 6;";

/// CORS 跨域响应头
const CORS_HEADERS: &str = "\
add_header Access-Control-Allow-Origin *;
add_header Access-Control-Allow-Methods \"GET, POST, OPTIONS\";
add_header Access-Control-Allow-Headers \"Origin, Content-Type, Accept\";";

/// 访问日志配置
const ACCESS_LOG: &str = "\
access_log /var/log/nginx/$site_name.access.log;";

/// 请求限流
const RATE_LIMIT: &str = "\
limit_req_zone $binary_remote_addr zone=general:10m rate=10r/s;
limit_req zone=general burst=20 nodelay;";

/// WebSocket 升级头
const WEBSOCKET_HEADERS: &str = "\
proxy_http_version 1.1;
proxy_set_header Upgrade $http_upgrade;
proxy_set_header Connection \"upgrade\";
proxy_read_timeout 86400;";

/// 代理请求头
const PROXY_HEADERS: &str = "\
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
proxy_set_header X-Forwarded-Proto $scheme;";

/// 反向代理缓冲控制
const PROXY_BUFFERS: &str = "\
proxy_buffering on;
proxy_buffer_size 4k;
proxy_buffers 8 32k;
proxy_busy_buffers_size 64k;";

/// 超时设置
const TIMEOUT_SETTINGS: &str = "\
proxy_connect_timeout 60s;
proxy_send_timeout 60s;
proxy_read_timeout 120s;";

/// 静态资源缓存
const STATIC_CACHE: &str = "\
location ~* \\.(jpg|jpeg|png|gif|ico|css|js|woff2)$ {
    expires 30d;
    add_header Cache-Control \"public, immutable\";
}";

/// 禁止敏感路径
const BLOCK_SENSITIVE: &str = "\
location ~ /\\.(ht|git|svn) {
    deny all;
    return 404;
}";

/// 自定义错误页
const CUSTOM_ERRORS: &str = "\
error_page 404 /404.html;
error_page 500 502 503 504 /50x.html;
location = /50x.html {
    root /usr/share/nginx/html;
}";

static ALL_SNIPPETS: &[Snippet] = &[
    // Before Location 片段
    Snippet {
        name: "安全响应头",
        content: SECURITY_HEADERS,
        slot: InjectionSlot::BeforeLocation,
    },
    Snippet {
        name: "Gzip 压缩配置",
        content: GZIP_CONFIG,
        slot: InjectionSlot::BeforeLocation,
    },
    Snippet {
        name: "CORS 跨域响应头",
        content: CORS_HEADERS,
        slot: InjectionSlot::BeforeLocation,
    },
    Snippet {
        name: "访问日志路径",
        content: ACCESS_LOG,
        slot: InjectionSlot::BeforeLocation,
    },
    Snippet {
        name: "请求限流模板",
        content: RATE_LIMIT,
        slot: InjectionSlot::BeforeLocation,
    },
    // Inside Location 片段
    Snippet {
        name: "WebSocket 升级头",
        content: WEBSOCKET_HEADERS,
        slot: InjectionSlot::InsideLocation,
    },
    Snippet {
        name: "代理请求头",
        content: PROXY_HEADERS,
        slot: InjectionSlot::InsideLocation,
    },
    Snippet {
        name: "反向代理缓冲控制",
        content: PROXY_BUFFERS,
        slot: InjectionSlot::InsideLocation,
    },
    Snippet {
        name: "超时设置",
        content: TIMEOUT_SETTINGS,
        slot: InjectionSlot::InsideLocation,
    },
    // After Location 片段
    Snippet {
        name: "静态资源缓存",
        content: STATIC_CACHE,
        slot: InjectionSlot::AfterLocation,
    },
    Snippet {
        name: "禁止敏感路径",
        content: BLOCK_SENSITIVE,
        slot: InjectionSlot::AfterLocation,
    },
    Snippet {
        name: "自定义错误页",
        content: CUSTOM_ERRORS,
        slot: InjectionSlot::AfterLocation,
    },
];

/// 获取指定注入槽的片段列表
pub fn get_snippets_for_slot(slot: InjectionSlot) -> Vec<&'static Snippet> {
    ALL_SNIPPETS.iter().filter(|s| s.slot == slot).collect()
}
