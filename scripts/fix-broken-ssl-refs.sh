#!/bin/bash
# 修复 nginx 配置中引用已删除证书的问题
# 用法：sudo bash fix-broken-ssl-refs.sh

set -e

echo "🔍 检查 nginx 配置中的证书引用..."
echo ""

# 测试 nginx 配置
if sudo nginx -t 2>&1 | grep -q "cannot load certificate"; then
    echo "❌ 发现证书引用错误"
    echo ""

    # 提取错误的证书路径
    broken_certs=$(sudo nginx -t 2>&1 | grep "cannot load certificate" | grep -oP '/etc/letsencrypt/live/[^/]+' | sort -u)

    echo "以下证书路径不存在："
    echo "$broken_certs"
    echo ""

    # 查找引用这些证书的配置文件
    echo "查找引用这些证书的配置文件..."
    for cert_path in $broken_certs; do
        cert_name=$(basename "$cert_path")
        echo ""
        echo "证书: $cert_name"
        files=$(sudo grep -rl "$cert_name" /etc/nginx/sites-enabled/ 2>/dev/null || true)

        if [ -n "$files" ]; then
            echo "引用文件："
            echo "$files"
        fi
    done

    echo ""
    echo "修复方法："
    echo "1. 手动编辑上述配置文件，注释掉 SSL 相关行："
    echo "   #    listen 443 ssl;"
    echo "   #    ssl_certificate /etc/letsencrypt/live/xxx/fullchain.pem;"
    echo "   #    ssl_certificate_key /etc/letsencrypt/live/xxx/privkey.pem;"
    echo ""
    echo "2. 或者禁用这些站点："
    for cert_path in $broken_certs; do
        cert_name=$(basename "$cert_path")
        files=$(sudo grep -rl "$cert_name" /etc/nginx/sites-enabled/ 2>/dev/null || true)
        for file in $files; do
            filename=$(basename "$file")
            echo "   sudo rm /etc/nginx/sites-enabled/$filename"
        done
    done
    echo ""
    echo "3. 然后重载 nginx："
    echo "   sudo nginx -t && sudo systemctl reload nginx"
    echo ""
    echo "4. 最后在 TUI 中重新申请证书"

else
    echo "✓ nginx 配置正常，没有发现证书引用错误"
fi
