#!/bin/sh
set -eu

CONFIG_PATH="${NODEGET_CONFIG_PATH:-/etc/nodeget/config.toml}"
DATA_DIR="${NODEGET_DATA_DIR:-/var/lib/nodeget}"

toml_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

write_config_from_env() {
    port="${NODEGET_PORT:-${PORT:-2211}}"
    ws_listener="${NODEGET_WS_LISTENER:-0.0.0.0:${port}}"
    server_uuid="${NODEGET_SERVER_UUID:-${SERVER_UUID:-auto_gen}}"
    log_filter="${NODEGET_LOG_FILTER:-${LOG_FILTER:-info}}"
    database_url="${NODEGET_DATABASE_URL:-${DATABASE_URL:-sqlite:///${DATA_DIR}/nodeget.db?mode=rwc}}"

    cat >"${CONFIG_PATH}" <<EOF
ws_listener = "$(toml_escape "${ws_listener}")"
jsonrpc_max_connections = ${NODEGET_JSONRPC_MAX_CONNECTIONS:-100}
enable_unix_socket = ${NODEGET_ENABLE_UNIX_SOCKET:-false}
unix_socket_path = "$(toml_escape "${NODEGET_UNIX_SOCKET_PATH:-/var/lib/nodeget.sock}")"
server_uuid = "$(toml_escape "${server_uuid}")"

[logging]
log_filter = "$(toml_escape "${log_filter}")"

[monitoring_buffer]
flush_interval_ms = ${NODEGET_MONITORING_FLUSH_INTERVAL_MS:-500}
max_batch_size = ${NODEGET_MONITORING_MAX_BATCH_SIZE:-1000}

[database]
database_url = "$(toml_escape "${database_url}")"
connect_timeout_ms = ${NODEGET_DB_CONNECT_TIMEOUT_MS:-3000}
acquire_timeout_ms = ${NODEGET_DB_ACQUIRE_TIMEOUT_MS:-3000}
idle_timeout_ms = ${NODEGET_DB_IDLE_TIMEOUT_MS:-3000}
max_lifetime_ms = ${NODEGET_DB_MAX_LIFETIME_MS:-30000}
max_connections = ${NODEGET_DB_MAX_CONNECTIONS:-10}
EOF
}

has_config_arg() {
    for arg in "$@"; do
        case "${arg}" in
            -c | --config | --config=*) return 0 ;;
        esac
    done
    return 1
}

mkdir -p "$(dirname "${CONFIG_PATH}")" "${DATA_DIR}"

if [ -d "${CONFIG_PATH}" ]; then
    echo "Config path ${CONFIG_PATH} is a directory. Remove it or replace it with a regular file." >&2
    exit 1
fi

if [ ! -s "${CONFIG_PATH}" ]; then
    write_config_from_env
fi

if [ "$#" -eq 0 ]; then
    set -- serve
fi

case "$1" in
    nodeget-server)
        if [ "$#" -ge 2 ]; then
            subcommand="$2"
            case "${subcommand}" in
                serve | init | get-uuid | roll-super-token)
                    shift 2
                    if has_config_arg "$@"; then
                        set -- nodeget-server "${subcommand}" "$@"
                    else
                        set -- nodeget-server "${subcommand}" "$@" -c "${CONFIG_PATH}"
                    fi
                    ;;
            esac
        fi
        ;;
    serve | init | get-uuid | roll-super-token)
        subcommand="$1"
        shift
        if has_config_arg "$@"; then
            set -- nodeget-server "${subcommand}" "$@"
        else
            set -- nodeget-server "${subcommand}" "$@" -c "${CONFIG_PATH}"
        fi
        ;;
    version | -h | --help)
        set -- nodeget-server "$@"
        ;;
esac

exec "$@"
