#!/bin/sh
set -eu

config_path="${NODEGET_CONFIG:-/config/config.toml}"
config_dir="$(dirname "$config_path")"

mkdir -p "$config_dir"

if [ ! -f "$config_path" ]; then
    cat > "$config_path" <<EOF
ws_listener = "0.0.0.0:${NODEGET_PORT:-3000}"
jsonrpc_max_connections = ${NODEGET_JSONRPC_MAX_CONNECTIONS:-100}
enable_unix_socket = ${NODEGET_ENABLE_UNIX_SOCKET:-false}
unix_socket_path = "${NODEGET_UNIX_SOCKET_PATH:-/var/lib/nodeget.sock}"
server_uuid = "${NODEGET_SERVER_UUID:-auto_gen}"

[logging]
log_filter = "${NODEGET_LOG_FILTER:-info}"

[monitoring_buffer]
flush_interval_ms = ${NODEGET_MONITORING_FLUSH_INTERVAL_MS:-500}
max_batch_size = ${NODEGET_MONITORING_MAX_BATCH_SIZE:-1000}

[database]
database_url = "${NODEGET_DATABASE_URL:-sqlite:///config/nodeget.db?mode=rwc}"
connect_timeout_ms = ${NODEGET_DATABASE_CONNECT_TIMEOUT_MS:-3000}
acquire_timeout_ms = ${NODEGET_DATABASE_ACQUIRE_TIMEOUT_MS:-3000}
idle_timeout_ms = ${NODEGET_DATABASE_IDLE_TIMEOUT_MS:-3000}
max_lifetime_ms = ${NODEGET_DATABASE_MAX_LIFETIME_MS:-30000}
max_connections = ${NODEGET_DATABASE_MAX_CONNECTIONS:-10}
EOF
fi

if [ "$(id -u)" -eq 0 ]; then
    chown -R nodeget:nodeget "$config_dir"
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

if [ "$(id -u)" -eq 0 ] && [ "$1" = "nodeget-server" ]; then
    set -- su-exec nodeget "$@"
fi

exec "$@"
