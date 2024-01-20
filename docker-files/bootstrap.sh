set -e

echo "Creating TUN device /dev/net/tun"
rm -f /dev/net/tun
mkdir -p /dev/net
mknod /dev/net/tun c 10 200
chmod 0666 /dev/net/tun

echo "Starting OpenVPN session"
echo $INSA_USERNAME > /etc/openvpn/credentials
echo $INSA_PASSWORD >> /etc/openvpn/credentials
openvpn --config /insa-ovpn-tun-ca.ovpn --auth-user-pass /etc/openvpn/credentials --daemon

sleep 5
if ! pgrep -x "openvpn" > /dev/null; then
  echo "OpenVPN failed to start. Exiting."
  exit 1
fi

echo "Starting program"
/network-scanner
