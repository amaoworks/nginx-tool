# Nginx-Tools

一套轻量级的 Nginx 站点管理工具集，提供两种使用方式：

- 🐚 **Shell 版**：纯 Bash 脚本，命令行参数式操作（`ng` / `ngmon`）
- 🖥️ **TUI 版**：Rust 编写的全屏交互式终端界面（`ngtool`），单二进制零依赖

> 适用于基于 Debian/Ubuntu 且使用 `sites-available` / `sites-enabled` 目录结构的 Nginx 环境。

---

## ✨ 功能特性

| 功能 | Shell 版 | TUI 版 |
|------|:------:|:----:|
| 🌐 站点管理（创建 / 启用 / 禁用 / 编辑 / 删除） | ✅ | ✅ |
| 🔐 SSL 证书申请、续期与自动续签 | ✅ | ✅ |
| 📊 状态监控（Nginx / 站点 / 证书 / 资源） | ✅ | ✅ |
| 📄 日志查看（访问日志 / 错误日志） | ✅ | ✅（含搜索 / 暂停） |
| 💾 配置备份与还原 | ✅ | ✅（含 sha256 校验） |
| 🪟 仪表盘视图 | 单次输出 | 实时刷新 |
| ⌨️ 表单 / 注入槽编辑器 | – | ✅ |
| 🛡️ 只读模式（非 root 自动降级） | – | ✅ |

---

## 📦 项目结构

```
Nginx-Tools/
├── install.sh              # 安装 / 状态检测 / 更新 / 卸载脚本
├── README.md
├── shell/                  # Shell 版相关脚本
│   ├── nginx-site.sh       # 站点管理脚本
│   ├── nginx-monitor.sh    # 状态监控脚本
│   └── templates/
│       ├── proxy.template  # 反向代理模板（通用）
│       ├── emby.template   # 流媒体反代模板（Emby / Jellyfin）
│       └── static.template # 静态网站模板
└── tui-next/               # TUI 版（Rust 源码 + 文档）
    ├── Cargo.toml
    ├── src/                # 源码
    ├── doc/                # 设计文档
    └── README.md
```

---

## 🚀 快速开始

### 前置条件

- 操作系统：**Debian** / **Ubuntu**
- TUI 版架构支持：**x86_64 (amd64)** / **aarch64 (arm64)**
- 需要 **root 权限**或 **sudo** 执行
- 安装脚本会自动检测并提示安装 Nginx、Certbot 等依赖

### 一键安装（交互式选择）

```bash
# 远程执行（推荐）—— 先检测当前安装状态，再进入安装/更新/卸载菜单
bash <(curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh)

# 或通过管道执行
curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh | bash
```

### 直接指定模式

```bash
# 只装 Shell 版
curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh | bash -s -- shell

# 只装 TUI 版（自动识别系统架构 amd64 / arm64）
curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh | bash -s -- tui

# 手动克隆后执行
git clone https://github.com/amaoworks/nginx-tool.git ~/nginx
sudo bash ~/nginx/install.sh        # 交互式
sudo bash ~/nginx/install.sh shell  # Shell 版
sudo bash ~/nginx/install.sh tui    # TUI 版
```

### 检查安装状态与更新

```bash
# 查看 shell / tui 是否已安装，以及当前是否有新版本
sudo bash ~/nginx/install.sh status

# 更新所有已安装组件；未安装的组件会自动跳过
sudo bash ~/nginx/install.sh update

# 远程执行
curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh | bash -s -- status
curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh | bash -s -- update

# 无参数远程执行也会先检测状态；检测到已安装组件可更新时，回车默认执行更新
curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh | bash
```

### 两种模式都做了什么？

#### Shell 版

- ✅ 检测操作系统（仅支持 Debian / Ubuntu）
- ✅ 识别终端类型（Bash / Zsh / Fish）并写入对应别名
- ✅ 检查并自动安装 Git（如果缺失）
- ✅ 检查 Nginx / Certbot 是否已安装，未安装则提示一键安装
- ✅ 克隆仓库到 `~/nginx`
- ✅ **重复执行时自动 `git pull` 升级**
- ✅ 注册 `ng`（站点管理）与 `ngmon`（状态监控）两条快捷命令

#### TUI 版

- ✅ 检测操作系统与 CPU 架构（`x86_64` → `amd64`，`aarch64` → `arm64`）
- ✅ 自动从 [最新 Release](https://github.com/amaoworks/nginx-tool/releases/latest) 下载对应架构的 `ngtool` 二进制
- ✅ 校验 ELF 文件头后安装到 `/usr/local/bin/ngtool`
- ✅ 检查 Nginx / Certbot 是否已安装，未安装则提示一键安装
- ✅ 可通过脚本更新到最新 Release
- ✅ 直接全局可用，无需配置 Shell 别名

#### 管理脚本

- ✅ 默认无参数执行时会先检测当前安装状态，再进入安装/更新/卸载菜单
- ✅ `status`：检测当前是否已安装 Shell / TUI 版，并显示版本/更新状态
- ✅ `update`：更新已安装的 Shell 仓库与 TUI 二进制
- ✅ 无参数执行时若检测到已安装组件可更新，菜单默认项自动切到 `update`
- ✅ `uninstall`：自动检测已安装组件并卸载

### 一键卸载

```bash
# 本地卸载（自动检测并移除已安装组件，shell + tui 同时清理）
sudo bash ~/nginx/install.sh uninstall

# 远程卸载
curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh | bash -s -- uninstall
```

卸载脚本会：

- 检测并移除 Shell 版（删除 `~/nginx` 与 Shell 别名）
- 检测并移除 TUI 版（删除 `/usr/local/bin/ngtool`）
- 可选删除备份目录（`~/nginx-backups`）
- **不会卸载 Nginx 和 Certbot**

### 安装完成后

- **Shell 版**：重新加载终端或执行 `source ~/.bashrc`（Zsh 用 `.zshrc`，Fish 用 `config.fish`），即可使用 `ng` / `ngmon`
- **TUI 版**：直接执行 `ngtool` 启动主界面（无需 reload Shell）

---

## 🐚 Shell 版命令参考

### 站点管理

```bash
ng list                                        # 列出所有站点及启用状态
ng new                                         # 交互式创建新站点
ng new <名称> <域名> <目标> [选项]              # 非交互式创建
    --type proxy|emby|static                   # 指定站点类型（默认 proxy）
    --enable                                   # 创建后立即启用
    --cert                                     # 创建后申请 SSL 证书
ng enable <站点名>                             # 启用站点（创建符号链接 + 自动 reload）
ng disable <站点名>                            # 禁用站点（删除符号链接 + 自动 reload）
ng delete <站点名>                             # 删除站点配置（含确认提示）
ng edit <站点名>                               # 编辑配置（nano，保存后可选 reload）
```

**`<目标>` 支持以下格式：**

| 格式 | 示例 | 解析结果 |
|------|------|---------|
| 纯端口号 | `8096` | `http://127.0.0.1:8096` |
| IP:端口 | `192.168.1.5:8096` | `http://192.168.1.5:8096` |
| HTTP 地址 | `http://10.0.0.1:8096` | `http://10.0.0.1:8096` |
| HTTPS 地址 | `https://10.0.0.1:8920` | `https://10.0.0.1:8920`（启用 SSL 反代） |

### SSL 证书

```bash
ng cert <域名>                   # 使用 Certbot 申请 Let's Encrypt 证书
ng renew                         # 手动续期所有证书
ng auto-renew                    # 配置证书自动续签（systemd timer 或 cron）
```

### 系统操作

```bash
ng test                          # 测试 Nginx 配置语法
ng reload                        # 测试通过后重载配置
ng restart                       # 重启 Nginx 服务
ng status                        # 查看 Nginx systemd 状态
```

### 日志与备份

```bash
ng logs [站点名]                 # 实时查看访问日志（不指定则查看全部）
ng errors [站点名]               # 实时查看错误日志
ng backup                        # 打包备份配置到 ~/nginx-backups/（保留最近 10 份）
ng restore                       # 交互式选择备份并还原（还原前自动备份当前配置）
```

### 监控面板

```bash
ngmon                            # 输出 Nginx 运行状态仪表盘
```

---

## 🖥️ TUI 版

启动主界面：

```bash
ngtool                       # 进入仪表盘
ngtool --readonly            # 强制只读模式
ngtool --version             # 查看版本（Release 构建时跟随 Git tag）
ngtool --help                # 命令行参数
```

主要功能：

- **仪表盘**：Nginx 服务状态、版本、已启用站点数、SSL 概览、磁盘 / 内存、最近错误日志
- **站点管理**：列表 / 新建（代理 / Emby / 静态）/ 表单或原始模式编辑 / 启用 / 停用 / 删除 / 单站点证书申请 / 跳转日志
- **证书管理**：按站点关联的证书列表、续期、自动续签状态检查
- **日志查看**：全局与站点级访问 / 错误日志实时跟踪、暂停 / 清屏 / 搜索
- **服务控制**：测试配置、重载、重启（确认弹窗）、查看 systemd 状态
- **版本更新**：在“服务控制”页检查最新 Release，也可直接更新当前 `ngtool` 二进制
- **备份还原**：限定到 `nginx.conf` + `sites-available/*.conf` + `sites-enabled` 启用关系，含 manifest 与 sha256 校验

更详细的快捷键、配置目录、源码构建说明见 [`tui-next/README.md`](tui-next/README.md)。

### TUI 版数据目录

```
~/.local/ngtool/
├── config.toml         # 用户配置（首次启动自动创建带注释样例）
├── backups/            # 备份归档（tar.gz + manifest.toml）
├── logs/
│   ├── tui.log         # 应用日志（tracing）
│   └── audit.log       # 操作审计（每行一条 JSON，写操作必记）
├── cache/              # 仪表盘 / 证书缓存
└── tmp/                # 原子写中转目录
```

非 root 启动会自动进入**只读模式**，可浏览全部数据但禁用所有写操作。

---

## 💡 完整使用示例（Shell 版）

### 示例一：通用 Web 服务（HTTP 反代）

```bash
# 一键创建 + 启用 + 申请证书
ng new newapi newapi.example.com 3000 --enable --cert

# 或分步执行：
ng new newapi newapi.example.com 3000   # 创建配置
ng enable newapi                         # 启用站点
ng cert newapi.example.com               # 申请证书

# 查看日志确认正常
ng logs newapi

# 备份配置
ng backup
```

### 示例二：Emby / Jellyfin 流媒体（支持 HTTP / HTTPS 反代）

```bash
# HTTP 反代（Emby 运行在本机 8096 端口）
ng new emby emby.example.com 8096 --type emby --enable --cert

# HTTPS 反代（Emby 使用 HTTPS，例如套了内网证书）
ng new emby emby.example.com https://192.168.1.5:8920 --type emby --enable --cert
```

Emby 模板包含以下专项优化：

- **协议自适应**：`proxy_pass` 根据目标地址自动使用 `http://` 或 `https://`
- **HTTPS 后端 SNI**：`proxy_ssl_name` + `proxy_ssl_server_name on`，确保 SSL 握手正确
- **Gzip 压缩**：自动压缩文本类响应（JSON / HTML / CSS / JS / SVG / 字体），减小传输体积
- **CORS 跨域**：`Access-Control-Allow-Origin *`，兼容 Infuse、Fileball 等第三方客户端
- **WebSocket 支持**：透传 `Upgrade` / `Connection` 头，支持实时通信

### 示例三：静态站点

```bash
ng new mysite mysite.example.com / --type static --enable --cert
# 将静态文件上传到 /var/www/mysite/
```

---

## 🔧 配置模板说明（位于 `shell/templates/`）

### 反向代理模板 (`proxy.template`)

适用于通用 Web 服务的反向代理配置，关键配置项：

- `proxy_pass`：转发到上游目标（自动识别 `http://` / `https://`）
- `proxy_http_version 1.1` + `Upgrade` / `Connection` 头：支持 WebSocket
- `proxy_buffering off`：关闭代理缓冲，适合流式响应
- 超时设置：连接 60s，读写 300s

### Emby 专用模板 (`emby.template`)

针对 Emby / Jellyfin 流媒体服务优化的反向代理配置：

- **协议自适应**：`proxy_pass` 根据 `UPSTREAM_SCHEME` 自动使用 `http://` 或 `https://`
- **HTTPS 后端 SNI**：`proxy_ssl_name` + `proxy_ssl_server_name on`
- **Gzip 压缩**：自动压缩文本类响应（JSON / HTML / CSS / JS / SVG / 字体）
- **CORS 跨域**：`Access-Control-Allow-Origin *`
- **WebSocket 支持**：透传 `Upgrade` / `Connection` 头
- **Host 头回写**：确保后端正确识别请求来源

### 静态站点模板 (`static.template`)

用于托管纯静态文件的站点配置：

- 文档根目录：`/var/www/<站点名>`
- 静态资源（图片、CSS、JS 等）设置 7 天缓存 + `immutable` 标志

---

## ⚠️ 注意事项

- 安装脚本需要 **root 权限**或使用 **sudo** 执行
- 确保 Nginx 使用 `sites-available` / `sites-enabled` 目录结构（Debian/Ubuntu 默认如此）
- 申请 SSL 证书前确保 **域名已解析到服务器** 且 **80 端口可访问**
- TUI 版当前仅提供 **amd64 / arm64** Linux 预编译二进制，其他架构请使用 Shell 版或自行从 `tui-next/` 源码构建
- Shell 版与 TUI 版可同时安装、互不冲突
- `install.sh status` 在无网络、无 `curl` 或非受支持架构下，仍会显示本地安装状态；仅最新 Release 查询会降级失败
- 建议定期执行 `ng backup`（或 TUI 版的备份功能）保存配置

---

## 📦 Release 与 CI

仓库根的 [`/.github/workflows/release.yml`](.github/workflows/release.yml) 在每次推送 `v*` tag 时自动触发：

- 在 `ubuntu-22.04`（glibc 2.35）上构建 `ngtool` Release 二进制
- 同时输出 **amd64**（`x86_64-unknown-linux-gnu`）与 **arm64**（`aarch64-unknown-linux-gnu`，cross-compile）
- 仅产出二进制，不打包压缩文件，命名格式：`ngtool-<tag>-linux-<arch>`
- 自动创建 / 更新对应 tag 的 GitHub Release，并把二进制作为 asset 上传

发布新版本：

```bash
# 本地打 tag 并推送，CI 会自动构建并发布
git tag v1.0.4
git push origin v1.0.4
```

`install.sh` 在 `tui` / `status` / `update` 相关流程里会调用
`https://api.github.com/repos/amaoworks/nginx-tool/releases/latest`，
解析出与当前架构匹配的 asset，用于检测或安装 `/usr/local/bin/ngtool`。

TUI 的 `--version` 与界面内版本检查使用统一的构建版本号：
本地在某个 tag 上构建时会自动读取该 tag；CI Release 构建时也会显式注入对应 tag 版本。
`Cargo.toml` 的 package version 也需要随发布号同步维护，否则 Cargo 编译日志会继续显示旧的
`Compiling ngtool v...`，即使最终二进制版本已由 tag 注入。

---

## 📄 License

MIT License
