FROM rust:1.43 as builder
WORKDIR /usr/src/panoptocord
COPY . .
RUN cargo install --path .

FROM ubuntu:latest
RUN apt-get update && apt-get install -y libssl1.1 ca-certificates
COPY --from=builder /usr/local/cargo/bin/panoptocord /usr/local/bin/panoptocord
VOLUME /cache
WORKDIR /cache
CMD ["panoptocord"]