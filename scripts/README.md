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

---

## fix-broken-ssl-refs.sh

修复 nginx 配置中引用已删除证书的问题。

### 使用场景

当你删除证书后，nginx 配置中仍然引用已删除的证书路径，导致：
- `nginx -t` 测试失败
- 无法申请新证书
- 站点无法正常工作

### 使用方法

1. 将脚本复制到远程服务器：
```bash
scp scripts/fix-broken-ssl-refs.sh user@your-server:/tmp/
```

2. 在远程服务器上执行：
```bash
ssh user@your-server
sudo bash /tmp/fix-broken-ssl-refs.sh
```

3. 脚本会：
   - 检测 nginx 配置中的证书引用错误
   - 列出引用已删除证书的配置文件
   - 提供修复建议

### 修复步骤

脚本会告诉你哪些配置文件有问题，然后你可以：

**方法 1：手动编辑配置文件**
```bash
sudo nano /etc/nginx/sites-enabled/xxx.conf

# 注释掉 SSL 相关行：
#    listen 443 ssl;
#    ssl_certificate /etc/letsencrypt/live/xxx/fullchain.pem;
#    ssl_certificate_key /etc/letsencrypt/live/xxx/privkey.pem;

# 保存后测试
sudo nginx -t
sudo systemctl reload nginx
```

**方法 2：临时禁用站点**
```bash
sudo rm /etc/nginx/sites-enabled/xxx.conf
sudo systemctl reload nginx
```

**方法 3：使用 TUI 编辑**
1. 在 TUI 中进入"📁 站点管理"
2. 选择站点按 `e` 编辑
3. 注释掉 SSL 相关行
4. 保存退出

---

## 完整的证书重建流程

### 1. 删除所有证书
```bash
scp scripts/delete-all-certs.sh user@your-server:/tmp/
ssh user@your-server
sudo bash /tmp/delete-all-certs.sh
# 输入 YES 确认
```

### 2. 修复 nginx 配置
```bash
scp scripts/fix-broken-ssl-refs.sh user@your-server:/tmp/
sudo bash /tmp/fix-broken-ssl-refs.sh
# 按照提示修复配置文件
```

### 3. 在 TUI 中重新申请证书

1. 进入"🔐 证书管理"页面
2. 用 ↑↓ 选择站点
3. 按 Tab 切换到操作按钮
4. 用 ←→ 选择"申请新证书"
5. 按 Enter 确认
6. 重复以上步骤为每个站点申请

或者手动执行：
```bash
sudo certbot --nginx -d your-domain.com
```

### 4. 验证
```bash
sudo nginx -t
sudo systemctl reload nginx
```

