use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::str::FromStr;
use std::time::{Instant, Duration};
use futures::future::select_all;
use string_tools::{get_all_before_strict, get_all_after_strict, get_all_between_strict};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use progress_bar::{global::*, Color, Style};

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

async fn check_ip(ip: Ipv4Addr, load_extended_info: bool) -> (Ipv4Addr, bool, Option<Result<ExtendedInfo, String>>) {
    let r = timeout(Duration::from_secs(4), async move {
        TcpStream::connect(
            &std::net::SocketAddr::new(std::net::IpAddr::V4(ip), 22)
        ).await.is_ok()
    }).await;
    let up = r == Ok(true);
    let extended_info = if load_extended_info && up {
        Some(load_extented_info(ip).await)
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
    candidates.reverse();
    init_progress_bar_with_eta(candidates.len());

    let mut tasks = Vec::new();
    for _ in 0..200 {
        let Some(ip) = candidates.pop() else { break };
        tasks.push(Box::pin(check_ip(ip.0, states.get(&ip.0).unwrap().extended_info.is_none())));
    }

    let mut i = 0;
    while !tasks.is_empty() {
        let ((ip, up, extended_info), _, new_tasks) = select_all(tasks).await;
        tasks = new_tasks;
        if let Some(ip) = candidates.pop() {
            tasks.push(Box::pin(check_ip(ip.0, states.get(&ip.0).unwrap().extended_info.is_none())));
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
            save_state(ip, up, now_utc).await;
        }
        state.up = Some(up);
        if (i % 500) == 0 {
            update_stats(&states).await;
            print_progress_bar_info("Updated", "Stats have been updated", Color::Green, Style::Bold)
        }
        i += 1;
        inc_progress_bar();
    }
    finalize_progress_bar();
}

async fn update_stats(states: &States) {
    let now_utc = now_utc();
    let mut lines = Vec::new();
    for (ip, state) in states {
        let up = state.up.unwrap_or(false);
        let last_change_utc = state.last_change_utc.unwrap_or(0);
        let last_checked_utc = state.last_checked_utc.unwrap_or(0);
        let uptime = state.uptime;
        let downtime = state.downtime;
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
        .open("stats.csv")
        .await
        .expect("Failed to open stats.csv");
    file.write_all(b"ip,up,uptime,downtime,last_change_utc,last_checked_utc,hostname,cpu,mem_kB,swap_kB,mac\n").await.expect("Failed to write to stats.csv");
    file.write_all(lines.join("\n").as_bytes()).await.expect("Failed to write to stats.csv");
}

#[derive(Debug)]
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

async fn load_extented_info(ip: Ipv4Addr) -> Result<ExtendedInfo, String> {
    let r = timeout(
        Duration::from_secs(3),
        run_shell_command(format!("ssh -oBatchMode=yes -oStrictHostKeyChecking=no \"sgirard@{ip}\" \"hostname; echo MUBELOTIX-SEPARATOR; cat /proc/cpuinfo; echo MUBELOTIX-SEPARATOR; cat /proc/meminfo; echo MUBELOTIX-SEPARATOR; ip addr\""))
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
    //let extended_info = load_extented_info(Ipv4Addr::new(172, 29, 4, 250)).await;
    //println!("{:?}", extended_info);

    // Restore state for all IPs
    let mut states = restore_state().await;
    for ip in generate_ips() {
        states.entry(ip).or_default();
    }
    
    update_stats(&states).await;
    loop {
        let now = Instant::now();
        update(&mut states).await;
        update_stats(&states).await;
        sleep(Duration::from_secs(600) - now.elapsed()).await;
    }
}
