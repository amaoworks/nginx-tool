# CHANGELOG

## 2026-05-13

- `install.sh` 新增 `--proxy` / `--proxy=URL` 参数，可为 GitHub 仓库拉取、Release API 查询和 TUI 二进制下载添加加速前缀。
- `install.sh` 修复部分 GitHub 加速服务不支持 Release API 时，TUI 安装无法解析最新版本的问题；现在会优先直连 API，失败后再尝试代理与 Release 页面兜底解析。
- `tui-next` 修复站点编辑保存时丢失附加域名的问题，站点列表现在显示全部 `server_name`，申请证书时也会包含附加域名。
- `README.md` 补充 GitHub 加速场景下的安装、模式指定与更新示例。
