FROM rust:1.85.1-alpine3.20 AS builder

RUN apk add musl-dev build-base perl cmake ncurses-dev libtirpc-dev --no-cache
