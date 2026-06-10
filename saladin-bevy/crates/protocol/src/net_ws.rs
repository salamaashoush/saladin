//! WebSocket lockstep transport + lobby (native AND wasm via `ewebsock`), plus
//! the native relay (`tungstenite`). Replaces the raw-TCP transport so one wire
//! protocol serves desktop and browser clients alike.
//!
//! Flow: clients connect and sit in a LOBBY — the relay assigns ids and
//! broadcasts the roster as players come and go. The first client is the host;
//! when it sends `Start`, the roster freezes, everyone gets `Welcome`, and the
//! lockstep phase begins (`Submit`/`Batch`, same contract as `Transport`).

use crate::PlayerCommand;
use crate::net::Transport;
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone)]
pub enum Msg {
    /// Lobby state push: your id, the host's id, everyone connected.
    Roster { you: u64, host: u64, players: Vec<u64> },
    /// Host → relay: freeze the roster and begin the match.
    Start,
    /// Relay → all: the match begins with this final roster.
    Welcome { players: Vec<u64> },
    Submit { tick: u64, player_id: u64, cmds: Vec<PlayerCommand> },
    Batch { tick: u64, entries: Vec<(u64, Vec<PlayerCommand>)> },
}

fn encode(m: &Msg) -> Vec<u8> {
    bincode::serialize(m).expect("msg serializes")
}

fn decode(bytes: &[u8]) -> Option<Msg> {
    bincode::deserialize(bytes).ok()
}

/// What the lobby/connection currently looks like, for the client UI.
#[derive(Clone, Debug, Default)]
pub struct LobbyState {
    pub connected: bool,
    pub you: u64,
    pub host: u64,
    pub players: Vec<u64>,
    pub started: bool,
    pub error: Option<String>,
}

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
    /// Begin connecting (non-blocking). `addr` may omit the scheme.
    pub fn connect(addr: &str) -> Result<Self, String> {
        let url = if addr.contains("://") { addr.to_string() } else { format!("ws://{addr}") };
        let (tx, rx) = ewebsock::connect(url, ewebsock::Options::default()).map_err(|e| e.to_string())?;
        Ok(WsTransport {
            tx,
            rx,
            open: false,
            outbox: Vec::new(),
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
                WsEvent::Message(WsMessage::Binary(bytes)) => {
                    match decode(&bytes) {
                        Some(Msg::Roster { you, host, players }) => {
                            self.lobby.you = you;
                            self.lobby.host = host;
                            self.lobby.players = players;
                        }
                        Some(Msg::Welcome { players }) => {
                            self.lobby.players = players;
                            self.lobby.started = true;
                        }
                        Some(Msg::Batch { tick, entries }) => {
                            self.batches.insert(tick, entries);
                        }
                        _ => {}
                    }
                }
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
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Arc, Mutex};
    use tungstenite::{Message as TMsg, accept};

    struct Shared {
        // each client's outgoing mailbox — its own socket thread drains it
        clients: Vec<(u64, Sender<Msg>)>,
        host: u64,
        started: bool,
        subs: HashMap<(u64, u64), Vec<PlayerCommand>>,
        next_tick: u64,
    }

    impl Shared {
        fn roster(&self) -> Vec<u64> {
            self.clients.iter().map(|(id, _)| *id).collect()
        }
        fn broadcast(&self, m: &Msg) {
            for (_, tx) in &self.clients {
                let _ = tx.send(m.clone());
            }
        }
        fn broadcast_roster(&self) {
            let players = self.roster();
            for (id, tx) in &self.clients {
                let _ = tx.send(Msg::Roster { you: *id, host: self.host, players: players.clone() });
            }
        }
        /// Flush every complete tick in order.
        fn flush_batches(&mut self) {
            loop {
                let ids = self.roster();
                let mut entries = Vec::with_capacity(ids.len());
                for &p in &ids {
                    match self.subs.get(&(self.next_tick, p)) {
                        Some(c) => entries.push((p, c.clone())),
                        None => return,
                    }
                }
                self.broadcast(&Msg::Batch { tick: self.next_tick, entries });
                self.next_tick += 1;
            }
        }
    }

    /// Run the authoritative websocket relay: lobby (dynamic joins, host
    /// starts) then lockstep batch broadcasting. Each client's socket is owned
    /// by ONE thread that interleaves draining its outgoing mailbox with short
    /// timed reads — no lock is ever held across socket I/O.
    pub fn run_relay_ws(addr: &str) -> std::io::Result<()> {
        let listener = TcpListener::bind(addr)?;
        println!("relay (websocket) on {addr} — first client hosts; host starts the match");
        let shared = Arc::new(Mutex::new(Shared {
            clients: Vec::new(),
            host: 0,
            started: false,
            subs: HashMap::new(),
            next_tick: 0,
        }));

        let mut next_id = 1u64;
        loop {
            let (stream, peer) = listener.accept()?;
            if shared.lock().unwrap().started {
                println!("rejecting {peer}: match already started");
                continue; // dropped — late joins need the reconnect phase
            }
            stream.set_read_timeout(Some(std::time::Duration::from_millis(2))).ok();
            stream.set_nodelay(true).ok();
            let Ok(mut ws) = accept(stream) else { continue };
            let id = next_id;
            next_id += 1;
            let (out_tx, out_rx) = mpsc::channel::<Msg>();
            {
                let mut g = shared.lock().unwrap();
                if g.clients.is_empty() {
                    g.host = id;
                }
                g.clients.push((id, out_tx));
                println!("player {id} joined from {peer} ({} in lobby)", g.clients.len());
                g.broadcast_roster();
            }

            let shared2 = shared.clone();
            std::thread::spawn(move || {
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
                        Ok(TMsg::Binary(bytes)) => match decode(&bytes) {
                            Some(Msg::Start) => {
                                let mut g = shared2.lock().unwrap();
                                if g.host == id && !g.started {
                                    g.started = true;
                                    let players = g.roster();
                                    println!("match started with players {players:?}");
                                    g.broadcast(&Msg::Welcome { players });
                                }
                            }
                            Some(Msg::Submit { tick, player_id, cmds }) => {
                                let mut g = shared2.lock().unwrap();
                                g.subs.entry((tick, player_id)).or_insert(cmds);
                                if g.started {
                                    g.flush_batches();
                                }
                            }
                            _ => {}
                        },
                        Ok(TMsg::Close(_)) | Err(_) => {
                            let mut g = shared2.lock().unwrap();
                            g.clients.retain(|(cid, _)| *cid != id);
                            println!("player {id} left ({} remain)", g.clients.len());
                            if !g.started {
                                if g.host == id {
                                    g.host = g.clients.first().map(|(c, _)| *c).unwrap_or(0);
                                }
                                g.broadcast_roster();
                            } else {
                                // remaining players' ticks can now complete without them
                                g.flush_batches();
                            }
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
