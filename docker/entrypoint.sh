#!/bin/sh
set -eu

CONFIG="/nodeget/config.toml"

if [ ! -f "${CONFIG}" ]; then
    db_url="${NODEGET_DATABASE_URL:-sqlite:///nodeget/nodeget.db?mode=rwc}"
    cat > "${CONFIG}" <<EOF
ws_listener = "0.0.0.0:2211"
server_uuid = "auto_gen"

[logging]
log_filter = "info"

[database]
database_url = "${db_url}"
EOF
    # TimescaleDB 时序优化（可选）：设置任一 NODEGET_TIMESCALE_* 环境变量即启用。
    # 仅当数据库是安装了 timescaledb 扩展的 PostgreSQL 时生效。
    # 注意 retention_days 默认 0 = 不启用自动删除（避免误删历史数据），需要时显式配置。
    if [ -n "${NODEGET_TIMESCALE_CHUNK_INTERVAL_DAYS:-}" ] \
        || [ -n "${NODEGET_TIMESCALE_COMPRESS_AFTER_DAYS:-}" ] \
        || [ -n "${NODEGET_TIMESCALE_RETENTION_DAYS:-}" ]; then
        cat >> "${CONFIG}" <<EOF

[database.timescale]
chunk_interval_days = ${NODEGET_TIMESCALE_CHUNK_INTERVAL_DAYS:-1}
compress_after_days = ${NODEGET_TIMESCALE_COMPRESS_AFTER_DAYS:-7}
retention_days = ${NODEGET_TIMESCALE_RETENTION_DAYS:-0}
EOF
    fi
fi

exec nodeget-server serve -c "${CONFIG}"
