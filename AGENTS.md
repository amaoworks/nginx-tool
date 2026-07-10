# nginx-tool 指南

一套轻量级的 Nginx 站点管理工具集，提供 Shell 和 TUI 两套工具，部署在 GitHub。

---

## 技术栈

| 模块 | 技术 |
|------|------|
| **Shell 版** | Bash 脚本（`#!/bin/bash`，兼容 bash 4+），无项目级依赖，运行依赖 Debian/Ubuntu 常见系统工具 |
| **TUI 版** | Rust（edition 2021，最低 1.75），ratatui 0.29 + crossterm 0.28 |
| **安装脚本** | `install.sh`，纯 Bash 脚本，支持远程管道执行 |
| **CI** | GitHub Actions，Linux ubuntu-22.04（glibc 2.35） |
| **模板引擎**（TUI） | minijinja 2 |

## 文件路径

```
Nginx-Tools/
├── install.sh              # 安装/检测/更新/卸载（入口脚本）
├── shell/
│   ├── nginx-site.sh       # 站点管理（ng 命令）
│   ├── nginx-monitor.sh    # 状态监控（ngmon 命令）
│   └── templates/          # 站点模板
│       ├── proxy.template  # 通用反向代理
│       ├── emby.template   # Emby/Jellyfin 流媒体
│       └── static.template # 静态网站
├── .gitignore              # 项目级 gitignore（覆盖 Rust 产物）
├── tui-next/
│   ├── Cargo.toml          # Rust 项目配置
│   ├── build.rs            # 构建脚本（注入版本号）
│   ├── CHANGELOG.md        # TUI 变更记录
│   ├── src/                # Rust 源码
│   │   ├── main.rs         # 入口
│   │   ├── app/            # 应用层（状态管理）
│   │   ├── ui/             # 界面层（ratatui 组件）
│   │   ├── domain/         # 领域层（站点/证书等模型）
│   │   ├── infra/          # 基础设施层（系统命令、文件操作）
│   │   ├── config/         # 配置管理
│   │   ├── template/       # 模板解析与渲染
│   │   ├── error.rs        # 统一错误类型
│   │   └── version.rs      # 版本号逻辑
│   ├── templates/          # TUI 内置 Jinja 模板
│   │   ├── proxy.conf.j2
│   │   ├── emby.conf.j2
│   │   └── static.conf.j2
│   ├── tests/
│   │   └── smoke.rs        # 冒烟测试（CJK 宽字符、基础布局）
│   └── doc/                # 设计文档（修改需许可）
│       ├── design.md
│       ├── architecture.md
│       ├── execution.md
│       └── risks.md
├── .github/
│   └── workflows/
│       ├── ci.yml          # push/PR：cargo test + clippy（tui-next）
│       └── release.yml     # tag：交叉编译并发布二进制
├── AGENTS.md
├── LICENSE                 # MIT
└── README.md
```

## 构建与测试

### TUI 版（Rust）

```bash
# 构建 Release 二进制
cd tui-next && cargo build --release

# 运行单元测试 + 冒烟测试
cd tui-next && cargo test --locked

# 运行冒烟测试（仅 P1）
cd tui-next && cargo test --test smoke

# Lint（与 CI 一致）
cd tui-next && cargo clippy --locked --all-targets -- -D warnings
```

- Release 配置：`opt-level = 3`，`lto = "thin"`，`strip = "symbols"`
- CI 交叉编译 arm64 需安装 `gcc-aarch64-linux-gnu` + `libc6-dev-arm64-cross`
- 错误处理使用 `anyhow` + `thiserror`，不直接用 `unwrap()` / `expect()`

### Shell 版（Bash）

```bash
# Shell 版无需构建，直接用 bash 运行
bash shell/nginx-site.sh --help
bash shell/nginx-monitor.sh

# 建议用 shellcheck 检查脚本质量
shellcheck shell/*.sh install.sh
```

## 代码规范

### Shell（Bash）
- 函数命名：`snake_case`
- 全局变量：`UPPER_SNAKE_CASE`
- 本地变量：`local lowercase`
- 新增/修改的复杂字符串判断优先使用 `[[ ... ]]`；不为风格统一单独批量替换既有 `[ ... ]`
- 统一使用 `#!/bin/bash` shebang

### TUI（Rust）
- 遵循 Rust 2021 edition 风格
- 模块按层拆分：`app/` → `ui/` → `domain/` → `infra/` → `config/`
- 使用 `anyhow::Result` 统一错误返回，`thiserror` 定义领域错误
- UI 组件使用 `ratatui` widget 模式，保持 render 函数纯化
- 状态机模式管理页面导航（见 `architecture.md`）
- 模板相关逻辑位于 `src/template/`，内置模板文件位于 `tui-next/templates/`
- 日志使用 `tracing`（输出到 `~/.local/ngtool/logs/tui.log` + 审计日志）

## 发布流程

```bash
# 1. 更新 tui-next/Cargo.toml 中的 version 字段（必须与 tag 一致，例如 v1.0.6 → version = "1.0.6"）
# 2. 确保 CHANGELOG.md 记录了本次变更
# 3. 提交并推送
git status
git add <本次发布相关文件>
git commit -m "chore: release v1.0.x"
# 4. 打 tag 推送，CI 自动构建两个架构的 Release
git tag v1.0.x
git push origin v1.0.x
```

> **注意**：`tui-next/Cargo.toml` 的 `version` 字段必须与 tag 版本号保持一致（无 `v` 前缀）。
> CI 构建时通过 `NGTOOL_BUILD_VERSION` 环境变量注入 tag 版本，若 Cargo.toml 版本滞后，
> `cargo build` 编译日志仍会显示旧版本号，导致混淆。

- 二进制命名格式：`ngtool-<tag>-linux-<arch>`（amd64 / arm64）
- 构建环境和运行时目标：Ubuntu 22.04（glibc 2.35），兼容 Debian 12+ / Ubuntu 22.04+
- TUI 版版本号通过环境变量 `NGTOOL_BUILD_VERSION` 由 build.rs 注入

## Git 工作流

- git的所有操作都需要指令去触发，例如：commit, push, 合入本地, 提交远端
- 直接推送 `main` 分支，不强制 PR 流程
- Commit message 风格：`type: description` 前缀
  - `feat:` — 新功能
  - `fix:` — 修复
  - `chore:` — 杂项（发布、配置、文档）
  - `ci:` — CI 配置变更
  - `doc:` — 文档
- tag 格式：`v<major>.<minor>.<patch>`（如 `v1.0.6`）

## 要求

- 所有改动都为最小改动
- `tui-next/doc/` 下的设计文档需要许可才能改动
- 思考使用英文，输出使用中文
- 禁止自行修改.git下的内容不经过确认

@RTK.md
