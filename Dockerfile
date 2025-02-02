FROM rust:buster as builder
WORKDIR /app

COPY . .

RUN cargo build --release --verbose

# =============

FROM phusion/baseimage:focal-1.2.0

RUN useradd -m -u 1000 -U -s /bin/sh -d /app docker

WORKDIR /app

RUN apt-get update && apt-get install tini

COPY --from=builder /app/target/release/subway /usr/local/bin
COPY configs/eth_config.yml /app/config.yml

# checks
RUN ldd /usr/local/bin/subway && \
	/usr/local/bin/subway --version

# Shrinking
RUN rm -rf /usr/lib/python* && \
	rm -rf /usr/sbin /usr/share/man

USER docker

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/subway", "-c", "/app/config.yml"]
