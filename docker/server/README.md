# NodeGet Server Docker

This image runs `nodeget-server` on Alpine Linux by downloading the released musl binary.
Release assets are currently downloaded from `GenshinMinecraft/NodeGet` by default because the official repository release workflow is temporarily unavailable.

## Quick Start

SQLite:

```bash
docker compose -f docker-compose.sqlite.yml up -d
```

PostgreSQL:

```bash
docker compose -f docker-compose.postgres.yml up -d
```

The server listens on port `3000` by default.

## Configuration

The container uses `/config/config.toml` by default. If the file does not exist, the entrypoint generates it from environment variables.

Supported environment variables:

- `NODEGET_PORT`: server port, default `3000`
- `NODEGET_SERVER_UUID`: server UUID, default `auto_gen`
- `NODEGET_LOG_FILTER`: log filter, default `info`
- `NODEGET_DATABASE_URL`: database URL, default `sqlite:///config/nodeget.db?mode=rwc`
- `NODEGET_DATABASE_MAX_CONNECTIONS`: database max connections, default `10`
- `NODEGET_CONFIG`: config path, default `/config/config.toml`

Additional environment variables map to the defaults in `server/config.toml.example`:

- `NODEGET_JSONRPC_MAX_CONNECTIONS`
- `NODEGET_ENABLE_UNIX_SOCKET`
- `NODEGET_UNIX_SOCKET_PATH`
- `NODEGET_MONITORING_FLUSH_INTERVAL_MS`
- `NODEGET_MONITORING_MAX_BATCH_SIZE`
- `NODEGET_DATABASE_CONNECT_TIMEOUT_MS`
- `NODEGET_DATABASE_ACQUIRE_TIMEOUT_MS`
- `NODEGET_DATABASE_IDLE_TIMEOUT_MS`
- `NODEGET_DATABASE_MAX_LIFETIME_MS`

If you mount your own `config.toml`, the environment variables are not written into it.
SQLite uses `./nodeget-config-sqlite/config.toml` and `./nodeget-config-sqlite/nodeget.db`.
PostgreSQL uses `./nodeget-config-postgres/config.toml` and the `postgres-data` named volume.

## Commands

```bash
docker run --rm ghcr.io/nodeseekdev/nodeget-server:latest version
docker run --rm -v ./nodeget-config-sqlite:/config ghcr.io/nodeseekdev/nodeget-server:latest get-uuid
```

The entrypoint automatically appends `--config /config/config.toml` for these commands:

- `serve`
- `init`
- `roll-super-token`
- `get-uuid`

## Build Locally

```bash
docker build -f Dockerfile.server --build-arg NODEGET_VERSION=v0.0.6 -t nodeget-server:local .
```

Build arguments:

- `NODEGET_VERSION`: release tag to package, default `latest`
- `NODEGET_RELEASE_REPO`: repository that hosts release assets, default `GenshinMinecraft/NodeGet`

Supported platforms:

- `linux/amd64`
- `linux/arm64`

## CI Publishing

The `nodeget-server-docker` workflow builds and pushes the image to GHCR.

Docker Hub publishing is optional. Configure these repository secrets to enable it:

- `DOCKERHUB_USERNAME`
- `DOCKERHUB_TOKEN`

Optionally set `DOCKERHUB_IMAGE_NAME` as a repository variable. The default is `nodeseek/nodeget-server`.

## Verification

```bash
docker compose -f docker-compose.sqlite.yml config
docker compose -f docker-compose.postgres.yml config
docker build -f Dockerfile.server --build-arg NODEGET_VERSION=v0.0.6 --build-arg NODEGET_RELEASE_REPO=GenshinMinecraft/NodeGet -t nodeget-server:local .
docker run --rm nodeget-server:local version
```
