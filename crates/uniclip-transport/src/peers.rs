use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::net::TcpStream;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::net;
use crate::session::{PeerSessionSnapshot, SessionState};

#[derive(Debug)]
enum PeerCmd {
    UpdateAddr(String),
    Send(Vec<u8>),
    Shutdown,
}

struct PeerHandle {
    tx: mpsc::Sender<PeerCmd>,
    state: Arc<Mutex<SessionState>>,
    addr: Arc<Mutex<Option<String>>>,
    backoff_until: Arc<Mutex<Option<Instant>>>,
}

/// 多 peer 管理器：
/// - mDNS 发现时 add_or_update_peer(peer_id, addr)
/// - watcher 产生 payload 时 broadcast(payload)
///
/// 注意：当前这里暴露的是“outbound session”状态，
/// 不是 inbound listener 的连接状态。
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
        if peer_id == self.device_id {
            return;
        }

        let mut map = self.inner.lock().unwrap();

        if let Some(handle) = map.get(peer_id) {
            {
                let mut addr_slot = handle.addr.lock().unwrap();
                *addr_slot = Some(addr.clone());
            }
            let _ = handle.tx.send(PeerCmd::UpdateAddr(addr));
            return;
        }

        let (tx, rx) = mpsc::channel::<PeerCmd>();

        let did = self.device_id.clone();
        let dname = self.device_name.clone();
        let pid = peer_id.to_string();

        let state = Arc::new(Mutex::new(SessionState::Disconnected));
        let state_for_worker = state.clone();

        let current_addr = Arc::new(Mutex::new(Some(addr.clone())));
        let addr_for_worker = current_addr.clone();

        let backoff_until = Arc::new(Mutex::new(None));
        let backoff_for_worker = backoff_until.clone();

        thread::spawn(move || {
            peer_worker_loop(
                pid,
                rx,
                did,
                dname,
                addr,
                state_for_worker,
                addr_for_worker,
                backoff_for_worker,
            );
        });

        map.insert(
            peer_id.to_string(),
            PeerHandle {
                tx,
                state,
                addr: current_addr,
                backoff_until,
            },
        );
    }

    pub fn broadcast(&self, payload: Vec<u8>) {
        let map = self.inner.lock().unwrap();
        println!("[peers] broadcast to {} peers", map.len());
        for (_peer_id, handle) in map.iter() {
            let _ = handle.tx.send(PeerCmd::Send(payload.clone()));
        }
    }

    pub fn session_state(&self, peer_id: &str) -> Option<SessionState> {
        let map = self.inner.lock().unwrap();
        let handle = map.get(peer_id)?;
        Some(*handle.state.lock().unwrap())
    }

    pub fn list_sessions(&self) -> Vec<PeerSessionSnapshot> {
        let map = self.inner.lock().unwrap();
        let now = Instant::now();

        map.iter()
            .map(|(peer_id, handle)| {
                let retry_in_ms = handle
                    .backoff_until
                    .lock()
                    .unwrap()
                    .map(|deadline| {
                        if deadline > now {
                            deadline.duration_since(now).as_millis() as u64
                        } else {
                            0
                        }
                    });

                PeerSessionSnapshot {
                    peer_id: peer_id.clone(),
                    addr: handle.addr.lock().unwrap().clone(),
                    state: *handle.state.lock().unwrap(),
                    retry_in_ms,
                }
            })
            .collect()
    }

    pub fn shutdown_all(&self) {
        let map = self.inner.lock().unwrap();
        for (_peer_id, handle) in map.iter() {
            let _ = handle.tx.send(PeerCmd::Shutdown);
        }
    }
}

fn set_state(peer_id: &str, state: &Arc<Mutex<SessionState>>, new_state: SessionState) {
    let mut s = state.lock().unwrap();
    if *s != new_state {
        println!("[peer:{}] state -> {:?}", peer_id, new_state);
        *s = new_state;
    }
}

fn set_backoff(backoff_until: &Arc<Mutex<Option<Instant>>>, until: Option<Instant>) {
    let mut slot = backoff_until.lock().unwrap();
    *slot = until;
}

fn in_backoff(backoff_until: &Arc<Mutex<Option<Instant>>>) -> bool {
    let now = Instant::now();
    let deadline = *backoff_until.lock().unwrap();
    matches!(deadline, Some(t) if t > now)
}

fn clear_backoff(backoff_until: &Arc<Mutex<Option<Instant>>>) {
    set_backoff(backoff_until, None);
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
    state: Arc<Mutex<SessionState>>,
    shared_addr: Arc<Mutex<Option<String>>>,
    backoff_until: Arc<Mutex<Option<Instant>>>,
) {
    let mut peer_addr: Option<String> = Some(initial_addr);
    let mut stream: Option<TcpStream> = None;

    let mut pending: VecDeque<Vec<u8>> = VecDeque::new();
    const MAX_PENDING: usize = 128;
    const BACKOFF_SECS: u64 = 1;

    fn ensure_connected(
        peer_id: &str,
        peer_addr: &str,
        stream: &mut Option<TcpStream>,
        device_id: &str,
        device_name: &str,
        state: &Arc<Mutex<SessionState>>,
    ) -> Result<()> {
        if stream.is_some() {
            return Ok(());
        }

        set_state(peer_id, state, SessionState::Connecting);

        let mut s = TcpStream::connect(peer_addr)?;
        s.set_nodelay(true).ok();

        let hello = uniclip_proto::WireMessage::Hello {
            version: uniclip_proto::PROTOCOL_VERSION,
            device: uniclip_core::DeviceInfo {
                device_id: device_id.to_string(),
                device_name: device_name.to_string(),
            },
        };
        let bytes = uniclip_proto::encode(&hello)?;
        net::send_frame(&mut s, &bytes)?;

        println!("[peer:{}] connected {}", peer_id, peer_addr);
        *stream = Some(s);
        set_state(peer_id, state, SessionState::Connected);
        Ok(())
    }

    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(PeerCmd::UpdateAddr(addr)) => {
                if peer_addr.as_deref() != Some(addr.as_str()) {
                    println!("[peer:{}] addr update {}", peer_id, addr);
                    peer_addr = Some(addr.clone());
                    {
                        let mut slot = shared_addr.lock().unwrap();
                        *slot = Some(addr);
                    }
                    stream = None;
                    clear_backoff(&backoff_until);
                    set_state(&peer_id, &state, SessionState::Disconnected);
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
                clear_backoff(&backoff_until);
                set_state(&peer_id, &state, SessionState::Disconnected);
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                println!("[peer:{}] channel closed", peer_id);
                clear_backoff(&backoff_until);
                set_state(&peer_id, &state, SessionState::Disconnected);
                break;
            }
        }

        if pending.is_empty() {
            // 没有待发任务时，如果不在连接中，也尽量回到 Disconnected
            if stream.is_none() && !in_backoff(&backoff_until) {
                set_state(&peer_id, &state, SessionState::Disconnected);
            }
            continue;
        }

        if in_backoff(&backoff_until) {
            set_state(&peer_id, &state, SessionState::Backoff);
            continue;
        }

        let Some(addr) = peer_addr.clone() else {
            clear_backoff(&backoff_until);
            set_state(&peer_id, &state, SessionState::Disconnected);
            continue;
        };

        if let Err(e) = ensure_connected(
            &peer_id,
            &addr,
            &mut stream,
            &device_id,
            &device_name,
            &state,
        ) {
            println!(
                "[peer:{}] connect fail: {} (retry {}s)",
                peer_id, e, BACKOFF_SECS
            );
            let until = Instant::now() + Duration::from_secs(BACKOFF_SECS);
            set_backoff(&backoff_until, Some(until));
            set_state(&peer_id, &state, SessionState::Backoff);
            continue;
        }

        clear_backoff(&backoff_until);

        while let Some(payload) = pending.pop_front() {
            if let Some(s) = stream.as_mut() {
                if let Err(e) = net::send_frame(s, &payload) {
                    println!("[peer:{}] send failed: {} (drop connection)", peer_id, e);
                    stream = None;
                    let until = Instant::now() + Duration::from_secs(BACKOFF_SECS);
                    set_backoff(&backoff_until, Some(until));
                    set_state(&peer_id, &state, SessionState::Backoff);
                    pending.push_front(payload);
                    break;
                } else {
                    println!("[SENT peer={}] ClipboardPush", peer_id);
                }
            }
        }
    }
}