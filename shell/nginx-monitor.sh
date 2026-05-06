#!/bin/bash

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🌐 Nginx 状态监控"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# 检查Nginx是否运行
if systemctl is-active --quiet nginx; then
    echo "✓ Nginx: 运行中"
else
    echo "✗ Nginx: 已停止！"
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📁 启用的网站"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

for conf in /etc/nginx/sites-enabled/*.conf; do
    if [ -f "$conf" ]; then
        name=$(basename "$conf" .conf)
        domain=$(grep -oP 'server_name\s+\K[^;]+' "$conf" 2>/dev/null | head -1)
        echo "  ✓ $name"
        echo "    └─ $domain"
    fi
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🔐 SSL证书状态"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if command -v certbot &> /dev/null; then
    cert_info=$(certbot certificates 2>/dev/null)
    if [ -n "$cert_info" ]; then
        echo "$cert_info" | grep -E "Certificate Name|Domains|Expiry Date" | sed 's/^/  /'
    else
        echo "  暂无证书"
    fi
else
    echo "  未安装 certbot"
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📊 系统资源"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

echo "  磁盘: $(df -h / | tail -1 | awk '{print $3 "/" $2 " (" $5 ")"}')"
echo "  内存: $(free -h | grep Mem | awk '{print $3 "/" $2}')"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "⚠️  最近错误 (最后3条)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [ -f /var/log/nginx/error.log ]; then
    error_count=$(wc -l < /var/log/nginx/error.log)
    if [ "$error_count" -gt 0 ]; then
        tail -3 /var/log/nginx/error.log | sed 's/^/  /'
    else
        echo "  无错误日志"
    fi
else
    echo "  无错误日志文件"
fi

echo ""