use anyhow::{anyhow, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo, ScopedIp};
use std::collections::HashMap;
use std::net::IpAddr;
use std::collections::{HashSet};

pub const SERVICE_TYPE: &str = "_uniclip._tcp.local.";

fn pick_ip_scoped(addresses: &HashSet<ScopedIp>) -> Option<IpAddr> {
    // 优先 IPv4 非 loopback
    for s in addresses.iter() {
        if s.is_ipv4() && !s.is_loopback() {
            return Some(s.to_ip_addr());
        }
    }
    // 退而求其次：任何非 loopback
    for s in addresses.iter() {
        if !s.is_loopback() {
            return Some(s.to_ip_addr());
        }
    }
    None
}

fn make_local_server(device_name: &str) -> String {
    let mut n = device_name.trim().to_string();
    if n.is_empty() {
        n = "device".to_string();
    }
    // avoid user giving "xxx.local" or "xxx.local."
    if n.ends_with(".local.") {
        n
    } else if n.ends_with(".local") {
        format!("{}.", n)
    } else {
        format!("{}.local.", n)
    }
}

/// 广播本机服务（mDNS advertise）
pub fn advertise(
    daemon: &ServiceDaemon,
    device_id: &str,
    device_name: &str,
    listen_port: u16,
) -> Result<()> {
    // instance_name 必须在局域网内尽量唯一
    let inst = format!("{}-{}", device_name, &device_id[..device_id.len().min(8)]);
    
    let host = make_local_server(&device_name);

    // 取本机 IPv4
    let ip = local_ip_address::local_ip()
        .map_err(|e| anyhow!("local_ip: {}", e))?;

    let mut props = HashMap::new();
    props.insert("device_id".to_string(), device_id.to_string());
    props.insert("device_name".to_string(), device_name.to_string());

    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        &inst,
        &host,
        ip,
        listen_port,
        props,
    ).map_err(|e| anyhow!("ServiceInfo::new: {}", e))?;

    daemon.register(service_info)
        .map_err(|e| anyhow!("mdns register: {}", e))?;
    println!("[mdns] advertised {} on {} {}:{}", SERVICE_TYPE, host, ip, listen_port);
    Ok(())
}

/// 浏览局域网内服务（mDNS browse）
pub fn browse_peers<F>(
    daemon: &ServiceDaemon,
    self_device_id: String,
    mut on_peer: F,
) -> Result<()>
where
    F: FnMut(String /*addr*/, String /*peer_id*/) + Send + 'static,
{
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|e| anyhow!("mdns browse: {}", e))?;

    std::thread::spawn(move || {
        println!("[mdns] browse thread started for {}", SERVICE_TYPE);

        while let Ok(event) = receiver.recv() {
            match event {
                ServiceEvent::ServiceFound(ty, fullname) => {
                    println!("[mdns] found: type={} name={}", ty, fullname);
                }
                ServiceEvent::ServiceResolved(info) => {
                    // 从 TXT 里读 device_id
                    let peer_id = info
                        .get_property("device_id")
                        .map(|s| s.to_string())
                        .and_then(|s| {
                            // 把 "device_id=xxxx" 变成 "xxxx"
                            if let Some((k, v)) = s.split_once('=') {
                                if k == "device_id" { return Some(v.to_string()); }
                            }
                            Some(s)
                        })
                        .unwrap_or_default();

                    if peer_id.is_empty() || peer_id == self_device_id {
                        continue; // 排除自己 / 未带 id 的
                    }

                    // 取 IP + port
                    let port = info.get_port();
                    let ip = pick_ip_scoped(&info.addresses);
                    let Some(ip) = ip else { continue; };

                    let addr = format!("{}:{}", ip, port);

                    on_peer(addr, peer_id);
                }
                ServiceEvent::ServiceRemoved(ty, fullname) => {
                    println!("[mdns] removed: ty={} name={}", ty, fullname);
                }
                _ => {}
            }
        }
    });

    Ok(())
}