#!/bin/sh
set -eu

CONFIG="/nodeget/config.toml"

if [ ! -f "${CONFIG}" ]; then
    db_url="${NODEGET_DATABASE_URL:-sqlite:///nodeget/nodeget.db?mode=rwc}"
    cat > "${CONFIG}" <<EOF
ws_listener = "0.0.0.0:2211"
server_uuid = "c56aa660-190f-498c-92ba-86fccd566673"

[logging]
log_filter = "info"

[database]
database_url = "${db_url}"
EOF
fi

exec nodeget-server serve -c "${CONFIG}"
