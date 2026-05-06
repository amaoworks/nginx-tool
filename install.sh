#!/bin/bash

# ============================================================
# Nginx-Tools 一键安装 / 卸载脚本
# 支持: Debian / Ubuntu  (x86_64 / aarch64)
# 用法:
#   bash install.sh                # 交互式选择 shell / tui
#   bash install.sh shell          # 安装 Shell 版（Bash 脚本）
#   bash install.sh tui            # 安装 TUI 版（ngtool 二进制）
#   bash install.sh uninstall      # 卸载（自动检测已安装组件）
#
#   # 远程执行
#   curl -fsSL <URL> | bash                      # 交互安装
#   curl -fsSL <URL> | bash -s -- shell          # 远程安装 Shell
#   curl -fsSL <URL> | bash -s -- tui            # 远程安装 TUI
#   curl -fsSL <URL> | bash -s -- uninstall      # 远程卸载
# ============================================================

set -e

# 获取真实用户信息（兼容 sudo）
if [ -n "$SUDO_USER" ]; then
    REAL_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
    REAL_SHELL=$(getent passwd "$SUDO_USER" | cut -d: -f7)
else
    REAL_HOME="$HOME"
    REAL_SHELL="$SHELL"
fi

REPO_URL="https://git.liyka.qzz.io/Brown/Nginx-Tools.git"
RELEASE_API="https://git.liyka.qzz.io/api/v1/repos/Brown/Nginx-Tools/releases/latest"
RELEASE_PAGE="https://git.liyka.qzz.io/Brown/Nginx-Tools/releases/latest"
INSTALL_DIR="$REAL_HOME/nginx"
TUI_BIN_PATH="/usr/local/bin/ngtool"

# ============================================================
# 颜色与输出
# ============================================================

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

info()    { echo -e "  ${BLUE}ℹ${NC}  $1"; }
success() { echo -e "  ${GREEN}✓${NC}  $1"; }
warn()    { echo -e "  ${YELLOW}⚠${NC}  $1"; }
error()   { echo -e "  ${RED}✗${NC}  $1"; }

header() {
    echo ""
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BOLD}  🌐 Nginx-Tools 安装管理${NC}"
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
}

# 兼容 curl | bash 的交互式读取（stdin 被管道占用时从 /dev/tty 读取）
ask() {
    local prompt="$1"
    local var_name="$2"
    if [ -t 0 ]; then
        read -p "$prompt" -n 1 -r "$var_name"
    else
        read -p "$prompt" -n 1 -r "$var_name" </dev/tty
    fi
    echo
}

# 读取整行（用于选择 1/2 这样的输入）
ask_line() {
    local prompt="$1"
    local var_name="$2"
    if [ -t 0 ]; then
        read -p "$prompt" -r "$var_name"
    else
        read -p "$prompt" -r "$var_name" </dev/tty
    fi
}

# ============================================================
# 环境检测
# ============================================================

# 检查 root 权限
check_root() {
    if [ "$(id -u)" -ne 0 ]; then
        error "请使用 root 用户运行此脚本"
        echo -e "     sudo bash install.sh"
        exit 1
    fi
    success "权限检查通过 (root)"
}

# 检测操作系统发行版
detect_distro() {
    if [ ! -f /etc/os-release ]; then
        error "无法识别操作系统（缺少 /etc/os-release）"
        exit 1
    fi

    . /etc/os-release
    DISTRO_ID="$ID"
    DISTRO_NAME="$PRETTY_NAME"

    case "$DISTRO_ID" in
        debian|ubuntu)
            success "操作系统: $DISTRO_NAME"
            ;;
        *)
            error "不支持的操作系统: $DISTRO_NAME"
            echo -e "     目前仅支持 ${BOLD}Debian${NC} / ${BOLD}Ubuntu${NC}"
            exit 1
            ;;
    esac
}

# 检测系统架构（仅 TUI 需要）
detect_arch() {
    local m
    m=$(uname -m)
    case "$m" in
        x86_64|amd64)
            ARCH="amd64"
            ;;
        aarch64|arm64)
            ARCH="arm64"
            ;;
        *)
            error "不支持的系统架构: $m"
            echo -e "     TUI 版仅提供 ${BOLD}amd64${NC} / ${BOLD}arm64${NC} 预编译二进制"
            echo -e "     ${DIM}如需其他架构请使用 shell 模式或自行从源码构建${NC}"
            exit 1
            ;;
    esac
    success "系统架构: $(uname -m) → linux-${ARCH}"
}

# 检测用户的登录 Shell
detect_shell() {
    local login_shell
    login_shell=$(basename "$REAL_SHELL")

    case "$login_shell" in
        bash)
            USER_SHELL="bash"
            SHELL_NAME="Bash"
            SHELL_CONFIG="$REAL_HOME/.bashrc"
            ;;
        zsh)
            USER_SHELL="zsh"
            SHELL_NAME="Zsh"
            SHELL_CONFIG="$REAL_HOME/.zshrc"
            ;;
        fish)
            USER_SHELL="fish"
            SHELL_NAME="Fish"
            SHELL_CONFIG="$REAL_HOME/.config/fish/config.fish"
            ;;
        *)
            USER_SHELL="bash"
            SHELL_NAME="$login_shell (未适配，回退到 Bash)"
            SHELL_CONFIG="$REAL_HOME/.bashrc"
            ;;
    esac

    success "登录终端: ${SHELL_NAME} → ${DIM}${SHELL_CONFIG}${NC}"
}

# ============================================================
# 依赖检查与安装
# ============================================================

# 检查 / 安装 Nginx
check_nginx() {
    if command -v nginx &>/dev/null; then
        local ver
        ver=$(nginx -v 2>&1 | grep -oP 'nginx/\K[\d.]+' || echo "未知")
        success "Nginx 已安装 ${DIM}(v${ver})${NC}"
    else
        warn "Nginx 未安装"
        ask "     是否现在安装 Nginx？(Y/n) " REPLY_NGINX
        if [[ ! "$REPLY_NGINX" =~ ^[Nn]$ ]]; then
            info "正在安装 Nginx..."
            apt-get update -qq >/dev/null 2>&1
            DEBIAN_FRONTEND=noninteractive apt-get install -y -qq nginx >/dev/null 2>&1 || { error "Nginx 安装失败"; exit 1; }
            success "Nginx 安装完成"

            # 确保 sites-available / sites-enabled 目录存在
            mkdir -p /etc/nginx/sites-available /etc/nginx/sites-enabled
        else
            warn "跳过 Nginx 安装（部分功能将不可用）"
        fi
    fi

    # 检查目录结构
    if [ -d /etc/nginx ] && { [ ! -d /etc/nginx/sites-available ] || [ ! -d /etc/nginx/sites-enabled ]; }; then
        mkdir -p /etc/nginx/sites-available /etc/nginx/sites-enabled
        info "已创建 sites-available / sites-enabled 目录"
    fi
}

# 检查 / 安装 Certbot
check_certbot() {
    if command -v certbot &>/dev/null; then
        success "Certbot 已安装"
    else
        warn "Certbot 未安装 ${DIM}(SSL 证书功能需要)${NC}"
        ask "     是否现在安装 Certbot？(Y/n) " REPLY_CERT
        if [[ ! "$REPLY_CERT" =~ ^[Nn]$ ]]; then
            info "正在安装 Certbot..."
            DEBIAN_FRONTEND=noninteractive apt-get install -y -qq certbot python3-certbot-nginx >/dev/null 2>&1 || { error "Certbot 安装失败"; exit 1; }
            success "Certbot 安装完成"
        else
            info "跳过（后续可手动安装: apt install certbot python3-certbot-nginx）"
        fi
    fi
}

# 检查 / 安装 Git
check_git() {
    if command -v git &>/dev/null; then
        return 0
    fi
    info "正在安装 Git..."
    apt-get update -qq >/dev/null 2>&1
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq git >/dev/null 2>&1 || { error "Git 安装失败"; exit 1; }
    success "Git 安装完成"
}

# 检查 / 安装 curl
check_curl() {
    if command -v curl &>/dev/null; then
        return 0
    fi
    info "正在安装 curl..."
    apt-get update -qq >/dev/null 2>&1
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq curl >/dev/null 2>&1 || { error "curl 安装失败"; exit 1; }
    success "curl 安装完成"
}

# ============================================================
# 别名管理（仅 Shell 模式使用）
# ============================================================

ALIAS_MARKER="# >>> Nginx-Tools >>>"
ALIAS_END="# <<< Nginx-Tools <<<"

# 写入别名到 Shell 配置文件
add_aliases() {
    # 确保配置文件所在目录存在（主要针对 Fish）
    mkdir -p "$(dirname "$SHELL_CONFIG")"
    touch "$SHELL_CONFIG"

    # 已存在则跳过
    if grep -qF "$ALIAS_MARKER" "$SHELL_CONFIG" 2>/dev/null; then
        info "别名已存在，跳过写入"
        return 0
    fi

    if [ "$USER_SHELL" = "fish" ]; then
        cat >> "$SHELL_CONFIG" <<EOF

$ALIAS_MARKER
alias ng '$INSTALL_DIR/shell/nginx-site.sh'
alias ngmon '$INSTALL_DIR/shell/nginx-monitor.sh'
$ALIAS_END
EOF
    else
        cat >> "$SHELL_CONFIG" <<EOF

$ALIAS_MARKER
alias ng='$INSTALL_DIR/shell/nginx-site.sh'
alias ngmon='$INSTALL_DIR/shell/nginx-monitor.sh'
$ALIAS_END
EOF
    fi

    success "已写入别名 → ${DIM}${SHELL_CONFIG}${NC}"
}

# 从 Shell 配置文件移除别名
remove_aliases() {
    if [ ! -f "$SHELL_CONFIG" ]; then
        info "配置文件不存在: $SHELL_CONFIG"
        return 0
    fi

    if ! grep -qF "$ALIAS_MARKER" "$SHELL_CONFIG" 2>/dev/null; then
        info "未找到别名配置"
        return 0
    fi

    # 删除标记块（包含起止标记之间的所有行）
    sed -i "/$ALIAS_MARKER/,/$ALIAS_END/d" "$SHELL_CONFIG"

    # 清理末尾多余空行
    sed -i -e :a -e '/^\n*$/{$d;N;ba' -e '}' "$SHELL_CONFIG"

    success "已移除别名 ← ${DIM}${SHELL_CONFIG}${NC}"
}

# ============================================================
# 安装模式选择
# ============================================================

choose_mode() {
    # 已通过命令行指定时跳过
    if [ -n "${MODE:-}" ]; then
        return 0
    fi

    echo ""
    echo -e "${BOLD}  🎛️  请选择安装模式${NC}"
    echo ""
    echo -e "    ${CYAN}1)${NC} ${BOLD}Shell 版${NC}  ${DIM}— Bash 脚本，命令行参数式（ng / ngmon）${NC}"
    echo -e "    ${CYAN}2)${NC} ${BOLD}TUI 版${NC}    ${DIM}— 全屏交互式终端界面（ngtool 二进制）${NC}"
    echo ""

    local choice
    while true; do
        ask_line "  输入序号 [1/2]（默认 1）: " choice
        choice="${choice:-1}"
        case "$choice" in
            1|shell|s|S) MODE="shell"; break ;;
            2|tui|t|T)   MODE="tui";   break ;;
            *) warn "无效输入: $choice，请重试" ;;
        esac
    done

    success "已选择: ${BOLD}${MODE}${NC} 模式"
}

# ============================================================
# Shell 版安装
# ============================================================

install_shell() {
    header
    echo -e "${BOLD}  📥 环境检测（Shell 版）${NC}"
    echo ""

    check_root
    detect_distro
    detect_shell

    echo ""
    echo -e "${BOLD}  📋 依赖检查${NC}"
    echo ""

    check_git
    check_nginx
    check_certbot

    echo ""
    echo -e "${BOLD}  📦 拉取仓库${NC}"
    echo ""

    # 克隆或更新仓库
    if [ -d "$INSTALL_DIR/.git" ]; then
        info "检测到已有安装，正在更新..."
        cd "$INSTALL_DIR"
        git pull --ff-only 2>/dev/null || git pull --rebase 2>/dev/null || { error "仓库更新失败，请检查网络连接"; exit 1; }
        success "已更新到最新版本"
    else
        if [ -d "$INSTALL_DIR" ]; then
            warn "$INSTALL_DIR 已存在但不是 Nginx-Tools 仓库"
            ask "     是否删除并重新安装？(y/N) " REPLY_OVERWRITE
            if [[ "$REPLY_OVERWRITE" =~ ^[Yy]$ ]]; then
                rm -rf "$INSTALL_DIR"
            else
                error "安装已取消"
                exit 1
            fi
        fi
        git clone "$REPO_URL" "$INSTALL_DIR" 2>/dev/null || { error "仓库克隆失败，请检查网络连接"; exit 1; }
        success "已克隆到 $INSTALL_DIR"
    fi

    # 设置执行权限
    if [ -f "$INSTALL_DIR/shell/nginx-site.sh" ] && [ -f "$INSTALL_DIR/shell/nginx-monitor.sh" ]; then
        chmod +x "$INSTALL_DIR/shell/nginx-site.sh" "$INSTALL_DIR/shell/nginx-monitor.sh"
        success "已设置脚本执行权限"
    else
        error "未找到 shell 脚本（$INSTALL_DIR/shell/）"
        exit 1
    fi

    # 添加别名
    add_aliases

    # 完成
    echo ""
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}${BOLD}  ✅ Shell 版安装完成！${NC}"
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    echo "  使别名生效（二选一）："
    echo -e "    ${CYAN}source $SHELL_CONFIG${NC}"
    echo "    或重新打开终端"
    echo ""
    echo "  快速使用："
    echo -e "    ${CYAN}ng${NC}        站点管理（输入 ng 查看帮助）"
    echo -e "    ${CYAN}ngmon${NC}     状态监控面板"
    echo ""
}

# ============================================================
# TUI 版安装
# ============================================================

# 解析最新 release 的下载 URL
resolve_tui_asset() {
    local asset="ngtool-*-linux-${ARCH}"
    local url=""

    # 优先调用 Forgejo / Gitea 兼容 API
    if command -v curl &>/dev/null; then
        local json
        json=$(curl -fsSL "$RELEASE_API" 2>/dev/null || true)
        if [ -n "$json" ]; then
            # 用 grep + sed 解析（不依赖 jq）
            url=$(printf '%s' "$json" \
                | tr ',' '\n' \
                | grep -oE '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*ngtool-[^"]*-linux-'"${ARCH}"'"' \
                | head -n1 \
                | sed -E 's/.*"(https?:[^"]*)".*/\1/')
            ASSET_VERSION=$(printf '%s' "$json" \
                | tr ',' '\n' \
                | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' \
                | head -n1 \
                | sed -E 's/.*"([^"]*)"$/\1/')
        fi
    fi

    if [ -z "$url" ]; then
        error "无法解析最新发布版本（API 调用失败）"
        echo -e "     请手动访问: ${CYAN}${RELEASE_PAGE}${NC}"
        exit 1
    fi

    TUI_DOWNLOAD_URL="$url"
    TUI_ASSET_NAME=$(basename "$url")
    success "最新版本: ${BOLD}${ASSET_VERSION:-unknown}${NC} → ${DIM}${TUI_ASSET_NAME}${NC}"
}

install_tui() {
    header
    echo -e "${BOLD}  📥 环境检测（TUI 版）${NC}"
    echo ""

    check_root
    detect_distro
    detect_arch

    echo ""
    echo -e "${BOLD}  📋 依赖检查${NC}"
    echo ""

    check_curl
    check_nginx
    check_certbot

    echo ""
    echo -e "${BOLD}  📦 下载 TUI 二进制${NC}"
    echo ""

    resolve_tui_asset

    # 已存在时提示版本
    if [ -x "$TUI_BIN_PATH" ]; then
        local cur_ver
        cur_ver=$("$TUI_BIN_PATH" --version 2>/dev/null || echo "未知")
        info "已存在: ${TUI_BIN_PATH} ${DIM}(${cur_ver})${NC}"
    fi

    info "下载: $TUI_DOWNLOAD_URL"
    local tmp
    tmp=$(mktemp)
    if ! curl -fsSL --progress-bar -o "$tmp" "$TUI_DOWNLOAD_URL"; then
        rm -f "$tmp"
        error "下载失败，请检查网络连接"
        exit 1
    fi

    # 简单校验：必须是 ELF 文件
    if ! head -c 4 "$tmp" | grep -q $'\x7fELF'; then
        rm -f "$tmp"
        error "下载的文件不是有效的 ELF 二进制"
        exit 1
    fi

    install -m 0755 "$tmp" "$TUI_BIN_PATH"
    rm -f "$tmp"
    success "已安装到 ${BOLD}${TUI_BIN_PATH}${NC}"

    # 验证可执行
    if "$TUI_BIN_PATH" --version >/dev/null 2>&1; then
        success "二进制运行正常"
    else
        warn "二进制安装成功，但 --version 调用失败（可能版本较旧）"
    fi

    # 完成
    echo ""
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}${BOLD}  ✅ TUI 版安装完成！${NC}"
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    echo "  快速使用："
    echo -e "    ${CYAN}ngtool${NC}            启动 TUI 主界面"
    echo -e "    ${CYAN}ngtool --help${NC}     查看命令行参数"
    echo ""
}

# ============================================================
# 卸载流程
# ============================================================

do_uninstall() {
    header
    echo -e "${BOLD}  📤 卸载 Nginx-Tools${NC}"
    echo ""

    detect_shell

    # 检测安装状态
    local has_shell=0
    local has_tui=0
    [ -d "$INSTALL_DIR/.git" ] && has_shell=1
    [ -x "$TUI_BIN_PATH" ] && has_tui=1

    if [ "$has_shell" -eq 0 ] && [ "$has_tui" -eq 0 ]; then
        warn "未检测到任何已安装的 Nginx-Tools 组件"
        exit 0
    fi

    echo ""
    [ "$has_shell" -eq 1 ] && info "已检测到 Shell 版: $INSTALL_DIR"
    [ "$has_tui" -eq 1 ]   && info "已检测到 TUI 版:   $TUI_BIN_PATH"
    echo ""

    ask "  ⚠️  确认卸载以上所有组件？(y/N) " REPLY_CONFIRM
    if [[ ! "$REPLY_CONFIRM" =~ ^[Yy]$ ]]; then
        echo "  已取消"
        exit 0
    fi

    echo ""

    # 卸载 Shell 版
    if [ "$has_shell" -eq 1 ]; then
        remove_aliases
        rm -rf "$INSTALL_DIR"
        success "已删除 $INSTALL_DIR"
    fi

    # 卸载 TUI 版
    if [ "$has_tui" -eq 1 ]; then
        rm -f "$TUI_BIN_PATH"
        success "已删除 $TUI_BIN_PATH"
    fi

    # 询问是否删除备份
    if [ -d "$REAL_HOME/nginx-backups" ]; then
        echo ""
        ask "  是否同时删除备份目录 ~/nginx-backups？(y/N) " REPLY_BACKUP
        if [[ "$REPLY_BACKUP" =~ ^[Yy]$ ]]; then
            rm -rf "$REAL_HOME/nginx-backups"
            success "已删除备份目录"
        else
            info "保留备份目录: ~/nginx-backups"
        fi
    fi

    echo ""
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}${BOLD}  ✅ 卸载完成${NC}"
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    if [ "$has_shell" -eq 1 ]; then
        echo "  请重新加载终端或执行："
        echo -e "    ${CYAN}source $SHELL_CONFIG${NC}"
        echo ""
    fi
    echo -e "  ${DIM}注意: Nginx 和 Certbot 未被卸载，需要时请手动移除${NC}"
    echo ""
}

# ============================================================
# 入口
# ============================================================

usage() {
    cat <<'EOF'

用法: bash install.sh [命令]

命令:
  （无参数）      交互式选择 shell / tui 安装模式（默认）
  shell           安装 Shell 版（Bash 脚本 + ng / ngmon 别名）
  tui             安装 TUI 版（ngtool 二进制 → /usr/local/bin/ngtool）
  uninstall       卸载（自动检测并移除已安装组件）
  help, -h        显示本帮助

示例:
  sudo bash install.sh
  sudo bash install.sh tui
  sudo bash install.sh uninstall

EOF
}

MODE=""

case "${1:-}" in
    ""|install)
        choose_mode
        ;;
    shell)
        MODE="shell"
        ;;
    tui)
        MODE="tui"
        ;;
    uninstall|remove)
        do_uninstall
        exit 0
        ;;
    help|-h|--help)
        usage
        exit 0
        ;;
    *)
        error "未知命令: $1"
        usage
        exit 1
        ;;
esac

case "$MODE" in
    shell) install_shell ;;
    tui)   install_tui   ;;
    *)
        error "未指定安装模式"
        usage
        exit 1
        ;;
esac
