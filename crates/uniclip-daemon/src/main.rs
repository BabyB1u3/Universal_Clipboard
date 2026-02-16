mod clipboard;
mod net;

use anyhow::{anyhow, Result};
use clipboard::{ArboardClipboard, ClipboardBackend};
use std::env;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, SystemTime};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn device_id_mvp() -> &'static str {
    //TODO: 持久化 UUID
    "dev-local-1"
}

fn run_listener(port: u16) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr)?;
    println!("[listen] {}", addr);

    // 用 event_id 去重收到的网络事件（防重复/重放）
    let mut rx_event_recent = uniclip_core::RecentSet::new(4096, Duration::from_secs(120));

    // 用 content_hash 抑制回环（短 TTL 就够）
    let mut suppress_content_recent = uniclip_core::RecentSet::new(1024, Duration::from_secs(2));

    let mut cb = ArboardClipboard::new()?;

    for conn in listener.incoming() {
        let mut stream = conn?;
        println!("[peer connected] {}", stream.peer_addr()?);

        loop {
            let frame = match net::recv_frame(&mut stream) {
                Ok(f) => f,
                Err(e) => { println!("[peer disconnected] {}", e); break; }
            };

            let msg = match uniclip_proto::decode(&frame) {
                Ok(m) => m,
                Err(e) => { println!("[decode error] {}", e); continue; }
            };

            match msg {
                uniclip_proto::WireMessage::ClipboardPush { item } => {
                    // 1) 先用 event_id 去重网络事件
                    if !rx_event_recent.check_and_remember(&item.event_id) {
                        continue;
                    }

                    // 2) 写入剪贴板前，把 content_hash 记入 suppress（断本机回环）
                    suppress_content_recent.remember(&item.content_hash);

                    match item.payload {
                        uniclip_proto::ClipboardPayload::Text { text } => {
                            cb.set_text(&text)?;
                            println!("[APPLIED] event={} hash={} from={}",
                                item.event_id, item.content_hash, item.from_device_id);
                        }
                    }
                }
                uniclip_proto::WireMessage::Hello { version, device } => {
                    println!("[HELLO] v={} device={:?}", version, device);
                }
            }
        }
    }

    Ok(())
}

fn run_sender(target: &str) -> Result<()> {
    let mut stream = TcpStream::connect(target)?;
    stream.set_nodelay(true)?;
    println!("[connect] {}", target);

    // hello 省略

    let mut cb = ArboardClipboard::new()?;

    // 用 content_hash 去抖
    let mut last_seen_content_hash: Option<String> = None;

    println!("[poll] clipboard text changes and push...");

    loop {
        if let Some(text) = cb.get_text()? {
            // make_text_item 会生成新的 event_id
            let item = uniclip_core::make_text_item(device_id_mvp(), now_ms(), text);

            // 轮询去抖只看 content_hash（内容没变就跳过）
            if last_seen_content_hash.as_deref() == Some(&item.content_hash) {
                std::thread::sleep(Duration::from_millis(300));
                continue;
            }
            last_seen_content_hash = Some(item.content_hash.clone());

            let msg = uniclip_proto::WireMessage::ClipboardPush { item };
            let bytes = uniclip_proto::encode(&msg)?;
            net::send_frame(&mut stream, &bytes)?;
            println!("[SENT] ClipboardPush");
        }

        std::thread::sleep(Duration::from_millis(300));
    }
}

fn usage() -> ! {
    eprintln!("Usage:");
    eprintln!("  uniclip-daemon listen <port>");
    eprintln!("  uniclip-daemon connect <ip:port>");
    std::process::exit(2);
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        usage();
    }

    match args[1].as_str() {
        "listen" => {
            let port: u16 = args[2].parse().map_err(|_| anyhow!("invalid port"))?;
            run_listener(port)
        }
        "connect" => run_sender(&args[2]),
        _ => usage(),
    }
}