use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::os::linux::raw::stat;
use std::str::FromStr;
use std::time::{Instant, Duration};
use futures::future::select_all;
use hickory_client::client::{Client, SyncClient, AsyncClient, ClientHandle};
use hickory_client::tcp::TcpClientStream;
use hickory_client::udp::UdpClientConnection;
use hickory_client::proto::iocompat::AsyncIoTokioAsStd;

use hickory_client::op::DnsResponse;
use hickory_client::rr::{DNSClass, Name, RData, Record, RecordType};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use string_tools::{get_all_before, get_all_before_strict, get_all_after_strict};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};


// IPs are updated on an hourly basis
// The hour is divided into 6 parts.
// Computers we think are on are checked at each one.
// The rest of them only get checked once every hour.

pub async fn run_shell_command(command: impl AsRef<str>) -> Result<String, String> {
    let command = command.as_ref();
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .await
        .expect("failed to execute process");
    let mut stdouterr = String::from_utf8_lossy(&output.stdout).into_owned();
    stdouterr.push_str(String::from_utf8_lossy(&output.stderr).as_ref());
    if output.status.success() {
        Ok(stdouterr)
    } else {
        Err(stdouterr)
    }
}

fn generate_domains() -> Vec<String> {
    let mut domains = Vec::new();
    for room in [11, 13, 15, 17] {
        for i in 1..20 {
            let domain = format!("iti-mahr2{room}-{i:02}");
            domains.push(domain);
        }
    }
    for i in 1..20 {
        let domain = format!("meca-{:02}", i);
        domains.push(domain);
    }
    for i in 1..20 {
        let domain = format!("stpi-aio-{:02}", i);
        domains.push(domain);
    }
    for room in [3, 5, 7, 9] {
        for i in 1..20 {
            let domain = format!("mahr2{room:02}-{i:02}");
            domains.push(domain);
        }
    }
    for room in [3, 5, 7] {
        for i in 1..25 {
            let domain = format!("boar2{room:02}-{:02}", i);
            domains.push(domain);
        }
    }
    domains.push(String::from("lin-2d-mini-03"));
    domains.push(String::from("lin-2d-29"));
    domains
}

async fn resolve_domains(domains: Vec<String>) -> HashMap<Ipv4Addr, String> {
    let address = "127.0.0.53:53".parse().unwrap();
    let (stream, sender) = TcpClientStream::<AsyncIoTokioAsStd<TcpStream>>::new(address);

    // Create a new client, the bg is a background future which handles
    //   the multiplexing of the DNS requests to the server.
    //   the client is a handle to an unbounded queue for sending requests via the
    //   background. The background must be scheduled to run before the client can
    //   send any dns requests
    let client = AsyncClient::new(stream, sender, None);

    let (mut client, bg) = client.await.expect("connection failed");

    // make sure to run the background task
    let handle = tokio::spawn(bg);



    let mut ips = HashMap::new();
    for domain in domains {
        let name = Name::from_str(&format!("{domain}.insa-rouen.fr")).unwrap();
        let response: DnsResponse = client.query(name, DNSClass::IN, RecordType::A).await.unwrap();
        let answers: &[Record] = response.answers();
        for answer in answers {
            if let Some(RData::A(ref ip)) = answer.data() {
                ips.insert(ip.0, domain.clone());
            }
        }
    }

    handle.abort();

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

    extended_info: Option<ExtendedInfo>,
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
    let now_utc = now_utc();

    let mut states = States::new();
    for (i, line) in file.lines().enumerate() {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() != 3 {
            eprintln!("Error line {i}: Wrong number of parts");
            continue;
        }
        let Ok(last_checked_utc) = parts[2].parse() else {
            eprintln!("Error line {i}: Invalid last checked");
            continue;
        };
        if last_checked_utc + 30 * 86_400 < now_utc {  // Too old
            continue;
        }
        let Ok(ip) = Ipv4Addr::from_str(parts[0]) else { 
            eprintln!("Error line {i}: Invalid IP");
            continue;
        };
        let Ok(up) = parts[1].parse() else {
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
                state.uptime += last_checked_utc - state.last_checked_utc.unwrap_or(last_checked_utc);
                state.last_change_utc = Some(last_checked_utc);
            },
        }
        state.last_checked_utc = Some(last_checked_utc);
        state.up = Some(up);
    }

    states
}

async fn save_state(ip: Ipv4Addr, up: bool, now_utc: u64) {
    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open("history.csv")
        .await
        .expect("Failed to open history.csv");
    let line = format!("{},{},{}\n", ip, up, now_utc);
    file.write_all(line.as_bytes()).await.expect("Failed to write to history.csv");
}

async fn check_ip(ip: Ipv4Addr, load_extended_info: bool) -> (Ipv4Addr, bool, Option<ExtendedInfo>) {
    let r = timeout(Duration::from_secs(4), async move {
        TcpStream::connect(
            &std::net::SocketAddr::new(std::net::IpAddr::V4(ip), 22)
        ).await.is_ok()
    }).await;
    let up = r == Ok(true);
    let extended_info = if load_extended_info && up {
        load_extented_info(ip).await
    } else {
        None
    };
    (ip, up, extended_info)
}

async fn update(states: &mut States) {
    let mut candidates: Vec<(Ipv4Addr, bool, u64)> = states.iter().map(|(ip, state)| {
        (*ip, state.up.unwrap_or(false), state.last_checked_utc.unwrap_or(0))
    }).collect();
    candidates.sort_by(|(_, up1, t1), (_, up2, t2)| {
        up1.cmp(up2).reverse().then(t1.cmp(t2))
    });
    candidates.truncate((255*255)/6);

    let mut tasks = Vec::new();
    for _ in 0..200 {
        let Some(ip) = candidates.pop() else { break };
        tasks.push(Box::pin(check_ip(ip.0, states.get(&ip.0).unwrap().extended_info.is_none())));
    }

    while !tasks.is_empty() {
        let ((addr, up, extended_info), _, new_tasks) = select_all(tasks).await;
        tasks = new_tasks;
        if let Some(ip) = candidates.pop() {
            tasks.push(Box::pin(check_ip(ip.0, states.get(&ip.0).unwrap().extended_info.is_none())));
        }
        let now_utc = now_utc();
        let state = states.entry(addr).or_default();
        if let Some(extended_info) = extended_info {
            state.extended_info = Some(extended_info);
        }
        match (state.up, up) {
            (Some(true), true) => state.uptime += now_utc - state.last_checked_utc.unwrap_or(now_utc),
            (Some(false), false) => state.downtime += now_utc - state.last_checked_utc.unwrap_or(now_utc),
            (Some(false), true) | (None, true) => {
                state.downtime += now_utc - state.last_checked_utc.unwrap_or(now_utc);
                state.last_change_utc = Some(now_utc);
            },
            (Some(true), false) | (None, false) => {
                state.uptime += now_utc - state.last_checked_utc.unwrap_or(now_utc);
                state.last_change_utc = Some(now_utc);
            },
        }
        state.last_checked_utc = Some(now_utc);
        if state.up != Some(up) {
            save_state(addr, up, now_utc).await;
        }
        state.up = Some(up);
    }
}

async fn update_stats(states: &States) {
    let now_utc = now_utc();
    let mut lines = Vec::new();
    for (ip, state) in states {
        let up = state.up.unwrap_or(false);
        let last_change_utc = state.last_change_utc.unwrap_or(0);
        let last_checked_utc = state.last_checked_utc.unwrap_or(0);
        let uptime = state.uptime + if up { now_utc - state.last_checked_utc.unwrap_or(now_utc) } else { 0 };
        let downtime = state.downtime + if !up { now_utc - state.last_checked_utc.unwrap_or(now_utc) } else { 0 };
        let domain = state.domain.as_ref().map(|s| s.as_str()).unwrap_or("");
        if uptime == 0 {
            continue;
        }
        lines.push(format!("{ip},{up},{uptime},{downtime},{last_change_utc},{last_checked_utc},{domain}"));
    }
    lines.sort();
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("stats.csv")
        .await
        .expect("Failed to open stats.csv");
    file.write_all(b"ip,up,uptime,downtime,last_change_utc,last_checked_utc,domain\n").await.expect("Failed to write to stats.csv");
    file.write_all(lines.join("\n").as_bytes()).await.expect("Failed to write to stats.csv");
}

#[derive(Debug)]
struct ExtendedInfo {
    hostname: String,
    cpuinfo: String,
    meminfo: String,
    ipaddr: String,
}

async fn load_extented_info(ip: Ipv4Addr) -> Option<ExtendedInfo> {
    let r = timeout(
        Duration::from_secs(3),
        run_shell_command(format!("ssh \"sgirard@{ip}\" \"hostname; echo MUBELOTIX-SEPARATOR; cat /proc/cpuinfo; echo MUBELOTIX-SEPARATOR; cat /proc/meminfo; echo MUBELOTIX-SEPARATOR; ip addr\""))
    ).await;
    r.ok().and_then(|r| r.ok()).and_then(|data| {
        let hostname = get_all_before_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let data = get_all_after_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let cpuinfo = get_all_before_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let data = get_all_after_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let meminfo = get_all_before_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let data = get_all_after_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let ipaddr = data;
        Some(ExtendedInfo {
            hostname: hostname.trim().to_owned(),
            cpuinfo: cpuinfo.trim().to_owned(),
            meminfo: meminfo.trim().to_owned(),
            ipaddr: ipaddr.trim().to_owned(),
        })
    })
}

#[tokio::main]
async fn main() {
    // Restore state for all IPs
    let mut states = restore_state().await;
    for ip in generate_ips() {
        states.entry(ip).or_default();
    }

    // Try associating domains to IPs
    let domains = generate_domains();
    let ips = resolve_domains(domains).await;
    println!("{} domains found!", ips.len());
    for (ip, domain) in ips {
        states.entry(ip).or_default().domain = Some(domain);
    }
    
    update_stats(&states).await;
    loop {
        let now = Instant::now();
        update(&mut states).await;
        update_stats(&states).await;
        sleep(Duration::from_secs(600) - now.elapsed()).await;
    }
}
