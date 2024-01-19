use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::str::FromStr;
use hickory_client::client::{Client, SyncClient};
use hickory_client::udp::UdpClientConnection;
use hickory_client::op::DnsResponse;
use hickory_client::rr::{DNSClass, Name, RData, Record, RecordType};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};


// IPs are updated on an hourly basis
// The hour is divided into 6 parts.
// Computers we think are on are checked at each one.
// The rest of them only get checked once every hour.

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

fn generate_ips() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    for i in 0..255 {
        for j in 0..=255 {
            let ip = Ipv4Addr::new(172, 29, j, i);
            ips.push(ip);
        }
    }
    ips
}

#[derive(Default)]
struct MachineState {
    last_checked_utc: Option<u64>,
    up: Option<bool>,

    last_change_utc: Option<u64>,
    domain: Option<String>,
    uptime: u64,
    downtime: u64,
}

type States = HashMap<Ipv4Addr, MachineState>;

fn now_utc() -> u64 {
    chrono::Utc::now().timestamp() as u64
}

async fn restore_state() -> States {
    let file = tokio::fs::read_to_string("history.csv").await.expect("Failed to read history.csv");

    let mut states = States::new();
    for (i, line) in file.lines().enumerate() {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 3 {
            eprintln!("Error line {i}: Wrong number of parts");
            continue;
        }
        let Ok(ip) = Ipv4Addr::from_str(parts[0]) else { 
            eprintln!("Error line {i}: Invalid IP");
            continue;
        };
        let Ok(last_checked_utc) = parts[2].parse() else {
            eprintln!("Error line {i}: Invalid last checked");
            continue;
        };
        let Ok(up) = parts[3].parse() else {
            eprintln!("Error line {i}: Invalid up");
            continue;
        };
        let state = states.entry(ip).or_default();
        match (state.up, up) {
            (Some(true), true) => state.uptime += last_checked_utc - state.last_checked_utc.unwrap_or(last_checked_utc),
            (Some(false), false) => state.downtime += last_checked_utc - state.last_checked_utc.unwrap_or(last_checked_utc),
            (Some(false), true) | (None, true) => {
                state.downtime += last_checked_utc - state.last_checked_utc.unwrap_or(last_checked_utc);
                state.last_change_utc = Some(last_checked_utc);
            },
            (Some(true), false) | (None, false) => {
                state.downtime = 0;
                state.last_change_utc = Some(last_checked_utc);
            },
        }
        state.last_checked_utc = Some(last_checked_utc);
        state.up = Some(up);
    }

    states
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

#[tokio::main]
async fn main() {
    let mut states = restore_state().await;
    
    let domains = generate_domains();
    println!("{:?}", domains);
    let mut ips = check_domains(domains);
    generate_ips(&mut ips);
    println!("{:?}", ips);
    let ips = check_ips(ips);
    println!("{:?}", ips);
}
