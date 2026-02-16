use anyhow::{anyhow, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use std::net::IpAddr;

pub const SERVICE_TYPE: &str = "_uniclip._tcp.local.";

fn pick_ip(addrs: impl Iterator<Item = IpAddr>) -> Option<IpAddr> {
    // 优先选 IPv4 且非 loopback
    let mut v4 = None;
    let mut other = None;
    for ip in addrs {
        if ip.is_loopback() {
            continue;
        }
        if ip.is_ipv4() && v4.is_none() {
            v4 = Some(ip);
        } else if other.is_none() {
            other = Some(ip);
        }
    }
    v4.or(other)
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
    let host = "uniclip.local."; // MVP：写死也能工作（mdns-sd 会处理）
    let ip = IpAddr::from([0, 0, 0, 0]); // 让系统选择；有些版本会忽略这个值

    let mut props = HashMap::new();
    props.insert("device_id".to_string(), device_id.to_string());
    props.insert("device_name".to_string(), device_name.to_string());

    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        &inst,
        host,
        ip,
        listen_port,
        props,
    ).map_err(|e| anyhow!("ServiceInfo::new: {}", e))?;

    daemon.register(service_info)
        .map_err(|e| anyhow!("mdns register: {}", e))?;
    Ok(())
}

/// 浏览局域网内服务（mDNS browse）
/// 找到第一个“不是自己”的 peer 后，回调 on_peer(addr, peer_device_id)
pub fn browse_first_peer<F>(
    daemon: &ServiceDaemon,
    self_device_id: String,
    mut on_peer: F,
) -> Result<()>
where
    F: FnMut(String, String) + Send + 'static,
{
    let receiver = daemon.browse(SERVICE_TYPE)
        .map_err(|e| anyhow!("mdns browse: {}", e))?;

    std::thread::spawn(move || {
        for event in receiver {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    // 从 TXT 里读 device_id
                    let peer_id = info
                        .get_property("device_id")
                        .map(|s| s.to_string())
                        .unwrap_or_default();

                    if peer_id.is_empty() || peer_id == self_device_id {
                        continue; // 排除自己 / 未带 id 的
                    }

                    // 取 IP + port
                    let port = info.get_port();
                    let ip = pick_ip(info.get_addresses().iter().copied());
                    let Some(ip) = ip else { continue; };

                    let addr = format!("{}:{}", ip, port);
                    on_peer(addr, peer_id);

                    // “单 peer”策略：找到第一个就不再继续（你要多 peer 时再改）
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(())
}
