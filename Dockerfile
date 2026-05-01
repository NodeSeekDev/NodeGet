# syntax=docker/dockerfile:1

ARG ALPINE_VERSION=3.22
ARG RUST_IMAGE=rustlang/rust:nightly-alpine3.22

FROM alpine:${ALPINE_VERSION} AS release-binary

ARG NODEGET_VERSION=latest
ARG NODEGET_RELEASE_REPO=GenshinMinecraft/NodeGet
ARG TARGETARCH
ARG TARGETVARIANT

RUN apk add --no-cache ca-certificates curl

RUN set -eux; \
    docker_target="${TARGETARCH}${TARGETVARIANT}"; \
    if [ -z "${docker_target}" ]; then \
        docker_target="$(apk --print-arch)"; \
    fi; \
    case "${docker_target}" in \
        amd64* | x86_64) asset="nodeget-server-linux-x86_64-musl" ;; \
        arm64* | aarch64) asset="nodeget-server-linux-aarch64-musl" ;; \
        armv7) asset="nodeget-server-linux-armv7-musleabihf" ;; \
        *) \
            echo "Unsupported Docker target: ${docker_target}. Supported: linux/amd64, linux/arm64, linux/arm/v7" >&2; \
            exit 1; \
            ;; \
    esac; \
    if [ "${NODEGET_VERSION}" = "latest" ]; then \
        release_path="latest/download"; \
    else \
        release_path="download/${NODEGET_VERSION}"; \
    fi; \
    mkdir -p /out; \
    curl -fsSL --retry 5 --retry-delay 2 \
        "https://github.com/${NODEGET_RELEASE_REPO}/releases/${release_path}/${asset}" \
        -o /out/nodeget-server; \
    chmod 0755 /out/nodeget-server

FROM ${RUST_IMAGE} AS source-binary

WORKDIR /src

RUN apk add --no-cache build-base clang clang-dev git musl-dev pkgconf

COPY . .

RUN cargo build --package nodeget-server --profile minimal --locked \
    && mkdir -p /out \
    && cp target/minimal/nodeget-server /out/nodeget-server \
    && chmod 0755 /out/nodeget-server

FROM alpine:${ALPINE_VERSION} AS runtime-base

LABEL org.opencontainers.image.title="NodeGet Server"
LABEL org.opencontainers.image.description="NodeGet server runtime image based on Alpine Linux"
LABEL org.opencontainers.image.source="https://github.com/GenshinMinecraft/NodeGet"
LABEL org.opencontainers.image.licenses="AGPL-3.0"

RUN apk add --no-cache ca-certificates tzdata \
    && update-ca-certificates \
    && mkdir -p /etc/nodeget /var/lib/nodeget

COPY docker/entrypoint.sh /usr/local/bin/nodeget-entrypoint

RUN chmod 0755 /usr/local/bin/nodeget-entrypoint

WORKDIR /etc/nodeget

ENV NODEGET_PORT="3000" \
    NODEGET_SERVER_UUID="auto_gen" \
    NODEGET_LOG_FILTER="info" \
    NODEGET_CONFIG_PATH="/etc/nodeget/config.toml" \
    NODEGET_DATABASE_URL="sqlite:///var/lib/nodeget/nodeget.db?mode=rwc"

EXPOSE 3000

ENTRYPOINT ["/usr/local/bin/nodeget-entrypoint"]
CMD ["serve"]

FROM runtime-base AS runtime-release
COPY --from=release-binary /out/nodeget-server /usr/local/bin/nodeget-server

FROM runtime-base AS runtime-source
COPY --from=source-binary /out/nodeget-server /usr/local/bin/nodeget-server

FROM runtime-release AS runtime
