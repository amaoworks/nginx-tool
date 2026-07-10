# Nginx-Tools

轻量的 Nginx 站点管理工具，提供两套入口：

- `ng` / `ngmon`：Shell 版
- `ngtool`：TUI 版

适用于 Debian / Ubuntu 风格的 Nginx 环境，默认目录为 `/etc/nginx/sites-available` 和 `/etc/nginx/sites-enabled`。

## 功能

- 站点创建、启用、停用、编辑、删除
- SSL 证书申请与续期
- Nginx 配置测试、重载、重启
- 访问 / 错误日志查看
- 配置备份与还原
- TUI 仪表盘、只读模式、审计日志

## 快速安装

```bash
# 交互安装 / 更新 / 卸载
bash <(curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh)

# 仅安装 Shell 版
bash <(curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh) shell

# 仅安装 TUI 版
bash <(curl -fsSL https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh) tui
```

国内用户拉取

```bash
# 交互安装 / 更新 / 卸载
bash <(curl -fsSL https://ghfast.top/https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh) --proxy https://ghfast.top

# 仅安装 Shell 版
bash <(curl -fsSL https://ghfast.top/https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh) --proxy https://ghfast.top shell

# 仅安装 TUI 版
bash <(curl -fsSL https://ghfast.top/https://raw.githubusercontent.com/amaoworks/nginx-tool/main/install.sh) --proxy https://ghfast.top tui
```

安装后：

- Shell：使用 `ng`、`ngmon`
- TUI：使用 `ngtool`

## 常用命令

```bash
ngtool
ngtool --readonly
ngtool --version

ng list
ng new my-api api.example.com 127.0.0.1:8080 --type proxy --enable
ng edit my-api
ng logs my-api
ng test
ng reload
```

## TUI 用法

TUI 里的站点管理已经改成“托管模式优先”：

- 新建 `反向代理` 时，直接选择关键开关：
  - `流式响应 / AI API`
  - `WebSocket`
  - `大请求体 / 上传`
  - `浏览器跨域 CORS`
  - `长超时后端`
- 新建 `静态站点` 时，直接选择：
  - `普通静态 / SPA 单页`
  - `静态资源缓存`
  - `敏感路径保护`
- 已有站点编辑默认进入 `托管` 模式
- 复杂定制再进入 `高级` 注入槽或 `原始` 配置模式

常用键位：

- 列表页：`↑↓` 选择，`Enter` 编辑，`s` 启用/停用，`o` 排序字段，`p` 升/降序，`n` 新建，`d` 删除
- 日志页：`/` 搜索，`n` / `N` 下一个/上一个匹配，`Space` 跟随尾部
- 托管编辑：`Tab` / `Shift+Tab` / `↑↓` 切项，`←→` / `Space` / `Enter` 切选项
- 保存统一使用 `F2` 保存、`F3` 保存并测试
- 模式切换统一使用 `F5` / `F6`，高级槽位编辑使用 `F7` / `F8`
- 原始编辑与槽位全屏编辑使用 `F9` / `F10` 撤销 / 重做
- 文本输入中的字符键只写入文本，不触发页面动作

详细说明见 [tui-next/README.md](tui-next/README.md)。

## Shell 模板

- `proxy.template`：通用反向代理
- `emby.template`：Emby / Jellyfin 优化代理
- `static.template`：静态站点

## 更新与卸载

```bash
sudo bash ~/nginx/install.sh status
sudo bash ~/nginx/install.sh update
sudo bash ~/nginx/install.sh uninstall
```

## 运维脚本（可选）

仓库 `scripts/` 提供少数高风险运维脚本（如修复坏掉的证书引用、清空全部证书），**不是日常功能入口**。  
说明与风险见 [scripts/README.md](scripts/README.md)。

## 注意

- 写操作通常需要 root 或 sudo
- 申请证书前，域名需先解析到服务器，且 80 端口可访问
- TUI 预编译二进制目前仅提供 Linux `amd64 / arm64`

## 致谢
- ❤️ 感谢 [**Linux.do**](https://linux.do) 社区提供的交流与分享平台
- 🙏 特别感谢 **@sixsixsix** 的 Shell 脚本思路启发，原帖：[https://linux.do/t/topic/1486160](https://linux.do/t/topic/1486160)

## License

[MIT](LICENSE)
