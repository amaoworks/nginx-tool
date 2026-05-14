# 变更记录

## 2026-05-14 — v1.1.0 TUI 按键提示与编辑快捷键收敛

### 背景

站点管理相关页面同时存在外层页脚提示和内层视图提示，用户需要在两处阅读按键说明；编辑流程继续依赖 `Ctrl+W`、`Ctrl+S`、`Ctrl+R`、`Ctrl+E` 等组合键，在浏览器终端、WebTTY、部分桌面终端中容易与宿主快捷键冲突。与此同时，托管 / 高级 / 原始三种编辑模式的切换入口也不统一，导致学习成本偏高。

### 修改清单

- `src/ui/mod.rs` 与各视图渲染：页脚改为唯一主提示源，移除或弱化页面内部重复提示，按当前焦点动态展示可用操作。
- `src/app/state.rs`：站点编辑主流程改用函数键族，统一为 `F2` 保存、`F3` 保存并测试、`F4` 重置、`F5/F6` 模式切换、`F7/F8` 槽位高级操作、`F9/F10` 撤销 / 重做。
- `src/app/state.rs`：移除编辑主流程对 `Ctrl+W`、`Ctrl+S`、`Ctrl+R`、`Ctrl+E`、`Ctrl+Z`、`Ctrl+Y` 以及 `a/o` 模式切换的依赖，避免和宿主环境快捷键冲突。
- `src/ui/views/site_edit*.rs`：补充模式切换提示文案，让托管 / 高级 / 原始三条路径的入口保持一致。
- `doc/design.md` / `doc/execution.md` / `doc/risks.md`：补齐分层按键分发、文本输入保护和兼容性约束，明确输入框中的普通字符不会触发页面动作。
- `README.md` / `tui-next/README.md`：同步新的用户可见键位说明。
- `Cargo.toml` / `Cargo.lock`：包版本提升至 `1.1.0`，与发布 tag 对齐。

## 2026-05-07 — 发布更新链路修复与 TUI 自升级

### 背景

安装脚本在 `ngtool --version` 输出带 `v` 前缀时无法解析本地版本，导致云端 Release 更新但本地状态无法可靠对比；TUI 内也只有“检查更新”，没有直接升级当前二进制的能力。同时 `Cargo.toml` 仍停留在旧版本，GitHub Actions 编译日志显示 `Compiling ngtool v1.0.2`。

### 修改清单

- `install.sh`：本地版本解析兼容 `v1.0.4` 和 `1.0.4`；状态检测静默解析 Release asset；无参数菜单检测到可更新时默认项自动切到“更新”。
- `install.sh`：修复 `set -e` 与 `compare_versions` 返回码冲突；当本地版本低于最新 Release 时，状态检测和更新流程不再提前退出。
- `src/domain/update.rs`：新增 Release asset 解析、架构匹配、ELF 校验和自升级替换当前二进制；版本比较改为只在远端版本更高时提示更新。
- `src/app/event.rs` / `src/app/state.rs` / `src/main.rs`：新增 `ServiceUpgradeResult` 和“更新 TUI”服务按钮。
- `src/ui/views/service.rs`：服务控制页增加“更新 TUI”按钮，并在只读模式下禁用。
- `Cargo.toml` / `Cargo.lock`：package version 升至 `1.0.4`，使 Cargo 编译日志和发布 tag 对齐。
- `build.rs`：CI 注入的 `v*` tag 标准化为无前缀版本号，统一 `ngtool --version`、UI 检查和备份 manifest。

### 使用说明

- 安装脚本：`curl -fsSL .../install.sh | bash` 仍先展示状态；若检测到已安装组件可更新，直接回车会执行更新。
- TUI：服务控制页先“检查更新”查看信息，再执行“更新 TUI”；更新完成后退出并重新启动 `ngtool` 生效。

## 2026-05-07 — 站点管理：删除/证书/日志快捷键功能补齐

### 背景

站点管理页面底部提示栏显示 `[d] 删除` `[c] 证书` `[l] 日志` 三个快捷键，但按键事件未实现，按下后无响应。用户无法从站点列表直接执行删除、申请证书或查看日志。

### 修改清单

#### 领域层：新增 `delete_site` 函数
- `src/domain/site.rs`：新增 `delete_site(ctx, name)` —— 若站点已启用则先停用，再删除 `sites-available` 配置文件，最后 `nginx -t` + reload。删除前不自动恢复已停用状态，删除后不可恢复。
- 审计日志动作标识：`site.delete`。

#### 事件层：新增 `SiteDeleteResult` 事件
- `src/app/event.rs`：新增 `SiteDeleteResult { site_name, result }` 变体。

#### 状态层：补齐按键处理
- `src/app/state.rs`：
  - `SitesState` 新增 `pending_delete: Option<String>` 字段
  - `handle_key` 中 `Route::Sites(SitesRoute::List)` 新增三个按键分支：
    - `d` → 弹出确认删除弹窗（`ModalAction::DeleteSite`）
    - `c` → 为当前站点申请证书并跳转到证书管理页
    - `l` → 跳转到日志查看页，自动选中当前站点的访问日志
  - `request_site_delete()` / `request_cert_for_current_site()` / `goto_site_log()` 三个新方法
  - `execute_modal_action` 处理 `DeleteSite`：设置 `action_in_flight` + `pending_delete`
  - `handle_event` 处理 `SiteDeleteResult`：清除 `action_in_flight`、通知、刷新列表

#### 弹窗层：新增 `DeleteSite` 确认动作
- `src/ui/modal.rs`：`ModalAction` 新增 `DeleteSite { site_name: String }` 变体。

#### 主循环：异步派发删除任务
- `src/main.rs`：新增 `take_site_delete_request` 消费分支，派发 `domain::site::delete_site` 异步任务。

### 不变更的部分

- 站点列表 `[n] 新建` `[e] 编辑` `[Enter] 启停` `[r] 刷新` 行为不变。
- 证书页、日志页自身按键逻辑不变。

## 2026-05-06 — Tab / Esc 焦点切换语义修订（design v0.7）

### 背景

用户反馈：在「证书管理 / 日志查看 / 服务控制 / 备份还原」四个视图中，进入子区域后无法用 `Tab` 或方向键回到左侧栏，操作上"卡住"。

### 诊断结论

属于"设计稿模糊 + 实现遗漏"双重问题：

1. **设计稿**：`§五 全局快捷键` 把 `Tab` 定义为"侧栏 ↔ 主视图切换"，但 `§四 各视图详细设计` 又在证书 / 日志 / 服务三个视图覆盖 `Tab` 为"子区域循环"，未规定子区域内如何回到侧栏。
2. **实现层（`src/app/state.rs`）**：
   - 证书页 `handle_certs_key`、日志页 `handle_logs_key`、服务页（全局分发处早返回）将 `Tab` 全部消费为内部循环，只有 `Esc` 能回侧栏。
   - 备份页 `handle_backup_key` 完全没处理 `Tab` / `Left` / `Right`，按下后 `match` 落入 `_=>{}` 静默吞掉，体感"按了没反应"。

### 选定方案

**维持原设计 + 强化 Esc。** 理由：方案改动最小，且 `Esc` 在 4/5 个顶层视图中已经实现"返回侧栏"，只需补齐文档与 UI 提示即可。

### 修改清单

#### 设计文档
- `doc/design.md §五`：在快捷键表后追加分层语义说明，明确：
  - 顶层视图按 `Esc` 统一回到侧栏
  - `Tab` 由各视图自行覆盖：站点列表回侧栏；证书 / 日志 / 服务在子区域循环；备份不响应
  - 子模式按 `Esc` 表示"回上一级或取消"，再次 `Esc` 才回侧栏
- `doc/design.md` 末尾：版本升至 v0.7（2026-05-06），追加变更条目

#### UI 状态栏（统一提示 [Esc] 返回侧栏）
- `src/ui/mod.rs::footer_hints`：在 Dashboard / Sites/List / Certs / Logs / Service / Backup 六个顶层路由的 tips 列表末尾追加 `[Esc] 返回侧栏`；Logs 同步补 `[Tab] 切换区域`，Service 把 `[Tab] 切换` 改写为 `[Tab] 切换按钮`。
- `src/ui/views/logs.rs::render_status`：非搜索模式下追加 `[Esc] 返回侧栏`。
- `src/ui/views/certs.rs::render_status_line`：状态行追加 `[Esc] 返回侧栏`。
- `src/ui/views/backup.rs::render_status_line`：正常模式与只读模式两条提示均追加 `[Esc] 返回侧栏`。
- `src/ui/views/service.rs::render_output`：空输出占位文案补 `Esc 返回侧栏`。

#### 行为修订
- `src/app/state.rs::handle_backup_key`：将 `Tab` / `BackTab` / `Left` / `Right` 从隐式 `_=>{}` 改为显式空分支并加注释，与设计稿"备份页不响应 Tab"的新约定一致；行为本身未变更（仍然不消费这些键），改动只为代码自解释。

### 不变更的部分

- 证书 / 日志 / 服务页的视图内 `Tab` 子区域循环逻辑保留，符合 `§四` 各视图设计。
- 各视图的 `Esc → 返回侧栏` 逻辑此前已实现，本次未触碰按键分发，仅补 UI 提示。
- 站点列表 `Tab → 侧栏` 行为不变。

### 后续待办（不在本次范围）

- 若用户反馈"两次 Tab 回侧栏"的需求依然强烈，可在新版设计中考虑"溢出回弹"（方案 B）或新增独立按键（方案 C）。
- 帮助页（F1）尚未实现，理想情况下应在帮助里完整展示分层语义。
