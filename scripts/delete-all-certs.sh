#!/bin/bash
# 删除所有 certbot 证书的脚本
# 用法：sudo bash delete-all-certs.sh

set -e

echo "⚠️  警告：此脚本将删除所有 certbot 证书！"
echo ""
echo "列出当前所有证书："
certbot certificates 2>/dev/null | grep "Certificate Name:" | awk '{print "  - " $3}'
echo ""
read -p "确认删除所有证书？(输入 YES 继续): " confirm

if [ "$confirm" != "YES" ]; then
    echo "已取消"
    exit 0
fi

echo ""
echo "开始删除证书..."

# 获取所有证书名称
cert_names=$(certbot certificates 2>/dev/null | grep "Certificate Name:" | awk '{print $3}')

if [ -z "$cert_names" ]; then
    echo "没有找到任何证书"
    exit 0
fi

# 逐个删除
for cert_name in $cert_names; do
    echo "删除证书: $cert_name"
    certbot delete --cert-name "$cert_name" --non-interactive
done

echo ""
echo "✓ 所有证书已删除"
echo ""
echo "接下来你可以："
echo "1. 在 TUI 中为每个站点重新申请证书"
echo "2. 或者手动执行: sudo certbot --nginx -d your-domain.com"
