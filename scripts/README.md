# 辅助脚本

## delete-all-certs.sh

删除服务器上所有 certbot 证书的脚本。

### 使用场景

当你需要清理服务器上的所有证书，然后在 TUI 中重新申请时使用。

### 使用方法

1. 将脚本复制到远程服务器：
```bash
scp scripts/delete-all-certs.sh user@your-server:/tmp/
```

2. 在远程服务器上执行：
```bash
ssh user@your-server
sudo bash /tmp/delete-all-certs.sh
```

3. 脚本会：
   - 列出所有现有证书
   - 要求输入 `YES` 确认
   - 逐个删除所有证书

### 注意事项

- ⚠️ 此操作不可撤销
- 删除证书后，站点的 HTTPS 会暂时失效
- 删除后需要立即重新申请证书
- 建议在低流量时段操作

### 删除后重新申请证书

在 TUI 中：
1. 进入"🔐 证书管理"页面
2. 选择站点
3. 按 Tab 切换到操作按钮
4. 选择"申请新证书"
5. 确认申请

或者手动执行：
```bash
sudo certbot --nginx -d your-domain.com
```
