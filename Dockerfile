FROM alpine:latest

VOLUME /data

ADD target/x86_64-unknown-linux-musl/release/network-scanner /network-scanner
ENV DATA_DIR=/data
CMD /network-scanner
