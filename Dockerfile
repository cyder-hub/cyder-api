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

# Copy all source files (including the small frontend project)
COPY . .

# Execute the custom build task
RUN cargo xtask build

# Prepare final artifacts
RUN mkdir -p /app && \
    cp /work/target/release/cyder-api /app && \
    cp -r /work/front/dist /app/public

# Stage 4: Final runtime image
FROM alpine:3.21.3
COPY --from=builder /app /app/
RUN mkdir -p /app/storage
WORKDIR /app

CMD ["/app/cyder-api"]
