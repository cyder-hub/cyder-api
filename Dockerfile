# Stage 1: Use the base image and install cargo-chef
# 1.93.0-24.13.0-alpine3.23
FROM cyderhub/cyder-api-base@sha256:4c7089fa3bb10a31405f4dbb7773e54b4f37f3d4261afedc8487f013dd88a144 AS chef
RUN cargo install cargo-chef --locked
WORKDIR /work

# Stage 2: Plan Rust dependencies
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Build Rust dependencies and business logic
FROM chef AS builder
COPY --from=planner /work/recipe.json recipe.json
# Build Rust dependencies (cached layer)
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN cargo chef cook --release --all-targets --recipe-path recipe.json

# Install frontend dependencies in their own cacheable layer.
COPY front/package.json front/package-lock.json front/
RUN npm --prefix front ci

# Copy the remaining project source. .dockerignore excludes build outputs and deps.
COPY . .

RUN npm --prefix front run build
RUN cargo build -p cyder-api --release

# Prepare final artifacts
RUN mkdir -p /opt/cyder/bin && \
    cp /work/target/release/cyder-api /opt/cyder/bin/cyder-api && \
    cp -r /work/front/dist /opt/cyder/public

# Stage 4: Final runtime image
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

COPY --from=builder /opt/cyder /opt/cyder/
COPY docker-entrypoint /usr/local/bin/cyder-entrypoint
RUN chmod 0755 /usr/local/bin/cyder-entrypoint && \
    chmod -R a+rX /opt/cyder

ENV CYDER_DATA_DIR=/data/cyder
WORKDIR /opt/cyder
VOLUME ["/data/cyder"]
EXPOSE 8000

ENTRYPOINT ["/usr/local/bin/cyder-entrypoint"]
CMD ["/opt/cyder/bin/cyder-api"]
