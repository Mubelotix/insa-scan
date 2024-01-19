use std::net::Ipv4Addr;
use std::str::FromStr;
use hickory_client::client::{Client, SyncClient};
use hickory_client::udp::UdpClientConnection;
use hickory_client::op::DnsResponse;
use hickory_client::rr::{DNSClass, Name, RData, Record, RecordType};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};


fn generate_domains() -> Vec<String> {
    let mut domains = Vec::new();
    for room in [11, 13, 15, 17] {
        for i in 1..20 {
            let domain = format!("iti-mahr2{room}-{i:02}.insa-rouen.fr");
            domains.push(domain);
        }
    }
    for i in 1..20 {
        let domain = format!("meca-{:02}.insa-rouen.fr", i);
        domains.push(domain);
    }
    domains
}

fn generate_ips(ips: &mut Vec<(String, Ipv4Addr)>) {
    for i in 0..255 {
        for j in 0..=255 { // 255
            let ip1 = Ipv4Addr::new(172, 29, j, i);
            if !ips.iter().any(|(_, ip)| *ip == ip1) {
                ips.push((String::new(), ip1));
            }
        }
    }
}

fn check_domains(domains: Vec<String>) -> Vec<(String, Ipv4Addr)> {
    let address = "127.0.0.53:53".parse().unwrap();
    let conn = UdpClientConnection::new(address).unwrap();
    let client = SyncClient::new(conn);

    let mut ips = Vec::new();

    for domain in domains {
        let name = Name::from_str(&domain).unwrap();
        let response: DnsResponse = client.query(&name, DNSClass::IN, RecordType::A).unwrap();
        let answers: &[Record] = response.answers();
        for answer in answers {
            if let Some(RData::A(ref ip)) = answer.data() {
                ips.push((domain.clone(), ip.0));
            }
        }
    }

    ips
}

fn check_ips(ips: Vec<(String, Ipv4Addr)>) -> Vec<(String, Ipv4Addr)> {
    std::env::set_var("RAYON_NUM_THREADS", "3000");
    ips.par_iter().filter_map(|(d, ip)| {
        let r = std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::new(std::net::IpAddr::V4(*ip), 22),
            std::time::Duration::from_secs(1),
        );
        match r {
            Ok(_) => Some((d.clone(), *ip)),
            Err(_) => None,
        }
    }).collect()
}

fn main() {
    let domains = generate_domains();
    println!("{:?}", domains);
    let mut ips = check_domains(domains);
    generate_ips(&mut ips);
    println!("{:?}", ips);
    let ips = check_ips(ips);
    println!("{:?}", ips);
}
