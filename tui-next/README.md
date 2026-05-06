# ngtool — Nginx-Tools TUI

`tui-next/` 是 Nginx-Tools 的 Rust TUI 实现，单二进制 `ngtool`，把 `nginx-site.sh` 与 `nginx-monitor.sh`
的核心能力整合进一个交互式终端界面。

> 设计、架构、风险与执行计划见 `doc/`。`design.md` 是交互最高准则；本 README 只覆盖如何使用。

## 功能

- **仪表盘**：Nginx 服务状态、版本、已启用站点数、SSL 概览、磁盘 / 内存、最近错误日志
- **站点管理**：列表、新建（代理 / Emby / 静态）、表单 / 原始模式编辑、注入槽全屏编辑、启用 / 停用、删除、单站点证书申请、跳转日志
- **证书管理**：按站点关联的证书列表、续期、自动续签状态检查
- **日志查看**：全局与站点级访问 / 错误日志实时跟踪、暂停 / 清屏 / 搜索
- **服务控制**：测试配置、重载、重启（确认弹窗）、查看 systemd 状态、检查最新 Release
- **备份还原**：范围限定到 `nginx.conf` + `sites-available/*.conf` + `sites-enabled` 启用关系，含 manifest 与 sha256 校验

## 系统要求

| 项目 | 要求 |
|------|------|
| OS | Linux (Debian / Ubuntu 风格) |
| Init | systemd（首版唯一支持） |
| Nginx | `/etc/nginx/sites-available` + `/etc/nginx/sites-enabled` 目录布局 |
| Rust | 1.75+（开发） |
| 可选 | `certbot`（缺失时证书相关功能禁用，但其它功能照常） |

非 root 启动会自动进入只读模式，可浏览全部数据但禁用所有写操作。

## 构建与运行

```bash
cd tui-next
cargo build --release            # 产物 target/release/ngtool

./target/release/ngtool          # 默认进入仪表盘
./target/release/ngtool --readonly        # 强制只读模式
./target/release/ngtool --config /path/to/config.toml
./target/release/ngtool --version
```

OrangePi (aarch64) 与常见 x86_64 Linux 直接 native 编译即可。

## 更新检查

在 TUI 的“服务控制”页可直接执行“检查更新”，会读取 GitHub 最新 Release，
显示当前版本、最新版本、发布时间与发布页面链接。

这只是版本检测，不会在 TUI 内直接覆盖安装二进制。实际更新仍建议使用仓库根的：

```bash
sudo bash install.sh update
```

## 快捷键速查

全局：

| 键 | 说明 |
|----|------|
| `1`–`6` | 跳转到对应一级菜单（仅在非文本输入字段时生效） |
| `q` | 软退出（仅在非文本输入字段时生效） |
| `Ctrl+C` | 硬退出（无视输入状态） |
| `Esc` | 返回上一级 / 取消当前模式 |
| `Tab` / `Shift+Tab` | 切换焦点 |

站点编辑（表单模式）：

| 键 | 说明 |
|----|------|
| `Ctrl+S` | 保存并 `nginx -t` 通过后 reload |
| `Ctrl+W` | 仅保存，不测试不 reload |
| `Ctrl+D` | 重置表单为加载时的原值 |
| `Ctrl+R` | 用当前模板替换槽位 |
| `Ctrl+E` | 进入注入槽全屏编辑 |
| `o` | 切换到原始配置模式 |
| `←` / `→` | 切换注入槽位置（焦点在槽位选择器时） |
| `Space` | 追加模板到当前槽位（焦点在模板列表时） |

站点编辑（原始模式）：

| 键 | 说明 |
|----|------|
| `Ctrl+S` / `Ctrl+W` | 同上 |
| `Ctrl+Z` / `Ctrl+Y` | 撤销 / 重做 |
| `o` | 切换回表单模式 |

注入槽全屏编辑：

| 键 | 说明 |
|----|------|
| `Ctrl+S` | 完成编辑，写回槽位并返回表单模式 |
| `Esc` | 取消，丢弃槽位编辑缓冲 |
| `Ctrl+D` | 清空整个槽位 |
| `Ctrl+Z` / `Ctrl+Y` | 撤销 / 重做 |

详细键位见 `doc/design.md §五`。

## 数据与配置目录

```
~/.local/ngtool/
├── config.toml         # 用户配置（首次启动自动创建带注释样例）
├── backups/            # 备份归档（tar.gz + manifest.toml）
├── logs/
│   ├── tui.log         # 应用日志（tracing）
│   └── audit.log       # 操作审计（每行一条 JSON，写操作必记）
├── cache/              # 仪表盘 / 证书缓存
└── tmp/                # 原子写中转目录（启动清理 7 天前残留）
```

启动时若检测到已有实例占用 `~/.local/ngtool/tui.lock`，新实例降级为只读模式打开。

## 已知限制（首版）

- **目录布局**：固定 Debian / Ubuntu 风格 `/etc/nginx/sites-available` + `sites-enabled`，CentOS / Alpine / Docker 默认布局未支持
- **Init 系统**：仅 systemd
- **界面语言**：仅中文，无 i18n
- **证书申请**：仅 `certbot --nginx` HTTP-01；通配符与 DNS-01 challenge 暂不支持；必须先创建站点并配置 `server_name` 再申请
- **备份范围**：限定为 `nginx.conf` + `sites-available/*.conf` + `sites-enabled` 启用关系；`conf.d/` `snippets/` `modules-enabled/` 等不在备份 / 还原范围
- **内置编辑器**：手写实现；架构层保留 `tui-textarea` 候选（参见 `doc/architecture.md §12.3`）
- **外部编辑器入口**（`$EDITOR`）：架构已规划，但首版未集成

## 开发循环

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo run -- --readonly
```

## 跨架构发布（P10）

```bash
rustup target add aarch64-unknown-linux-gnu
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu
```

## 进一步阅读

| 文档 | 内容 |
|------|------|
| `doc/design.md` | 交互设计稿（最高准则） |
| `doc/architecture.md` | 工程架构与决策 |
| `doc/execution.md` | P0–P10 执行计划与进度量化 |
| `doc/risks.md` | 风险登记与处理规则 |

## 许可

MIT
