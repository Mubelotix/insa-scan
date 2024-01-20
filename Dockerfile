FROM debian:latest
RUN apt update
RUN apt install -y apt-transport-https curl ssh openvpn

#RUN mkdir -p /etc/apt/keyrings
#RUN curl -sSfL https://packages.openvpn.net/packages-repo.gpg >/etc/apt/keyrings/openvpn.asc
#RUN echo "deb [signed-by=/etc/apt/keyrings/openvpn.asc] https://packages.openvpn.net/openvpn3/debian bookworm main" >>/etc/apt/sources.list.d/openvpn3.list
#RUN apt update
#RUN apt install -y openvpn3

VOLUME /data

ENV INSA_USERNAME= \
    INSA_PASSWORD= \
    DATA_DIR=/data

COPY docker-files/insa-ovpn-tun-ca.ovpn /insa-ovpn-tun-ca.ovpn
COPY docker-files/bootstrap.sh /bootstrap.sh
RUN chmod +x /bootstrap.sh
COPY target/release/network-scanner /network-scanner

CMD [ "sh", "-c", "sh /bootstrap.sh" ]
