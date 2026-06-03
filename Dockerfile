# syntax=docker/dockerfile:1

ARG ALPINE_VERSION=3.22

FROM alpine:${ALPINE_VERSION} AS runtime

LABEL org.opencontainers.image.title="NodeGet Server"
LABEL org.opencontainers.image.description="NodeGet server runtime image based on Alpine Linux"
LABEL org.opencontainers.image.licenses="AGPL-3.0"
LABEL org.opencontainers.image.source="https://github.com/GenshinMinecraft/NodeGet"

RUN apk add --no-cache ca-certificates tzdata \
    && update-ca-certificates \
    && mkdir -p /nodeget

ARG TARGETARCH
COPY bin/nodeget-server-${TARGETARCH} /usr/local/bin/nodeget-server
COPY docker/entrypoint.sh /usr/local/bin/nodeget-entrypoint
RUN chmod 0755 /usr/local/bin/nodeget-server /usr/local/bin/nodeget-entrypoint

WORKDIR /nodeget

ENV NODEGET_DATABASE_URL="sqlite:///nodeget/nodeget.db?mode=rwc"

EXPOSE 2211

ENTRYPOINT ["nodeget-entrypoint"]
