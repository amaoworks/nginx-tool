# Nginx-Tools

一套轻量级的 Nginx 站点管理工具集，提供两种使用方式：

- 🐚 **Shell 版**：纯 Bash 脚本，命令行操作（`ng` / `ngmon`）
- 🖥️ **TUI 版**：Rust 编写的全屏交互界面（`ngtool`），单二进制零依赖

> 适用于 Debian/Ubuntu，使用 `sites-available` / `sites-enabled` 目录结构的 Nginx 环境。

---

## 功能一览

| 功能 | Shell | TUI |
|------|:-----:|:---:|
| 站点管理（创建/启用/禁用/编辑/删除） | ✅ | ✅ |
| SSL 证书申请、续期与自动续签 | ✅ | ✅ |
| 状态监控（Nginx/站点/证书/资源） | ✅ | ✅ |
| 日志查看（访问/错误日志） | ✅ | ✅（含搜索/暂停） |
| 配置备份与还原 | ✅ | ✅（含 sha256 校验） |
| 仪表盘视图 | 单次输出 | 实时刷新 |
| 表单/注入槽编辑器 | – | ✅ |
| 只读模式（非 root 自动降级） | – | ✅ |

## 快速安装

```bash
# 交互式（推荐）—— 检测状态后进入安装/更新/卸载菜单
bash <(curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh)

# 指定 Shell 版
bash <(curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh) shell

# 指定 TUI 版（自动识别 amd64 / arm64）
bash <(curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh) tui

# GitHub 加速
curl -fsSL https://ghfast.top/https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh | bash -s -- --proxy https://ghfast.top tui
```

安装完成后：
- **Shell 版**：重载终端后使用 `ng`（站点管理）和 `ngmon`（监控面板）
- **TUI 版**：直接执行 `ngtool` 启动界面

### 检查与更新

```bash
sudo bash ~/nginx/install.sh status    # 检查安装状态
sudo bash ~/nginx/install.sh update    # 更新已安装组件
sudo bash ~/nginx/install.sh uninstall # 卸载
```

## Shell 版命令速查

| 命令 | 说明 |
|------|------|
| `ng list` | 列出所有站点及状态 |
| `ng new <名称> <域名> <目标> [选项]` | 创建站点（`--type proxy\|emby\|static`、`--enable`、`--cert`） |
| `ng enable/disable/delete/edit <站点名>` | 启用/禁用/删除/编辑站点 |
| `ng cert <域名>` | 申请 SSL 证书 |
| `ng renew` | 手动续期所有证书 |
| `ng auto-renew` | 配置自动续签 |
| `ng test/reload/restart/status` | Nginx 服务控制 |
| `ng logs/errors [站点名]` | 实时查看日志 |
| `ng backup/restore` | 配置备份与还原 |
| `ngmon` | 状态监控面板 |

## TUI 版

```bash
ngtool                  # 启动仪表盘
ngtool --readonly       # 强制只读模式
ngtool --version        # 查看版本
```

功能：仪表盘、站点管理（表单/原始模式编辑）、证书管理、日志查看（实时跟踪/搜索/暂停）、服务控制（测试配置/重载/重启）、版本更新、备份还原（sha256 校验）。

数据目录：`~/.local/ngtool/`（配置、备份、日志、审计、缓存）

> 详细说明见 [`tui-next/README.md`](tui-next/README.md)。

## 模板说明（`shell/templates/`）

- **`proxy.template`**：通用反向代理，支持 WebSocket、自动识别 HTTP/HTTPS 后端
- **`emby.template`**：Emby/Jellyfin 流媒体优化，含 Gzip、CORS、WebSocket、SNI
- **`static.template`**：静态站点，7 天缓存 + `immutable`

## 项目结构

```
Nginx-Tools/
├── install.sh            # 安装/状态检测/更新/卸载脚本
├── shell/                # Shell 版脚本
│   ├── nginx-site.sh     # 站点管理
│   ├── nginx-monitor.sh  # 状态监控
│   └── templates/        # 配置模板
├── tui-next/             # TUI 版（Rust 源码）
│   ├── src/              # 源码
│   ├── doc/              # 设计文档
│   └── README.md
├── LICENSE               # MIT License
└── README.md
```

## 发布新版本

```bash
git tag v1.0.x
git push origin v1.0.x   # CI 自动构建并发布 Release
```

## 注意事项

- 需要 **root 权限**或 **sudo** 执行
- SSL 证书申请前确保 **域名已解析到服务器** 且 **80 端口可访问**
- TUI 版仅提供 **amd64 / arm64** Linux 预编译二进制，其他架构请用 Shell 版或自行构建
- Shell 版与 TUI 版可同时安装、互不冲突
- 建议定期备份配置

## 致谢

- ❤️ 感谢 [**Linux.do**](https://linux.do) 社区提供的交流与分享平台
- 🙏 特别感谢 **@sixsixsix** 的 Shell 脚本思路启发，原帖：[https://linux.do/t/topic/1486160](https://linux.do/t/topic/1486160)

## License

[MIT License](LICENSE)
