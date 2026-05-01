#!/bin/sh
set -e

CONFIG_FILE="/app/config.toml"

# If config.toml does not exist, generate it from environment variables
if [ ! -f "$CONFIG_FILE" ]; then
    echo "Generating config.toml from environment variables..."
    cat <<EOF > "$CONFIG_FILE"
ws_listener = "0.0.0.0:${PORT:-3000}"
server_uuid = "${SERVER_UUID:-auto_gen}"

[logging]
log_filter = "${LOG_FILTER:-info}"

[database]
database_url = "${DATABASE_URL:-sqlite://nodeget.db?mode=rwc}"
EOF
else
    echo "Found existing config.toml, using it."
fi

# 默认执行 serve 命令
if [ $# -eq 0 ]; then
    set -- serve
fi

# 如果第一个参数是内置的子命令，则自动注入 nodeget-server 和 -c 参数
case "$1" in
    serve|init|roll-super-token|get-uuid)
        CMD="$1"
        shift
        exec /app/nodeget-server "$CMD" -c "$CONFIG_FILE" "$@"
        ;;
    *)
        # 否则作为普通命令执行（如 /bin/sh）
        exec "$@"
        ;;
esac
