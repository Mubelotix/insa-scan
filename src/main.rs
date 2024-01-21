use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::str::FromStr;
use std::time::{Instant, Duration};
use futures::future::select_all;
use string_tools::{get_all_before_strict, get_all_after_strict, get_all_between_strict};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use progress_bar::{global::*, Color, Style};
use serde::{Serialize, Deserialize};

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

fn is_blacklisted(ip: Ipv4Addr) -> bool {
    ip.octets()[2] == 0 || ip.octets()[2] == 33 || ip == Ipv4Addr::new(172, 29, 4, 250)
}

fn generate_ips() -> HashSet<Ipv4Addr> {
    let mut ips = HashSet::new();
    for i in 0..=255 {
        for j in 0..=255 {
            let ip = Ipv4Addr::new(172, 29, i, j);
            ips.insert(ip);
        }
    }
    ips
}

type States = HashMap<Ipv4Addr, MachineState>;

fn now_utc() -> u64 {
    chrono::Utc::now().timestamp() as u64
}

async fn restore_state(data_dir: &str) -> States {
    let file = match tokio::fs::read_to_string(format!("{data_dir}/states.bin")).await {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return States::new(),
        Err(e) => panic!("Failed to open states.bin: {}", e),
    };

    bincode::deserialize_from(file.as_bytes()).expect("Failed to deserialize states.bin")
}

async fn save_states(states: &States, data_dir: &str) {
    let file = bincode::serialize(states).expect("Failed to serialize states");
    tokio::fs::write(format!("{data_dir}/states.bin"), file).await.expect("Failed to write states.bin");
}

async fn check_ip(ip: Ipv4Addr, was_up: bool, data_dir: &str) -> (Ipv4Addr, bool, Option<Result<ExtendedInfo, String>>) {
    let time_to_wait = match was_up {
        true => Duration::from_secs(12),
        false => Duration::from_secs(4),
    };
    let r = timeout(time_to_wait, async move {
        TcpStream::connect(
            &std::net::SocketAddr::new(std::net::IpAddr::V4(ip), 22)
        ).await.is_ok()
    }).await;
    let up = r == Ok(true);
    let extended_info = if !was_up && up {
        Some(load_extented_info(ip, data_dir).await)
    } else {
        None
    };
    (ip, up, extended_info)
}

async fn update(states: &mut States, data_dir: &str) {
    let mut candidates: Vec<(Ipv4Addr, bool, u64)> = states.iter().map(|(ip, state)| {
        (*ip, state.up(), state.last_checked())
    }).collect();
    candidates.sort_by(|(_, up1, t1), (_, up2, t2)| {
        up1.cmp(up2).reverse().then(t1.cmp(t2))
    });
    candidates.truncate((255*255)/6);
    candidates.reverse();
    init_progress_bar_with_eta(candidates.len());

    let mut tasks = Vec::new();
    for _ in 0..200 {
        let Some(ip) = candidates.pop() else { break };
        tasks.push(Box::pin(check_ip(ip.0, states.get(&ip.0).unwrap().up(), data_dir)));
    }

    let mut i = 0;
    while !tasks.is_empty() {
        let ((ip, up, extended_info), _, new_tasks) = select_all(tasks).await;
        tasks = new_tasks;
        if let Some(ip) = candidates.pop() {
            tasks.push(Box::pin(check_ip(ip.0, states.get(&ip.0).unwrap().up(), data_dir)));
        }
        let now_utc = now_utc();
        let state = states.entry(ip).or_default();
        match extended_info {
            Some(Ok(extended_info)) => state.extended_info = Some(extended_info),
            Some(Err(err)) => {
                print_progress_bar_info("Failed", &format!("to load extended info for {ip}: {err:?}"), Color::Yellow, Style::Bold);
                state.extended_info = None;
            },
            None => (),
        }
        state.checked(up, now_utc);
        if (i % 500) == 0 {
            update_stats(&states, data_dir).await;
            save_states(&states, data_dir).await;
            print_progress_bar_info("Updated", "Stats have been updated", Color::Green, Style::Bold)
        }
        i += 1;
        inc_progress_bar();
    }
    finalize_progress_bar();
}

async fn update_stats(states: &States, data_dir: &str) {
    let now_utc = now_utc();
    let mut lines = Vec::new();
    for (ip, state) in states {
        let last_change_utc = state.last_change();
        let last_checked_utc = state.last_checked();
        let (up, uptime, downtime) = state.times_since(now_utc - 30*86400);
        if uptime == 0 && !up {
            continue;
        }
        let hostname = state.extended_info.as_ref().map(|info| info.hostname.as_str()).unwrap_or("");
        let cpu = state.extended_info.as_ref().and_then(|info| info.cpu()).unwrap_or("");
        let mem = state.extended_info.as_ref().and_then(|info| info.mem()).unwrap_or("");
        let swap = state.extended_info.as_ref().and_then(|info| info.swap()).unwrap_or("");
        let mac = state.extended_info.as_ref().and_then(|info| info.mac()).unwrap_or("");
        lines.push(format!("{ip},{up},{uptime},{downtime},{last_change_utc},{last_checked_utc},{hostname},{cpu},{mem},{swap},{mac}"));
    }
    lines.sort();
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(format!("{data_dir}/stats.csv"))
        .await
        .expect("Failed to open stats.csv");
    file.write_all(b"ip,up,uptime,downtime,last_change_utc,last_checked_utc,hostname,cpu,mem_kB,swap_kB,mac\n").await.expect("Failed to write to stats.csv");
    file.write_all(lines.join("\n").as_bytes()).await.expect("Failed to write to stats.csv");
}

#[derive(Debug, Serialize, Deserialize)]
struct ExtendedInfo {
    hostname: String,
    cpuinfo: String,
    meminfo: String,
    ipaddr: String,
}

impl ExtendedInfo {
    pub fn cpu(&self) -> Option<&str> {
        get_all_between_strict(&self.cpuinfo, "model name	: ", "\n")
    }

    pub fn mem(&self) -> Option<&str> {
        get_all_between_strict(&self.meminfo, "MemTotal:", " kB").map(|s| s.trim())
    }

    pub fn swap(&self) -> Option<&str> {
        get_all_between_strict(&self.meminfo, "SwapTotal:", " kB").map(|s| s.trim())
    }

    pub fn mac(&self) -> Option<&str> {
        get_all_between_strict(&self.ipaddr, "    link/ether ", " brd").map(|s| s.trim())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MachineState {
    /// First item is the time it was scanned the first time
    /// At this point it's considered up
    changes: Vec<u64>,
    last_checked: u64,
    pub extended_info: Option<ExtendedInfo>,
}

impl MachineState {
    pub fn checked(&mut self, up: bool, now_utc: u64) {
        if up != self.up() {
            self.changes.push(now_utc);
        } else if !up && self.changes.is_empty() {
            self.changes.push(now_utc);
            self.changes.push(now_utc);
        }
        self.last_checked = now_utc;
    }

    pub fn up(&self) -> bool {
        self.changes.len() % 2 == 1
    }

    pub fn last_change(&self) -> u64 {
        self.changes.last().copied().unwrap_or_else(|| now_utc())
    }

    pub fn last_checked(&self) -> u64 {
        self.last_checked
    }

    pub fn times_since(&self, since: u64) -> (bool, u64, u64) {
        if self.changes.is_empty() {
            return (false, 0, 0);
        }
        let mut uptime = 0;
        let mut downtime = 0;
        let mut up = true;
        for i in 1..self.changes.len() {
            up = !up;
            if self.changes[i] < since {
                continue;
            }
            let segment = self.changes[i] - std::cmp::max(self.changes[i-1], since);
            if up {
                uptime += segment;
            } else {
                downtime += segment;
            }
        }
        (up, uptime, downtime)
    }
}

async fn load_extented_info(ip: Ipv4Addr, data_dir: &str) -> Result<ExtendedInfo, String> {
    let r = timeout(
        Duration::from_secs(3),
        run_shell_command(format!("ssh -i {data_dir}/ssh-key -oBatchMode=yes -oStrictHostKeyChecking=no \"sgirard@{ip}\" \"hostname; echo MUBELOTIX-SEPARATOR; cat /proc/cpuinfo; echo MUBELOTIX-SEPARATOR; cat /proc/meminfo; echo MUBELOTIX-SEPARATOR; ip addr\""))
    ).await;
    let r = match r {
        Ok(r) => r?,
        Err(_) => return Err(String::from("Timeout")),
    };
    
    let inner = |data: &str| -> Option<ExtendedInfo> {
        let hostname = get_all_before_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let data = get_all_after_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let cpuinfo = get_all_before_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let data = get_all_after_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let meminfo = get_all_before_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let data = get_all_after_strict(&data, "MUBELOTIX-SEPARATOR")?;
        let ipaddr = data;
        Some(ExtendedInfo {
            hostname: hostname.trim().replace(",", " "),
            cpuinfo: cpuinfo.trim().replace(",", " "),
            meminfo: meminfo.trim().replace(",", " "),
            ipaddr: ipaddr.trim().replace(",", " "),
        })
    };

    inner(&r).ok_or_else(|| String::from("Invalid response"))
}

#[tokio::main]
async fn main() {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| String::from("."));

    //let extended_info = load_extented_info(Ipv4Addr::new(172, 29, 4, 250)).await;
    //println!("{:?}", extended_info);

    // Restore state for all IPs
    let mut states = restore_state(&data_dir).await;
    for ip in generate_ips() {
        states.entry(ip).or_default();
    }

    states.retain(|ip, _| !is_blacklisted(*ip));
    
    update_stats(&states, &data_dir).await;
    loop {
        let now = Instant::now();
        update(&mut states, &data_dir).await;
        update_stats(&states, &data_dir).await;
        sleep(Duration::from_secs(600) - now.elapsed()).await;
    }
}
