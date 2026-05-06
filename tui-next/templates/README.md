# Nginx-Tools TUI 模板说明

本目录的 `*.j2` 模板由 `template::renderer` 通过 `include_str!` 在编译期嵌入二进制，
运行时不读取磁盘。模板内容必须包含三段标记（详见 architecture.md §12.2）：

- `# nginx-tools:custom-before-location:start/end`
- `# nginx-tools:custom-inside-location:start/end`
- `# nginx-tools:custom-after-location:start/end`

仓库根目录的 `templates/*.template` 是 Bash 版工具的资源，二者并行至 Bash 版退役。
