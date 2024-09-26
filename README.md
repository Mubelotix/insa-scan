# insa-scan

This project is a scanner for the INSA network.
It's intented to help students find machines to connect to over SSH.
This was a pain point for me, especially during the night or week-end, when almost all machines are turned off.
A working VPN is configured in a docker, so that all you need is valid INSA credentials.
This project is able to track uptime statistics, along with CPU and memory characteristics.

## Project status

It is with deep disappointment and frustration that I must inform users that this project has been forcibly terminated by the school administration. Despite its potential to address a significant issue faced by students, particularly during off-hours, the administration showed a concerning lack of support or understanding for student-driven initiatives aimed at improving the learning environment. The abrupt shutdown of this project, without meaningful dialogue or consideration of its benefits, raises questions about the institution's commitment to fostering innovation and addressing student needs. This experience has been deeply disheartening and reflects a disconnect between administrative decisions and the practical challenges faced by the student body. I hope that M. Vasseur might reconsider his choice someday.

## Building

```bash
cargo build --release
sudo docker build -t mubelotix/insa-scan:0.1.1 .
```

## Deploying

```bash
sudo docker push mubelotix/insa-scan:0.1.1
```

## Running

```bash
sudo docker run \
    --name insa-scan \
    -v $(pwd):/data \
    -e INSA_USERNAME='username' \
    -e INSA_PASSWORD='password' \
    --cap-add=NET_ADMIN \
    mubelotix/insa-scan:0.1.1
```
