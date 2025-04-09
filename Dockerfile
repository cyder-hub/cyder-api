# build
FROM chenluo/cyder-api-base:1.85.1-alpine3.20 AS builder

ADD . /work
WORKDIR /work

RUN cargo build --release

# install
FROM alpine:3.21.3

COPY --from=builder /work/target/release/cyder-api /app/
COPY --from=builder /work/public /app/public
RUN mkdir /app/storage

WORKDIR /app

CMD ["/app/cyder-api"]
