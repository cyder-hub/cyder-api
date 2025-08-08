# frontend builder
FROM node:22.16.0-alpine3.22 AS front-builder

ADD ./front /work
WORKDIR /work

RUN npm ci
RUN npm run build 

# build
FROM cyderhub/cyder-api-base:1.85.1-alpine3.20 AS backend-builder

ADD . /work
WORKDIR /work

RUN cargo xtask build-backend

# install
FROM alpine:3.21.3

COPY --from=backend-builder /work/target/release/cyder-api /app/
COPY --from=front-builder /work/dist /app/public
RUN mkdir /app/storage

WORKDIR /app

CMD ["/app/cyder-api"]
