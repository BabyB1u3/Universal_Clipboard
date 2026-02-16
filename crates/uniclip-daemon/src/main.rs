mod clipboard;
mod net;
mod config;
mod mdns;

use anyhow::{anyhow, Result};
use clipboard::{ArboardClipboard, ClipboardBackend};
use std::env;
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::collections::VecDeque;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

enum SenderCmd {
    SetPeer(String),
    Send(Vec<u8>),
}

/// 共享状态：listener 和 watcher 都要访问
struct SharedState {
    /// 用 event_id 去重网络事件（防重放/重复转发）
    rx_event_recent: uniclip_core::RecentSet,
    /// 抑制回环：收到并写入剪贴板后，把 content_hash 放这里，短 TTL
    suppress_content_recent: uniclip_core::RecentSet,
}

fn start_listener(listen_port: u16, shared: Arc<Mutex<SharedState>>) -> std::thread::JoinHandle<()> {
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

fn start_sender(
    rx: mpsc::Receiver<SenderCmd>,
    device_id: String,
    device_name: String,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut peer_addr: Option<String> = None;
        let mut stream: Option<TcpStream> = None;

        // 缓存少量待发送消息（peer 未就绪或断线时）
        let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
        const MAX_PENDING: usize = 64;

        fn ensure_connected(
            peer_addr: &str,
            stream: &mut Option<TcpStream>,
            device_id: &str,
            device_name: &str,
        ) -> Result<()> {
            if stream.is_some() {
                return Ok(());
            }

            let mut s = TcpStream::connect(peer_addr)?;
            s.set_nodelay(true).ok();

            let hello = uniclip_proto::WireMessage::Hello {
                version: uniclip_proto::PROTOCOL_VERSION,
                device: uniclip_proto::DeviceInfo {
                    device_id: device_id.to_string(),
                    device_name: device_name.to_string(),
                },
            };
            let bytes = uniclip_proto::encode(&hello)?;
            net::send_frame(&mut s, &bytes)?;

            println!("[sender] connected to {}", peer_addr);
            *stream = Some(s);
            Ok(())
        }

        loop {
            match rx.recv() {
                Ok(SenderCmd::SetPeer(addr)) => {
                    if peer_addr.as_deref() != Some(addr.as_str()) {
                        println!("[sender] peer set to {}", addr);
                        peer_addr = Some(addr);
                        stream = None; // 强制用新 peer 重连
                    }
                }
                Ok(SenderCmd::Send(payload)) => {
                    if pending.len() >= MAX_PENDING {
                        pending.pop_front();
                    }
                    pending.push_back(payload);
                }
                Err(_) => {
                    println!("[sender] channel closed, exit");
                    break;
                }
            }

            // 尝试 flush pending
            let Some(addr) = peer_addr.clone() else {
                continue; // 还没发现 peer
            };

            if let Err(e) = ensure_connected(&addr, &mut stream, &device_id, &device_name) {
                println!("[sender] {} (retry in 1s)", e);
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }

            while let Some(payload) = pending.pop_front() {
                if let Some(s) = stream.as_mut() {
                    if let Err(e) = net::send_frame(s, &payload) {
                        println!("[sender] send failed: {} (drop connection)", e);
                        stream = None;
                        // 发送失败把这条塞回去，下次重连再发
                        pending.push_front(payload);
                        break;
                    } else {
                        println!("[SENT] ClipboardPush");
                    }
                }
            }
        }
    })
}

fn start_watcher(
    shared: Arc<Mutex<SharedState>>,
    tx: mpsc::Sender<SenderCmd>,
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
                        let _ = tx.send(SenderCmd::Send(bytes));
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
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  Auto-discovery:");
    eprintln!("    uniclip-daemon run 7878");
    eprintln!("  Manual peer (debug):");
    eprintln!("    uniclip-daemon run 7878 127.0.0.1:7879");
    std::process::exit(2);
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if (args.len() != 3 && args.len() != 4) || args[1] != "run" {
        usage();
    }

    let listen_port: u16 = args[2].parse().map_err(|_| anyhow!("invalid listen_port"))?;
    let manual_peer: Option<String> = if args.len() >= 4 { Some(args[3].clone()) } else { None };

    let app = config::init_or_create(listen_port)?;
    println!("[config] dir={}", app.dir.display());
    println!("[config] device_id={}", app.config.device_id);
    println!("[config] device_name={}", app.config.device_name);
    println!("[config] pubkey_b64={}", app.identity.public_key_b64());
    println!("[config] config_path={}", app.config_path.display());
    println!("[config] key_path={}", app.key_path.display());

    let (cmd_tx, cmd_rx) = mpsc::channel::<SenderCmd>();

    let device_id = app.config.device_id.clone();
    let device_name = app.config.device_name.clone();

    let shared = Arc::new(Mutex::new(SharedState {
        rx_event_recent: uniclip_core::RecentSet::new(8192, Duration::from_secs(180)),
        suppress_content_recent: uniclip_core::RecentSet::new(2048, Duration::from_secs(2)),
    }));

    let _t_listener = start_listener(listen_port, shared.clone());
    let _t_sender = start_sender(cmd_rx, device_id.clone(), device_name.clone());
    let _t_watcher = start_watcher(shared.clone(), cmd_tx.clone(), device_id.clone());

    let mdns = mdns_sd::ServiceDaemon::new()?;
    mdns::advertise(&mdns, &device_id, &device_name, listen_port)?;

    if let Some(addr) = manual_peer {
        // 手动指定 peer（调试用）
        let _ = cmd_tx.send(SenderCmd::SetPeer(addr));
    } else {
        // 自动发现第一个 peer
        mdns::browse_first_peer(&mdns, device_id.clone(), move |addr, peer_id| {
            println!("[mdns] found peer {} ({})", addr, peer_id);
            let _ = cmd_tx.send(SenderCmd::SetPeer(addr));
        })?;
    }

    // 主线程不退出
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}