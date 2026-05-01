# NodeGet Server Docker

This image runs `nodeget-server` on Alpine Linux by downloading the released musl binary.

## Configuration

The container uses `/etc/nodeget/config.toml` by default. If the file does not exist, the entrypoint generates it from environment variables.

Supported environment variables:

- `NODEGET_PORT`: server port, default `3000`
- `NODEGET_SERVER_UUID`: server UUID, default `auto_gen`
- `NODEGET_LOG_FILTER`: log filter, default `info`
- `NODEGET_DATABASE_URL`: database URL, default `sqlite:///etc/nodeget/nodeget.db?mode=rwc`
- `NODEGET_DATABASE_MAX_CONNECTIONS`: database max connections, default `10`
- `NODEGET_CONFIG`: config path, default `/etc/nodeget/config.toml`

If you mount your own `config.toml`, the environment variables are not written into it.
The compose files persist `./nodeget-config/config.toml` only. Other runtime data is not persisted by default.
The SQLite compose file stores its database in `/tmp`, so it is suitable for simple or disposable deployments.

## SQLite

```bash
docker compose -f docker-compose.sqlite.yml up -d
```

## PostgreSQL

```bash
docker compose -f docker-compose.postgres.yml up -d
```

## Build

```bash
docker build -f Dockerfile.server --build-arg NODEGET_VERSION=v0.0.6 -t nodeget-server:local .
```
