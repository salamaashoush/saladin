//! WebSocket lockstep transport + lobby (native AND wasm via `ewebsock`), plus
//! the native relay (`tungstenite`). Same wire protocol as the TCP transport
//! (`net_msg::Msg`), kept for a future browser build — the desktop game uses
//! TCP (`net_tcp`), as this transport has a known client-side stall.

use crate::PlayerCommand;
use crate::net::Transport;
use crate::net_msg::{JoinIntent, LobbyState, Msg, PROTOCOL_VERSION, decode, encode};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use std::collections::HashMap;

/// Poll-based WebSocket transport. Works on native and wasm — `ewebsock` does
/// its I/O behind the scenes; we drain its receiver whenever the game polls.
pub struct WsTransport {
    tx: WsSender,
    rx: WsReceiver,
    open: bool,
    outbox: Vec<Msg>,
    pub lobby: LobbyState,
    batches: HashMap<u64, Vec<(u64, Vec<PlayerCommand>)>>,
}

impl WsTransport {
    /// Begin connecting (non-blocking). `addr` may omit the scheme. The
    /// handshake `Hello` is queued and flushed as soon as the socket opens.
    pub fn connect(addr: &str, name: &str, intent: JoinIntent) -> Result<Self, String> {
        let url = if addr.contains("://") { addr.to_string() } else { format!("ws://{addr}") };
        let (tx, rx) = ewebsock::connect(url, ewebsock::Options::default()).map_err(|e| e.to_string())?;
        Ok(WsTransport {
            tx,
            rx,
            open: false,
            outbox: vec![Msg::Hello { version: PROTOCOL_VERSION, name: name.to_string(), intent }],
            lobby: LobbyState::default(),
            batches: HashMap::new(),
        })
    }

    fn send(&mut self, m: Msg) {
        if self.open {
            self.tx.send(WsMessage::Binary(encode(&m)));
        } else {
            self.outbox.push(m);
        }
    }

    /// Drain socket events into lobby state + the batch cache. Call every frame.
    pub fn poll(&mut self) {
        while let Some(ev) = self.rx.try_recv() {
            match ev {
                WsEvent::Opened => {
                    self.open = true;
                    self.lobby.connected = true;
                    for m in std::mem::take(&mut self.outbox) {
                        self.tx.send(WsMessage::Binary(encode(&m)));
                    }
                }
                WsEvent::Message(WsMessage::Binary(bytes)) => match decode(&bytes) {
                    Some(Msg::Batch { tick, entries }) => {
                        self.batches.insert(tick, entries);
                    }
                    Some(m) => self.lobby.apply(&m),
                    None => {}
                },
                WsEvent::Error(e) => self.lobby.error = Some(e),
                WsEvent::Closed => {
                    self.lobby.connected = false;
                    self.lobby.error.get_or_insert_with(|| "connection closed".into());
                }
                _ => {}
            }
        }
    }

    /// Host: freeze the lobby and begin the match.
    pub fn request_start(&mut self) {
        self.send(Msg::Start);
    }
}

impl Transport for WsTransport {
    fn submit(&mut self, tick: u64, player_id: u64, cmds: Vec<PlayerCommand>) {
        self.send(Msg::Submit { tick, player_id, cmds });
    }
    fn batch(&mut self, tick: u64) -> Option<Vec<(u64, Vec<PlayerCommand>)>> {
        self.poll();
        self.batches.get(&tick).cloned()
    }
}

// ── relay (native) ───────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
mod relay {
    use super::*;
    use crate::relay_core::Rooms;
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::mpsc;
    use tungstenite::{Message as TMsg, accept};

    /// Run the websocket relay: same rooms/lobby/lockstep brain as the TCP
    /// relay (`relay_core::Rooms`). Each client's socket is owned by ONE thread
    /// that interleaves draining its outgoing mailbox with short timed reads —
    /// no lock is ever held across socket I/O.
    pub fn run_relay_ws(addr: &str) -> std::io::Result<()> {
        let listener = TcpListener::bind(addr)?;
        println!("relay (websocket) on {addr} — protocol v{PROTOCOL_VERSION}, rooms enabled");
        let rooms: Arc<Rooms> = Arc::default();
        loop {
            let (stream, _peer) = listener.accept()?;
            stream.set_read_timeout(Some(std::time::Duration::from_millis(2))).ok();
            stream.set_nodelay(true).ok();
            let Ok(mut ws) = accept(stream) else { continue };
            let rooms = rooms.clone();
            std::thread::spawn(move || {
                // handshake: first binary frame must be a version-matched Hello
                let hello = loop {
                    match ws.read() {
                        Ok(TMsg::Binary(bytes)) => match decode(&bytes) {
                            Some(m @ Msg::Hello { .. }) => break m,
                            _ => return,
                        },
                        Err(tungstenite::Error::Io(e))
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Ok(TMsg::Close(_)) | Err(_) => return,
                        Ok(_) => {}
                    }
                };
                let (out_tx, out_rx) = mpsc::channel::<Msg>();
                let handle = match rooms.join(&hello, out_tx) {
                    Ok(h) => h,
                    Err(reason) => {
                        let _ = ws.send(TMsg::Binary(encode(&Msg::Reject { reason }).into()));
                        return;
                    }
                };
                loop {
                    // drain the mailbox first so broadcasts never wait on reads
                    while let Ok(m) = out_rx.try_recv() {
                        if ws.send(TMsg::Binary(encode(&m).into())).is_err() {
                            break;
                        }
                    }
                    match ws.read() {
                        Err(tungstenite::Error::Io(e))
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Ok(TMsg::Binary(bytes)) => {
                            if let Some(m) = decode(&bytes) {
                                handle.handle(m);
                            }
                        }
                        Ok(TMsg::Close(_)) | Err(_) => {
                            handle.disconnect();
                            return;
                        }
                        Ok(_) => {}
                    }
                }
            });
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use relay::run_relay_ws;
