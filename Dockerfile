# Stage 1: Download the pre-compiled musl binary
FROM alpine:latest AS downloader

ARG TARGETARCH
ARG VERSION=latest

RUN apk add --no-cache wget ca-certificates tzdata

WORKDIR /app

RUN set -ex; \
    if [ -z "$TARGETARCH" ]; then \
        ARCH=$(uname -m); \
        if [ "$ARCH" = "x86_64" ]; then TARGETARCH="amd64"; \
        elif [ "$ARCH" = "aarch64" ]; then TARGETARCH="arm64"; \
        else echo "Unsupported architecture: $ARCH"; exit 1; fi; \
    fi; \
    if [ "$TARGETARCH" = "amd64" ]; then \
        ARCH="x86_64"; \
    elif [ "$TARGETARCH" = "arm64" ]; then \
        ARCH="aarch64"; \
    else \
        echo "Unsupported architecture: $TARGETARCH"; \
        exit 1; \
    fi; \
    BINARY_NAME="nodeget-server-linux-${ARCH}-musl"; \
    if [ "$VERSION" = "latest" ]; then \
        DOWNLOAD_URL="https://github.com/NodeSeekDev/NodeGet/releases/latest/download/${BINARY_NAME}"; \
    elif [ "$VERSION" = "dev" ]; then \
        DOWNLOAD_URL="https://github.com/NodeSeekDev/NodeGet/releases/download/dev/${BINARY_NAME}"; \
    else \
        DOWNLOAD_URL="https://github.com/NodeSeekDev/NodeGet/releases/download/${VERSION}/${BINARY_NAME}"; \
    fi; \
    echo "Downloading ${DOWNLOAD_URL}..."; \
    wget -qO /app/nodeget-server "$DOWNLOAD_URL"; \
    chmod +x /app/nodeget-server

COPY entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh

# Stage 2: Runtime with minimal alpine
FROM alpine:3.21 AS runtime

WORKDIR /app

# Copy only what's needed for runtime
COPY --from=downloader /app/nodeget-server /app/nodeget-server
COPY --from=downloader /app/entrypoint.sh /app/entrypoint.sh
COPY --from=downloader /etc/ssl/certs /etc/ssl/certs
COPY --from=downloader /usr/share/zoneinfo /usr/share/zoneinfo

# Default Environment Variables
ENV PORT=3000
ENV SERVER_UUID=auto_gen
ENV LOG_FILTER=info
ENV DATABASE_URL=sqlite://nodeget.db?mode=rwc

EXPOSE 3000

ENTRYPOINT ["/app/entrypoint.sh"]