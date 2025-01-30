FROM rust:1-bookworm AS builder
WORKDIR /workspace
COPY . .
RUN cargo build --release && cargo build --bin presence_http --release

FROM debian:bookworm-slim
# COPY --from=builder /workspace/target/release/presenced /usr/local/bin/presenced
COPY --from=builder /workspace/target/release/presence_http /usr/local/bin/presence_http
# presence_http by default
CMD ["/usr/local/bin/presence_http"]
