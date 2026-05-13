# ngtool

`ngtool` 是 Nginx-Tools 的 TUI 版本，提供仪表盘、站点管理、证书、日志、服务控制和备份还原。

## 运行

```bash
ngtool
ngtool --readonly
ngtool --version
```

非 root 启动时会自动进入只读模式，可查看但不能执行写操作。

## 站点管理

当前主流程是“托管模式优先”。

### 新建站点

- `反向代理`
  - 流式响应 / AI API
  - WebSocket
  - 大请求体 / 上传
  - 浏览器跨域 CORS
  - 长超时后端
- `静态站点`
  - 普通静态 / SPA 单页
  - 静态资源缓存
  - 敏感路径保护
- `Emby / Jellyfin`
  - 使用内置优化代理预设

### 编辑已有站点

- `托管`：默认入口，只保留关键选项，适合一键维护
- `高级`：维护注入槽和内置模板片段
- `原始`：直接编辑完整 Nginx 配置

模板会写入托管标记，编辑已有站点时会自动恢复对应开关，不会因为再次保存而丢失这些预设。

## 常用键位

全局：

- `1-6` 切主菜单
- `Tab` 切区域
- `Esc` 返回
- `q` / `Ctrl+C` 退出

站点列表：

- `n` 新建
- `Enter` 编辑
- `s` 启用 / 停用
- `d` 删除
- `c` 申请证书
- `l` 查看日志

托管编辑：

- `Tab` / `↑↓` 切项
- `←→` / `Space` / `Enter` 切换当前选项
- `a` 进入高级模式
- `o` 进入原始模式
- `Ctrl+S` 保存并测试
- `Ctrl+W` 仅保存

高级编辑：

- `←→` 切换注入槽
- `↑↓` 选择模板
- `Enter` / `Space` 追加模板
- `Ctrl+R` 用模板替换当前槽位
- `Ctrl+E` 全屏编辑当前槽位
- `a` 返回托管模式
- `o` 进入原始模式

原始编辑：

- `Ctrl+S` 保存并测试
- `Ctrl+W` 仅保存
- `Ctrl+Z` / `Ctrl+Y` 撤销 / 重做
- `o` 返回托管模式

## 数据目录

```text
~/.local/ngtool/
├── config.toml
├── backups/
├── logs/
│   ├── tui.log
│   └── audit.log
├── cache/
└── tmp/
```

## 开发

```bash
cd tui-next
cargo test
cargo run -- --readonly
```

更完整的设计和实现说明见 `doc/`。
