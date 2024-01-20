cargo build --release --target x86_64-unknown-linux-musl
docker build -t network-scanner:0.1.0 .
docker run -it -v $(pwd):/data network-scanner:0.1.0
