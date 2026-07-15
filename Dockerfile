# 1.93.0-24.13.0-alpine3.23
FROM cyderhub/cyder-api-base@sha256:4c7089fa3bb10a31405f4dbb7773e54b4f37f3d4261afedc8487f013dd88a144 AS base
WORKDIR /work

# Rust dependency planner and builder.
FROM base AS chef
RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY server ./server
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS rust-builder
COPY --from=planner /work/recipe.json recipe.json
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN cargo chef cook --release --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY server ./server
RUN cargo build --locked -p cyder-api --release --bin cyder-api
RUN mkdir -p /opt/cyder/bin && \
    cp /work/target/release/cyder-api /opt/cyder/bin/cyder-api

# Build the frontend independently so frontend-only changes do not invalidate
# the Rust binary layer, and backend-only changes keep the frontend layer hot.
FROM base AS frontend-builder
COPY front/package.json front/package-lock.json front/
RUN npm --prefix front ci
COPY front ./front
RUN npm --prefix front run build

# Final runtime image.
FROM alpine:3.21.3

RUN apk add --no-cache ca-certificates su-exec && \
    addgroup -g 1000 -S cyder && \
    adduser -u 1000 -S -D -H -G cyder -s /sbin/nologin cyder && \
    mkdir -p \
        /opt/cyder/bin \
        /opt/cyder/public \
        /data/cyder/config \
        /data/cyder/db \
        /data/cyder/storage \
        /tmp/cyder-api && \
    chown -R cyder:cyder /data/cyder /tmp/cyder-api

COPY --from=rust-builder /opt/cyder/bin /opt/cyder/bin/
COPY --from=frontend-builder /work/front/dist /opt/cyder/public/
COPY docker-entrypoint /usr/local/bin/cyder-entrypoint
RUN chmod 0755 /usr/local/bin/cyder-entrypoint && \
    chmod -R a+rX /opt/cyder

ENV CYDER_DATA_DIR=/data/cyder
WORKDIR /opt/cyder
VOLUME ["/data/cyder"]
EXPOSE 8000

ENTRYPOINT ["/usr/local/bin/cyder-entrypoint"]
CMD ["/opt/cyder/bin/cyder-api"]
