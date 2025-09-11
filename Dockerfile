# frontend builder
FROM cyderhub/cyder-api-base@sha256:4da6471a75b962a111f055712d2c3e9eb32d0ebc6a0b26dcdca48fec886bf452 AS builder

ADD . /work
WORKDIR /work

RUN cargo xtask build

RUN mkdir -p /app && \
    cp /work/target/release/cyder-api /app && \
    cp -r /work/front/dist /app/public

# install
FROM alpine:3.21.3

COPY --from=builder /app /app/
RUN mkdir -p /app/storage

WORKDIR /app

CMD ["/app/cyder-api"]
