# CHANGELOG

## 2026-05-13

- `install.sh` 新增 `--proxy` / `--proxy=URL` 参数，可为 GitHub 仓库拉取、Release API 查询和 TUI 二进制下载添加加速前缀。
- `install.sh` 修复部分 GitHub 加速服务不支持 Release API 时，TUI 安装无法解析最新版本的问题；现在会优先直连 API，失败后再尝试代理与 Release 页面兜底解析。
- `tui-next` 修复站点编辑保存时丢失附加域名的问题，站点列表现在显示全部 `server_name`，申请证书时也会包含附加域名。
- `README.md` 精简内容，去除冗余细节，保持核心信息；新增致谢 Linux.do 社区与 @sixsixsix。
- 创建 `LICENSE` 文件（MIT 协议），完善开源协议相关文件。
- `AGENTS.md` 补充技术栈、项目结构、构建测试、代码规范、发布流程、Git 工作流等细节。
- `tui-next/.gitignore` 迁移至项目根目录 `.gitignore`，覆盖所有 Rust 产物。
- `tui-next/doc/changelog.md` 迁移至 `tui-next/CHANGELOG.md`，整理 TUI 模块文件结构。
