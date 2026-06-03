# syntax=docker/dockerfile:1

ARG ALPINE_VERSION=3.22

FROM alpine:${ALPINE_VERSION} AS runtime

LABEL org.opencontainers.image.title="NodeGet Server"
LABEL org.opencontainers.image.description="NodeGet server runtime image based on Alpine Linux"
LABEL org.opencontainers.image.source="https://github.com/GenshinMinecraft/NodeGet"
LABEL org.opencontainers.image.licenses="AGPL-3.0"

RUN apk add --no-cache ca-certificates tzdata \
    && update-ca-certificates

ARG TARGETARCH
COPY bin/nodeget-server-${TARGETARCH} /nodeget/nodeget-server
COPY docker/entrypoint.sh /nodeget/entrypoint.sh
RUN chmod 0755 /nodeget/nodeget-server /nodeget/entrypoint.sh

WORKDIR /nodeget

ENV NODEGET_DATABASE_URL="sqlite:///nodeget/nodeget.db?mode=rwc"

EXPOSE 2211

ENTRYPOINT ["/nodeget/entrypoint.sh"]
