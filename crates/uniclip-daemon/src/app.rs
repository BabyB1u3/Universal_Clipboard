use anyhow::{anyhow, Result};
use std::env;
use std::net::{TcpListener};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::clipboard::{ArboardClipboard, ClipboardBackend};
use crate::pairing_service::PairingService;

use uniclip_core::{make_text_item, ClipboardPayload, RecentSet};
use uniclip_discovery::mdns;
use uniclip_store::{TrustStore, PeerRecord};
use uniclip_transport::{recv_frame, PeerManager};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// 共享状态：listener 和 watcher 都要访问
struct SharedState {
    /// 用 event_id 去重网络事件（防重放/重复转发）
    rx_event_recent: uniclip_core::RecentSet,
    /// 抑制回环：收到并写入剪贴板后，把 content_hash 放这里，短 TTL
    suppress_content_recent: uniclip_core::RecentSet,
}

fn start_listener(
    listen_port: u16,
    shared: Arc<Mutex<SharedState>>,
    trusted: Arc<std::collections::BTreeMap<String, PeerRecord>>,
) -> std::thread::JoinHandle<()> {
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

            println!("[peer connected] {}",
                    stream
                        .peer_addr()
                        .ok()
                        .map(|x| x.to_string())
                        .unwrap_or("?".into())
            );

            loop {
                let frame = match recv_frame(&mut stream) {
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
                            ClipboardPayload::Text { text } => {
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
                let item = make_text_item(&device_id, now_ms(), text);

                // 轮询去抖：内容没变就不处理
                if last_seen_hash.as_deref() == Some(&item.content_hash) {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }
                last_seen_hash = Some(item.content_hash.clone());

                println!("[watcher] new content_hash={} (will broadcast)", item.content_hash);

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

fn cmd_show_pairing(listen_port: u16) -> Result<()> {
    let store = Arc::new(Mutex::new(TrustStore::load(listen_port)?));
    let pairing = PairingService::new(store);

    let s = pairing.export_pairing_json()?;
    println!("{}", s);
    Ok(())
}

fn cmd_pair(listen_port: u16, pairing_json: &str) -> Result<()> {
    let store = Arc::new(Mutex::new(TrustStore::load(listen_port)?));
    let pairing = PairingService::new(store);

    let info = pairing.import_pairing_json(pairing_json)?;

    println!(
        "[paired] added device_id={} name={}",
        info.device_id, info.device_name
    );
    Ok(())
}

fn cmd_run(listen_port: u16, manual_peer: Option<String>) -> Result<()> {
    let store = TrustStore::load(listen_port)?;

    println!("[config] dir={}", store.config_dir().display());
    println!("[config] device_id={}", store.device_id());
    println!("[config] device_name={}", store.device_name());
    println!("[config] pubkey_b64={}", store.public_key_b64());

    let device_id = store.device_id().to_string();
    let device_name = store.device_name().to_string();

    let peer_mgr = Arc::new(PeerManager::new(device_id.clone(), device_name.clone()));

    let shared = Arc::new(Mutex::new(SharedState {
        rx_event_recent: RecentSet::new(8192, Duration::from_secs(180)),
        suppress_content_recent: RecentSet::new(2048, Duration::from_secs(2)),
    }));

    let trusted = Arc::new(store.trusted_peers().clone());
    let _t_listener = start_listener(listen_port, shared.clone(), trusted.clone());
    let _t_watcher = start_watcher(shared.clone(), peer_mgr.clone(), device_id.clone());

    let mdns_daemon = mdns_sd::ServiceDaemon::new()?;
    mdns::advertise(&mdns_daemon, &device_id, &device_name, listen_port)?;

    if let Some(addr) = manual_peer {
        // 手动指定 peer
        peer_mgr.add_or_update_peer("manual-peer", addr);
    } else {
        let trusted = store.trusted_peers().clone();
        let pm = peer_mgr.clone();

        mdns::browse_peers(&mdns_daemon, device_id.clone(), move |addr, peer_id| {
            if !trusted.contains_key(&peer_id) {
                println!(
                    "[mdns] discovered UNTRUSTED peer_id={} addr={} (ignored)",
                    peer_id, addr
                );
                println!(
                    "[mdns] trusted keys: {:?}",
                    trusted.keys().collect::<Vec<_>>()
                );
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

pub fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        usage();
    }

    match args[1].as_str() {
        "show-pairing" => {
            if args.len() != 3 {
                usage();
            }
            let listen_port: u16 =
                args[2].parse().map_err(|_| anyhow!("invalid listen_port"))?;
            cmd_show_pairing(listen_port)
        }

        "pair" => {
            if args.len() != 4 {
                usage();
            }
            let listen_port: u16 =
                args[2].parse().map_err(|_| anyhow!("invalid listen_port"))?;
            cmd_pair(listen_port, &args[3])
        }

        "run" => {
            if args.len() < 3 {
                usage();
            }
            let listen_port: u16 =
                args[2].parse().map_err(|_| anyhow!("invalid listen_port"))?;
            let manual_peer = if args.len() >= 4 {
                Some(args[3].clone())
            } else {
                None
            };
            cmd_run(listen_port, manual_peer)
        }

        _ => usage(),
    }
}