mod clipboard;
mod net;
mod config;
mod mdns;
mod peers;

use anyhow::{anyhow, Result};
use clipboard::{ArboardClipboard, ClipboardBackend};
use std::env;
use std::net::{TcpListener};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use peers::PeerManager;
use config::PeerRecord;
use serde::{Deserialize, Serialize};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PairingInfo {
    device_id: String,
    device_name: String,
    pubkey_b64: String,
}

/// 共享状态：listener 和 watcher 都要访问
struct SharedState {
    /// 用 event_id 去重网络事件（防重放/重复转发）
    rx_event_recent: uniclip_core::RecentSet,
    /// 抑制回环：收到并写入剪贴板后，把 content_hash 放这里，短 TTL
    suppress_content_recent: uniclip_core::RecentSet,
}

fn start_listener(listen_port: u16, shared: Arc<Mutex<SharedState>>, trusted: Arc<std::collections::BTreeMap<String, PeerRecord>>,) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let addr = format!("0.0.0.0:{}", listen_port);
        let listener = TcpListener::bind(&addr).expect("bind listen failed");
        println!("[listen] {}", addr);

        let mut cb = ArboardClipboard::new().expect("clipboard init failed");

        for conn in listener.incoming() {
            let mut stream = match conn {
                Ok(s) => s,
                Err(e) => {
                    println!("[listen accept error] {}", e);
                    continue;
                }
            };

            println!("[peer connected] {}", stream.peer_addr().ok().map(|x| x.to_string()).unwrap_or("?".into()));

            loop {
                let frame = match net::recv_frame(&mut stream) {
                    Ok(f) => f,
                    Err(e) => {
                        println!("[peer disconnected] {}", e);
                        break;
                    }
                };

                let msg = match uniclip_proto::decode(&frame) {
                    Ok(m) => m,
                    Err(e) => {
                        println!("[decode error] {}", e);
                        continue;
                    }
                };

                match msg {
                    uniclip_proto::WireMessage::ClipboardPush { item } => {
                        if !trusted.contains_key(&item.from_device_id) {
                            println!("[DROP] untrusted from={}", item.from_device_id);
                            continue;
                        }
                        // --- 1) event_id 去重（网络层去重）---
                        let should_apply = {
                            let mut st = shared.lock().unwrap();
                            st.rx_event_recent.check_and_remember(&item.event_id)
                        };
                        if !should_apply {
                            continue;
                        }

                        // --- 2) 抑制回环：写入剪贴板前记住 content_hash（短 TTL）---
                        {
                            let mut st = shared.lock().unwrap();
                            st.suppress_content_recent.remember(&item.content_hash);
                        }

                        // --- 3) 应用到本机剪贴板 ---
                        match item.payload {
                            uniclip_proto::ClipboardPayload::Text { text } => {
                                if let Err(e) = cb.set_text(&text) {
                                    println!("[clipboard set error] {}", e);
                                    continue;
                                }
                                println!(
                                    "[APPLIED] event={} hash={} from={}",
                                    item.event_id, item.content_hash, item.from_device_id
                                );
                            }
                        }
                    }
                    uniclip_proto::WireMessage::Hello { version, device } => {
                        println!("[HELLO] v={} device={:?}", version, device);
                    }
                }
            }
        }
    })
}

fn start_watcher(
    shared: Arc<Mutex<SharedState>>,
    peer_mgr: Arc<PeerManager>,
    device_id: String,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        println!("[watcher] polling clipboard text...");

        let mut cb = ArboardClipboard::new().expect("clipboard init failed");
        let mut last_seen_hash: Option<String> = None;

        loop {
            if let Ok(Some(text)) = cb.get_text() {
                // 构造 item（会生成新 event_id + 稳定 content_hash）
                let item = uniclip_core::make_text_item(&device_id, now_ms(), text);

                // 轮询去抖：内容没变就不处理
                if last_seen_hash.as_deref() == Some(&item.content_hash) {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }
                last_seen_hash = Some(item.content_hash.clone());

                // 抑制回环：如果这个 hash 是刚从网络写入的，就跳过一次（短 TTL）
                let suppressed = {
                    let mut st = shared.lock().unwrap();
                    st.suppress_content_recent.contains_fresh(&item.content_hash)
                };
                if suppressed {
                    // 不刷新 suppress 的 TTL（contains_fresh 不会 refresh）
                    // 这样只会抑制短时间内的一次回环，不会影响用户后续再复制同内容
                    println!("[watcher] suppressed hash={}", item.content_hash);
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }

                let msg = uniclip_proto::WireMessage::ClipboardPush { item };
                match uniclip_proto::encode(&msg) {
                    Ok(bytes) => {
                        peer_mgr.broadcast(bytes);
                    }
                    Err(e) => println!("[watcher] encode error: {}", e),
                }
            }

            std::thread::sleep(Duration::from_millis(250));
        }
    })
}

fn usage() -> ! {
    eprintln!("Usage:");
    eprintln!("  uniclip-daemon run <listen_port> [peer_ip:peer_port]");
    eprintln!("  uniclip-daemon show-pairing <listen_port>");
    eprintln!("  uniclip-daemon pair <listen_port> <pairing_json>");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  A: uniclip-daemon show-pairing 7878");
    eprintln!("  B: uniclip-daemon pair 7879 '{{\"device_id\":\"...\",\"device_name\":\"...\",\"pubkey_b64\":\"...\"}}'");
    eprintln!("  A: uniclip-daemon pair 7878 '{{...B...}}'");
    eprintln!("  A: uniclip-daemon run 7878");
    eprintln!("  B: uniclip-daemon run 7879");
    std::process::exit(2);
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        usage();
    }

    let cmd = args[1].as_str();

    match cmd {
        "show-pairing" => {
            if args.len() != 3 {
                usage();
            }
            let listen_port: u16 = args[2].parse().map_err(|_| anyhow!("invalid listen_port"))?;
            let app = config::init_or_create(listen_port)?;

            let info = PairingInfo {
                device_id: app.config.device_id.clone(),
                device_name: app.config.device_name.clone(),
                pubkey_b64: app.identity.public_key_b64(),
            };

            let s = serde_json::to_string(&info)?; // 一行 JSON，方便复制
            println!("{}", s);
            return Ok(());
        }

        "pair" => {
            if args.len() != 4 {
                usage();
            }
            let listen_port: u16 = args[2].parse().map_err(|_| anyhow!("invalid listen_port"))?;
            let pairing_json = &args[3];

            let mut app = config::init_or_create(listen_port)?;
            let info: PairingInfo = serde_json::from_str(pairing_json)
                .map_err(|e| anyhow!("invalid pairing json: {}", e))?;

            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            // 写入 trusted_peers
            app.config.trusted_peers.insert(
                info.device_id.clone(),
                config::PeerRecord {
                    device_name: info.device_name.clone(),
                    pubkey_b64: info.pubkey_b64.clone(),
                    added_at_ms: now_ms,
                },
            );
            app.save_config()?;

            println!(
                "[paired] added device_id={} name={}",
                info.device_id, info.device_name
            );
            return Ok(());
        }

        "run" => {
            let listen_port: u16 = args[2].parse().map_err(|_| anyhow!("invalid listen_port"))?;
            let manual_peer: Option<String> = if args.len() >= 4 { Some(args[3].clone()) } else { None };

            let app = config::init_or_create(listen_port)?;
            println!("[config] device_id={}", app.config.device_id);
            println!("[config] device_name={}", app.config.device_name);
            println!("[config] pubkey_b64={}", app.identity.public_key_b64());
            
            let device_id = app.config.device_id.clone();
            let device_name = app.config.device_name.clone();

            let peer_mgr = Arc::new(PeerManager::new(device_id.clone(), device_name.clone()));

            let shared = Arc::new(Mutex::new(SharedState {
                rx_event_recent: uniclip_core::RecentSet::new(8192, Duration::from_secs(180)),
                suppress_content_recent: uniclip_core::RecentSet::new(2048, Duration::from_secs(2)),
            }));

            let trusted = Arc::new(app.config.trusted_peers.clone());
            let _t_listener = start_listener(listen_port, shared.clone(), trusted.clone());
            let _t_watcher = start_watcher(shared.clone(), peer_mgr.clone(), device_id.clone());

            let mdns = mdns_sd::ServiceDaemon::new()?;
            mdns::advertise(&mdns, &device_id, &device_name, listen_port)?;

            if let Some(addr) = manual_peer {
                // 手动指定 peer
                peer_mgr.add_or_update_peer("manual-peer", addr);
            } else {
                let trusted = app.config.trusted_peers.clone();
                let pm = peer_mgr.clone();
                mdns::browse_peers(&mdns, device_id.clone(), move |addr, peer_id| {
                    if !trusted.contains_key(&peer_id) {
                        // 未配对：忽略（后面可以打印一句提示）
                        return;
                    }
                    println!("[mdns] trusted peer_id={} addr={}", peer_id, addr);
                    pm.add_or_update_peer(&peer_id, addr);
                })?;
                }

            // 主线程不退出
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }

        _ => usage(),
    }
}