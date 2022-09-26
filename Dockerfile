FROM rust:1.63 as builder
WORKDIR /usr/src/clementine
COPY . .
RUN cargo install --path .

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y libfreetype6 libfontconfig1-dev && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/clementine /usr/local/bin/clementine
ENTRYPOINT ["clementine"]