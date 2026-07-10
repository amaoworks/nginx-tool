# 运维辅助脚本

这些脚本是 **可选运维工具**，不是 TUI 日常流程的一部分。  
用于证书大规模重建、配置引用损坏等少数场景。请在理解风险后再执行。

| 脚本 | 作用 | 危险级别 |
|------|------|----------|
| `fix-broken-ssl-refs.sh` | 检查 nginx 是否引用了已删除的证书路径，并给出修复建议 | 低（默认只诊断） |
| `delete-all-certs.sh` | 删除本机 **全部** certbot 证书 | **极高**（不可撤销） |

> 日常证书申请 / 清理请优先使用 TUI「证书」页。  
> 仅在 TUI 无法处理（例如 nginx -t 已因坏引用失败）时使用本目录脚本。

---

## fix-broken-ssl-refs.sh

### 何时使用

删除证书后，nginx 配置仍引用旧路径，导致：

- `nginx -t` 失败（`cannot load certificate`）
- 无法申请新证书 / 无法重载

### 用法

```bash
sudo bash scripts/fix-broken-ssl-refs.sh
```

脚本会列出问题证书与引用文件，并提示注释 SSL 行、临时禁用站点或用 TUI 编辑。

---

## delete-all-certs.sh

### 何时使用

需要清空本机全部 Let's Encrypt 证书后重新申请（例如域名体系大改）。

### 用法

```bash
sudo bash scripts/delete-all-certs.sh
# 必须输入 YES 才会继续
```

### 注意

- 操作 **不可撤销**
- 删除后站点 HTTPS 会立即失效
- 建议配合 `fix-broken-ssl-refs.sh` 清理坏引用，再在 TUI 中逐站重新申请

---

## 推荐：证书重建三步

1. （可选）`delete-all-certs.sh` 清空证书  
2. `fix-broken-ssl-refs.sh` 检查并按提示修好坏引用  
3. 在 TUI「证书」页为各站点重新申请，然后 `nginx -t && systemctl reload nginx`
