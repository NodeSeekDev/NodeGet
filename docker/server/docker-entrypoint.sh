#!/bin/sh
set -eu

config_path="${NODEGET_CONFIG:-/etc/nodeget/config.toml}"
config_dir="$(dirname "$config_path")"

mkdir -p "$config_dir"

if [ ! -f "$config_path" ]; then
    cat > "$config_path" <<EOF
ws_listener = "0.0.0.0:${NODEGET_PORT:-3000}"
jsonrpc_max_connections = 100
enable_unix_socket = false
unix_socket_path = "/var/lib/nodeget.sock"
server_uuid = "${NODEGET_SERVER_UUID:-auto_gen}"

[logging]
log_filter = "${NODEGET_LOG_FILTER:-info}"

[monitoring_buffer]

[database]
database_url = "${NODEGET_DATABASE_URL:-sqlite:///tmp/nodeget.db?mode=rwc}"
connect_timeout_ms = 3000
acquire_timeout_ms = 3000
idle_timeout_ms = 3000
max_lifetime_ms = 30000
max_connections = ${NODEGET_DATABASE_MAX_CONNECTIONS:-10}
EOF
fi

if [ "$#" -eq 0 ]; then
    set -- serve
fi

case "$1" in
    serve|init|roll-super-token|get-uuid)
        command="$1"
        shift
        set -- nodeget-server "$command" --config "$config_path" "$@"
        ;;
    version|--version|-V)
        set -- nodeget-server version
        ;;
    nodeget-server)
        ;;
    *)
        set -- nodeget-server "$@"
        ;;
esac

exec "$@"
