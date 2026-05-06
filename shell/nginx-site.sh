#!/bin/bash

# Nginx 网站管理工具
# 位置: ~/nginx/shell/nginx-site.sh

# 脚本所在目录（用于定位模板文件）
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TEMPLATE_DIR="$SCRIPT_DIR/templates"
CMD_NAME="${0:-nginx-site.sh}"

# ============================================================
# 工具函数
# ============================================================

# 解析代理目标：纯端口 → 127.0.0.1:端口，IP:端口 → 原样，http(s)://... → 去掉协议头
parse_upstream() {
    local input="$1"
    # 去掉可能的 http:// 或 https:// 前缀
    local target="${input#http://}"
    target="${target#https://}"
    # 去掉尾部斜杠
    target="${target%/}"
    # 如果只是纯数字（端口号），加上 127.0.0.1
    if [[ "$target" =~ ^[0-9]+$ ]]; then
        echo "127.0.0.1:$target"
    else
        echo "$target"
    fi
}

# 解析上游主机名：用于 HTTPS 上游的 SNI
parse_upstream_host() {
    local target
    target=$(parse_upstream "$1")

    # IPv6 形式如 [2001:db8::1]:8920
    if [[ "$target" =~ ^\[([0-9a-fA-F:]+)\](:[0-9]+)?$ ]]; then
        echo "${BASH_REMATCH[1]}"
    else
        echo "${target%%:*}"
    fi
}

# 解析协议：从输入中提取 http 或 https，默认 http
parse_scheme() {
    local input="$1"
    if [[ "$input" =~ ^https:// ]]; then
        echo "https"
    else
        echo "http"
    fi
}

# 从模板创建配置文件
create_from_template() {
    local template_file="$1"
    local site_name="$2"
    local domain="$3"
    local upstream="$4"
    local scheme="${5:-http}"
    local upstream_host="${6:-$upstream}"
    local output_file="/etc/nginx/sites-available/$site_name.conf"

    if [ ! -f "$template_file" ]; then
        echo "❌ 模板文件不存在: $template_file"
        return 1
    fi

    sed -e "s|DOMAIN_NAME|$domain|g" \
        -e "s|SITE_NAME|$site_name|g" \
        -e "s|UPSTREAM_SCHEME|$scheme|g" \
        -e "s|UPSTREAM_TARGET|$upstream|g" \
        -e "s|UPSTREAM_HOST|$upstream_host|g" \
        "$template_file" > "$output_file"

    echo "$output_file"
}

# 启用站点（内部使用）
do_enable() {
    local name="$1"
    if [ ! -f "/etc/nginx/sites-available/$name.conf" ]; then
        echo "❌ 配置文件不存在: /etc/nginx/sites-available/$name.conf"
        return 1
    fi
    ln -sf /etc/nginx/sites-available/$name.conf /etc/nginx/sites-enabled/
    if nginx -t 2>/dev/null; then
        systemctl reload nginx
        echo "✓ 已启用 $name"
    else
        echo "⚠️  配置测试失败，回滚..."
        rm -f /etc/nginx/sites-enabled/$name.conf
        nginx -t
        return 1
    fi
}

# ============================================================
# 主命令分发
# ============================================================

case "$1" in
    new)
        # ==================================================
        # 创建新站点（支持交互式 / 非交互式）
        # ==================================================
        
        if [ -n "$2" ] && [ -n "$3" ] && [ -n "$4" ]; then
            # --- 非交互模式: ng new <名称> <域名> <目标> [--type proxy|emby|static] [--enable] [--cert] ---
            SITE_NAME="$2"
            DOMAIN="$3"
            RAW_TARGET="$4"
            SITE_TYPE="proxy"
            DO_ENABLE=false
            DO_CERT=false

            shift 4
            while [ $# -gt 0 ]; do
                case "$1" in
                    --type)  SITE_TYPE="$2"; shift 2 ;;
                    --enable) DO_ENABLE=true; shift ;;
                    --cert)  DO_CERT=true; shift ;;
                    *)       shift ;;
                esac
            done
        else
            # --- 交互模式 ---
            echo "🌐 创建新站点"
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
            echo ""
            
            read -p "📝 站点名称: " SITE_NAME
            if [ -z "$SITE_NAME" ]; then
                echo "❌ 站点名称不能为空"
                exit 1
            fi

            read -p "🌍 域名: " DOMAIN
            if [ -z "$DOMAIN" ]; then
                echo "❌ 域名不能为空"
                exit 1
            fi

            echo ""
            echo "选择站点类型:"
            echo "  1) 反向代理（通用）"
            echo "  2) 反向代理（Emby/Jellyfin）"
            echo "  3) 静态站点"
            read -p "请选择 [1-3，默认 1]: " TYPE_CHOICE
            
            case "$TYPE_CHOICE" in
                2) SITE_TYPE="emby" ;;
                3) SITE_TYPE="static" ;;
                *) SITE_TYPE="proxy" ;;
            esac

            if [ "$SITE_TYPE" != "static" ]; then
                echo ""
                echo "💡 支持以下格式:"
                echo "   端口号:     8080                    → http://127.0.0.1:8080"
                echo "   IP:端口:    192.168.1.5:8096        → http://192.168.1.5:8096"
                echo "   HTTP地址:   http://10.0.0.1:8096"
                echo "   HTTPS地址:  https://10.0.0.1:8920   → HTTPS 反代"
                read -p "🎯 代理目标: " RAW_TARGET
                if [ -z "$RAW_TARGET" ]; then
                    echo "❌ 代理目标不能为空"
                    exit 1
                fi
            else
                RAW_TARGET=""
            fi

            echo ""
            read -p "是否立即启用？(Y/n) " -n 1 -r REPLY_ENABLE
            echo
            [[ ! "$REPLY_ENABLE" =~ ^[Nn]$ ]] && DO_ENABLE=true || DO_ENABLE=false

            read -p "是否申请 SSL 证书？(y/N) " -n 1 -r REPLY_CERT
            echo
            [[ "$REPLY_CERT" =~ ^[Yy]$ ]] && DO_CERT=true || DO_CERT=false
        fi

        # --- 执行创建 ---
        if [ -f "/etc/nginx/sites-available/$SITE_NAME.conf" ]; then
            echo "❌ 配置文件已存在: $SITE_NAME.conf"
            exit 1
        fi

        echo ""
        case "$SITE_TYPE" in
            emby)
                UPSTREAM=$(parse_upstream "$RAW_TARGET")
                SCHEME=$(parse_scheme "$RAW_TARGET")
                UPSTREAM_HOST=$(parse_upstream_host "$RAW_TARGET")
                OUTPUT=$(create_from_template "$TEMPLATE_DIR/emby.template" "$SITE_NAME" "$DOMAIN" "$UPSTREAM" "$SCHEME" "$UPSTREAM_HOST")
                if [ $? -ne 0 ]; then exit 1; fi
                ;;
            static)
                # 静态站点：创建文档根目录
                if [ ! -f "$TEMPLATE_DIR/static.template" ]; then
                    echo "❌ 模板文件不存在: $TEMPLATE_DIR/static.template"
                    exit 1
                fi
                mkdir -p "/var/www/$SITE_NAME"
                sed -e "s|DOMAIN_NAME|$DOMAIN|g" \
                    -e "s|SITE_NAME|$SITE_NAME|g" \
                    "$TEMPLATE_DIR/static.template" > "/etc/nginx/sites-available/$SITE_NAME.conf"
                if [ $? -ne 0 ]; then echo "❌ 模板渲染失败"; exit 1; fi
                OUTPUT="/etc/nginx/sites-available/$SITE_NAME.conf"
                echo "✓ 已创建文档根目录: /var/www/$SITE_NAME"
                ;;
            *)
                UPSTREAM=$(parse_upstream "$RAW_TARGET")
                OUTPUT=$(create_from_template "$TEMPLATE_DIR/proxy.template" "$SITE_NAME" "$DOMAIN" "$UPSTREAM")
                if [ $? -ne 0 ]; then exit 1; fi
                ;;
        esac

        echo "✓ 已创建配置: $OUTPUT"

        # 自动启用
        if [ "$DO_ENABLE" = true ]; then
            echo ""
            do_enable "$SITE_NAME"
        fi

        # 自动申请证书
        if [ "$DO_CERT" = true ]; then
            echo ""
            echo "🔐 申请 SSL 证书..."
            certbot --nginx -d "$DOMAIN"
        fi

        # 提示下一步
        echo ""
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        if [ "$DO_ENABLE" != true ]; then
            echo "📌 下一步: $CMD_NAME enable $SITE_NAME"
        fi
        if [ "$DO_CERT" != true ]; then
            echo "📌 申请证书: $CMD_NAME cert $DOMAIN"
        fi
        ;;

    enable)
        if [ -z "$2" ]; then
            echo "❌ 请指定站点名"
            echo "用法: $CMD_NAME enable <站点名>"
            exit 1
        fi
        do_enable "$2"
        ;;
        
    disable)
        if [ -z "$2" ]; then
            echo "❌ 请指定站点名"
            echo "用法: $CMD_NAME disable <站点名>"
            exit 1
        fi
        rm -f /etc/nginx/sites-enabled/$2.conf
        nginx -t && systemctl reload nginx
        echo "✓ 已禁用 $2"
        ;;

    delete)
        if [ -z "$2" ]; then
            echo "❌ 请指定站点名"
            echo "用法: $CMD_NAME delete <站点名>"
            exit 1
        fi

        CONF_FILE="/etc/nginx/sites-available/$2.conf"
        if [ ! -f "$CONF_FILE" ]; then
            echo "❌ 配置文件不存在: $CONF_FILE"
            exit 1
        fi

        echo "⚠️  即将删除站点: $2"
        echo "   配置文件: $CONF_FILE"
        read -p "确认删除？(y/N) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            rm -f /etc/nginx/sites-enabled/$2.conf
            rm -f "$CONF_FILE"
            nginx -t && systemctl reload nginx
            echo "✓ 已删除 $2"
        else
            echo "已取消"
        fi
        ;;
        
    list)
        echo ""
        echo "📁 可用网站"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        
        found=false
        for conf in /etc/nginx/sites-available/*.conf; do
            if [ -f "$conf" ]; then
                found=true
                name=$(basename "$conf" .conf)
                domain=$(grep -oP 'server_name\s+\K[^;]+' "$conf" 2>/dev/null | head -1)
                proxy=$(grep -oP 'proxy_pass\s+\K[^;]+' "$conf" 2>/dev/null | head -1)
                root_dir=$(grep -oP '^\s*root\s+\K[^;]+' "$conf" 2>/dev/null | head -1)

                if [ -L "/etc/nginx/sites-enabled/$name.conf" ]; then
                    echo "  ✓ $name (已启用)"
                else
                    echo "  ○ $name (未启用)"
                fi

                if [ -n "$domain" ]; then
                    if [ -n "$proxy" ]; then
                        echo "    └─ $domain → $proxy"
                    elif [ -n "$root_dir" ]; then
                        echo "    └─ $domain → $root_dir"
                    else
                        echo "    └─ $domain"
                    fi
                fi
            fi
        done
        
        if [ "$found" = false ]; then
            echo "  （无站点配置）"
        fi
        echo ""
        ;;
        
    test)
        nginx -t
        ;;
        
    reload)
        echo "🔄 测试并重载配置..."
        nginx -t && systemctl reload nginx
        echo "✓ 配置已重载"
        ;;
        
    restart)
        echo "🔄 重启 Nginx..."
        systemctl restart nginx
        echo "✓ Nginx 已重启"
        ;;
        
    status)
        systemctl status nginx
        ;;

    logs)
        if [ -z "$2" ]; then
            echo "📄 查看所有访问日志（Ctrl+C 退出）："
            tail -f /var/log/nginx/*.access.log
        else
            LOG_FILE="/var/log/nginx/$2.access.log"
            if [ ! -f "$LOG_FILE" ]; then
                echo "❌ 日志不存在: $LOG_FILE"
                exit 1
            fi
            echo "📄 查看 $2 访问日志（Ctrl+C 退出）："
            tail -f "$LOG_FILE"
        fi
        ;;
        
    errors)
        if [ -z "$2" ]; then
            echo "⚠️  查看所有站点错误日志（Ctrl+C 退出）："
            tail -f /var/log/nginx/*.error.log
        else
            LOG_FILE="/var/log/nginx/$2.error.log"
            if [ ! -f "$LOG_FILE" ]; then
                echo "❌ 错误日志不存在: $LOG_FILE"
                exit 1
            fi
            echo "⚠️  查看 $2 错误日志（Ctrl+C 退出）："
            tail -f "$LOG_FILE"
        fi
        ;;
        
    cert)
        if [ -z "$2" ]; then
            echo "❌ 请指定域名"
            echo "用法: $CMD_NAME cert <域名>"
            exit 1
        fi
        certbot --nginx -d "$2"
        ;;
        
    renew)
        echo "🔐 续期所有 SSL 证书..."
        certbot renew
        echo "✓ 证书续期完成"
        ;;

    auto-renew)
        echo "🔐 配置证书自动续签"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

        # 检查 certbot 是否安装
        if ! command -v certbot &>/dev/null; then
            echo "❌ 未安装 certbot"
            echo "   安装: sudo apt install certbot python3-certbot-nginx -y"
            exit 1
        fi

        # 检查并启用 systemd timer
        if systemctl list-unit-files certbot.timer &>/dev/null; then
            if systemctl is-enabled --quiet certbot.timer 2>/dev/null; then
                echo "✓ certbot.timer 已启用"
            else
                systemctl enable --now certbot.timer
                echo "✓ 已启用 certbot.timer"
            fi
            echo "  下次执行: $(systemctl list-timers certbot.timer --no-pager | grep certbot | awk '{print $1, $2, $3}')"
        else
            # 没有 systemd timer，创建 cron job
            echo "⚠️  未找到 certbot.timer，创建 cron 任务..."
            CRON_CMD="0 3 * * * certbot renew --quiet --deploy-hook 'systemctl reload nginx'"
            if ! crontab -l 2>/dev/null | grep -q "certbot renew"; then
                (crontab -l 2>/dev/null; echo "$CRON_CMD") | crontab -
                echo "✓ 已添加 cron 任务（每天凌晨 3 点检查续签）"
            else
                echo "✓ cron 续签任务已存在"
            fi
        fi

        # 设置 deploy hook（续签成功后自动 reload nginx）
        HOOK_DIR="/etc/letsencrypt/renewal-hooks/deploy"
        HOOK_FILE="$HOOK_DIR/reload-nginx.sh"
        if [ ! -f "$HOOK_FILE" ]; then
            mkdir -p "$HOOK_DIR"
            cat > "$HOOK_FILE" <<'HOOKEOF'
#!/bin/bash
# 证书续签成功后自动重载 Nginx
systemctl reload nginx
HOOKEOF
            chmod +x "$HOOK_FILE"
            echo "✓ 已创建 deploy hook: $HOOK_FILE"
        else
            echo "✓ deploy hook 已存在"
        fi

        echo ""
        echo "✓ 自动续签配置完成"
        echo "  测试续签: sudo certbot renew --dry-run"
        ;;
        
    edit)
        if [ -z "$2" ]; then
            echo "❌ 请指定站点名"
            echo "用法: $CMD_NAME edit <站点名>"
            exit 1
        fi
        
        if [ ! -f "/etc/nginx/sites-available/$2.conf" ]; then
            echo "❌ 配置文件不存在: /etc/nginx/sites-available/$2.conf"
            exit 1
        fi
        
        nano /etc/nginx/sites-available/$2.conf
        echo ""
        read -p "是否测试并重载配置？(y/n) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            nginx -t && systemctl reload nginx
        fi
        ;;
        
    backup)
        BACKUP_DIR=~/nginx-backups
        DATE=$(date +%Y%m%d-%H%M%S)
        
        mkdir -p "$BACKUP_DIR"
        
        tar -czf "$BACKUP_DIR/nginx-config-$DATE.tar.gz" \
            /etc/nginx/sites-available/ \
            /etc/nginx/snippets/ \
            /etc/nginx/nginx.conf 2>/dev/null
        
        echo "✓ 已备份到: $BACKUP_DIR/nginx-config-$DATE.tar.gz"
        
        # 只保留最近 10 个备份
        ls -t "$BACKUP_DIR"/*.tar.gz 2>/dev/null | tail -n +11 | xargs -r rm -f 2>/dev/null
        ;;

    restore)
        BACKUP_DIR=~/nginx-backups

        if [ ! -d "$BACKUP_DIR" ] || ! ls "$BACKUP_DIR"/*.tar.gz >/dev/null 2>&1; then
            echo "❌ 没有找到任何备份文件"
            echo "   备份目录: $BACKUP_DIR"
            exit 1
        fi

        echo "📦 可用备份:"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        
        # 列出备份文件
        mapfile -t backups < <(ls -t "$BACKUP_DIR"/*.tar.gz 2>/dev/null)
        for i in "${!backups[@]}"; do
            file=$(basename "${backups[$i]}")
            size=$(du -h "${backups[$i]}" | awk '{print $1}')
            echo "  $((i+1))) $file ($size)"
        done

        echo ""
        read -p "选择要还原的备份 [1-${#backups[@]}]: " choice

        if ! [[ "$choice" =~ ^[0-9]+$ ]] || [ "$choice" -lt 1 ] || [ "$choice" -gt "${#backups[@]}" ]; then
            echo "❌ 无效选择"
            exit 1
        fi

        selected="${backups[$((choice-1))]}"
        echo ""
        echo "⚠️  即将还原: $(basename "$selected")"
        echo "   这将覆盖当前 Nginx 配置！"
        read -p "确认还原？(y/N) " -n 1 -r
        echo

        if [[ $REPLY =~ ^[Yy]$ ]]; then
            # 先备份当前配置
            PRE_RESTORE_DATE=$(date +%Y%m%d-%H%M%S)
            tar -czf "$BACKUP_DIR/pre-restore-$PRE_RESTORE_DATE.tar.gz" \
                /etc/nginx/sites-available/ \
                /etc/nginx/snippets/ \
                /etc/nginx/nginx.conf 2>/dev/null
            echo "✓ 已备份当前配置: pre-restore-$PRE_RESTORE_DATE.tar.gz"

            # 还原
            tar -xzf "$selected" -C /
            
            # 测试配置
            if nginx -t 2>/dev/null; then
                systemctl reload nginx
                echo "✓ 还原成功，Nginx 已重载"
            else
                echo "⚠️  还原后配置测试失败！"
                echo "   当前配置可能有问题，请检查："
                nginx -t
                echo ""
                echo "   还原前的备份: $BACKUP_DIR/pre-restore-$PRE_RESTORE_DATE.tar.gz"
            fi
        else
            echo "已取消"
        fi
        ;;
        
    *)
        cat <<EOF
🌐 Nginx 网站管理工具
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

📋 站点管理:
  new                             交互式创建新站点
  new <名称> <域名> <目标> [选项]  非交互式创建
      --type proxy|emby|static    指定站点类型（默认 proxy）
      --enable                    创建后立即启用
      --cert                      创建后申请 SSL 证书
  enable <站点名>                  启用站点
  disable <站点名>                 禁用站点
  delete <站点名>                  删除站点（含确认）
  edit <站点名>                    编辑站点配置
  list                            列出所有站点

🔐 证书管理:
  cert <域名>                     申请 SSL 证书
  renew                           续期所有证书
  auto-renew                      配置证书自动续签

⚙️  系统操作:
  test                            测试 Nginx 配置
  reload                          重载配置
  restart                         重启 Nginx
  status                          查看 Nginx 状态

📊 日志:
  logs [站点名]                   查看访问日志
  errors [站点名]                 查看错误日志

💾 备份还原:
  backup                          备份所有配置
  restore                         从备份还原配置

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
示例:
  $CMD_NAME new                                        # 交互式创建
  $CMD_NAME new myapp app.com 8080 --enable --cert     # 一键创建+启用+证书
  $CMD_NAME new emby emby.com 192.168.1.5:8096 --type emby --enable
  $CMD_NAME list                                       # 查看所有站点
  $CMD_NAME delete myapp                               # 删除站点
  $CMD_NAME backup                                     # 备份配置
  $CMD_NAME restore                                    # 还原配置
  $CMD_NAME auto-renew                                 # 配置自动续签
EOF
        exit 1
        ;;
esac
