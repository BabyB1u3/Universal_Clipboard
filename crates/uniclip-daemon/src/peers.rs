use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::net::TcpStream;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::sync::mpsc::RecvTimeoutError;

use crate::net;

#[derive(Debug)]
enum PeerCmd {
    UpdateAddr(String),
    Send(Vec<u8>),
    Shutdown,
}

struct PeerHandle {
    tx: mpsc::Sender<PeerCmd>,
}

/// 多 peer 管理器：
/// - mDNS 发现时 add_or_update_peer(peer_id, addr)
/// - watcher 产生 payload 时 broadcast(payload)
pub struct PeerManager {
    inner: Arc<Mutex<HashMap<String, PeerHandle>>>,
    device_id: String,
    device_name: String,
}

impl PeerManager {
    pub fn new(device_id: String, device_name: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            device_id,
            device_name,
        }
    }

    /// 新增或更新一个 peer（由 mDNS 回调调用）
    pub fn add_or_update_peer(&self, peer_id: &str, addr: String) {
        // 不连自己（双保险；正常 mDNS 回调处也会过滤）
        if peer_id == self.device_id {
            return;
        }

        let mut map = self.inner.lock().unwrap();

        if let Some(handle) = map.get(peer_id) {
            let _ = handle.tx.send(PeerCmd::UpdateAddr(addr));
            return;
        }

        // 新 peer：起一个 worker
        let (tx, rx) = mpsc::channel::<PeerCmd>();

        let did = self.device_id.clone();
        let dname = self.device_name.clone();
        let pid = peer_id.to_string();

        thread::spawn(move || {
            peer_worker_loop(pid, rx, did, dname, addr);
        });

        map.insert(peer_id.to_string(), PeerHandle { tx });
    }

    /// 广播 payload 给所有 peer（由 watcher 调用）
    pub fn broadcast(&self, payload: Vec<u8>) {
        let map = self.inner.lock().unwrap();
        println!("[peers] broadcast to {} peers", map.len());
        for (peer_id, handle) in map.iter() {
            // 每个 peer 都需要一份 payload（Vec<u8>），这里 clone 是最简单 MVP
            let _ = handle.tx.send(PeerCmd::Send(payload.clone()));
            // 也可以在以后用 Arc<[u8]> 来避免 clone
            let _ = peer_id;
        }
    }
}

/// 每个 peer 一个独立线程：
/// - 维护 addr / stream / pending queue
/// - 断线重连
/// - peer_addr 更新时立即切换
fn peer_worker_loop(
    peer_id: String,
    rx: mpsc::Receiver<PeerCmd>,
    device_id: String,
    device_name: String,
    initial_addr: String,
) {
    let mut peer_addr: Option<String> = Some(initial_addr);
    let mut stream: Option<TcpStream> = None;

    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    const MAX_PENDING: usize = 128;

    fn ensure_connected(
        peer_id: &str,
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

        // 连接建立后发 hello（后面做认证握手时会替换/增强）
        let hello = uniclip_proto::WireMessage::Hello {
            version: uniclip_proto::PROTOCOL_VERSION,
            device: uniclip_proto::DeviceInfo {
                device_id: device_id.to_string(),
                device_name: device_name.to_string(),
            },
        };
        let bytes = uniclip_proto::encode(&hello)?;
        net::send_frame(&mut s, &bytes)?;

        println!("[peer:{}] connected {}", peer_id, peer_addr);
        *stream = Some(s);
        Ok(())
    }

    loop {
        // 1) 尝试收命令，但不要永远阻塞
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PeerCmd::UpdateAddr(addr)) => {
                if peer_addr.as_deref() != Some(addr.as_str()) {
                    println!("[peer:{}] addr update {}", peer_id, addr);
                    peer_addr = Some(addr);
                    stream = None; // 强制重连到新地址
                }
            }
            Ok(PeerCmd::Send(payload)) => {
                if pending.len() >= MAX_PENDING {
                    pending.pop_front();
                }
                pending.push_back(payload);
            }
            Ok(PeerCmd::Shutdown) => {
                println!("[peer:{}] shutdown", peer_id);
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
            // 没有新命令：继续往下走，尝试重连/flush
            }
            Err(RecvTimeoutError::Disconnected) => {
                println!("[peer:{}] channel closed", peer_id);
                break;
            }
        }

        // 2) 没有待发消息，就别白连
        if pending.is_empty() {
            continue;
        }

        // 3) flush pending
        let Some(addr) = peer_addr.clone() else {
            continue;
        };

        if let Err(e) = ensure_connected(&peer_id, &addr, &mut stream, &device_id, &device_name) {
            println!("[peer:{}] connect fail: {} (retry 1s)", peer_id, e);
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        while let Some(payload) = pending.pop_front() {
            if let Some(s) = stream.as_mut() {
                if let Err(e) = net::send_frame(s, &payload) {
                    println!("[peer:{}] send failed: {} (drop connection)", peer_id, e);
                    stream = None;
                    pending.push_front(payload); // 放回去，重连后重发
                    break;
                } else {
                    println!("[SENT peer={}] ClipboardPush", peer_id);
                }
            }
        }
    }
}
