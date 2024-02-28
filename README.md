# insa-scan

This project is a scanner for the INSA network.
It's intented to help students find machines to connect to over SSH.
This was a pain point for me, especially during the night or week-end, when almost all machines are turned off.
A working VPN is configured in a docker, so that all you need is valid INSA credentials.
This project is able to track uptime statistics, along with CPU and memory characteristics.

## Building

```bash
cargo build --release
sudo docker build -t insa-scan:0.1.0 .
```

## Running

```bash
sudo docker run \
    --name insa-scan \
    -v $(pwd):/data \
    -e INSA_USERNAME='username' \
    -e INSA_PASSWORD='password' \
    --cap-add=NET_ADMIN \
    insa-scan:0.1.0
```
