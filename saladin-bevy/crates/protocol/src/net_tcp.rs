//! TCP lockstep transport + host/join relay (native). Deterministic lockstep
//! needs reliable ordered delivery, which is exactly TCP — no custom UDP
//! reliability layer buys anything at 20 Hz ticks. The relay embeds in the
//! HOSTING client (`spawn_host_relay`), so "Host Game" + "Join by IP" works
//! like any classic RTS with no separate server process (a dedicated
//! `saladin-server` can still run the same relay).
//!
//! Flow: clients connect into a LOBBY — the relay assigns ids and broadcasts
//! the roster as players come and go (first client = host). When the host
//! sends `Start` the roster freezes, everyone gets `Welcome`, and the lockstep
//! phase begins (`Submit`/`Batch` per the `Transport` contract).

use crate::PlayerCommand;
use crate::net::Transport;
use crate::net_ws::{LobbyState, Msg};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

fn encode(m: &Msg) -> Vec<u8> {
    bincode::serialize(m).expect("msg serializes")
}

fn write_msg(s: &mut TcpStream, m: &Msg) -> std::io::Result<()> {
    let bytes = encode(m);
    s.write_all(&(bytes.len() as u32).to_le_bytes())?;
    s.write_all(&bytes)
}

fn read_msg(s: &mut TcpStream) -> std::io::Result<Msg> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let n = u32::from_le_bytes(len) as usize;
    if n > 16 * 1024 * 1024 {
        return Err(std::io::Error::other("oversized frame"));
    }
    let mut buf = vec![0u8; n];
    s.read_exact(&mut buf)?;
    bincode::deserialize(&buf).map_err(|e| std::io::Error::other(e.to_string()))
}

// ── client transport ─────────────────────────────────────────────────────────

#[derive(Default)]
struct ClientState {
    lobby: LobbyState,
    batches: HashMap<u64, Vec<(u64, Vec<PlayerCommand>)>>,
}

/// Client side: a writer handle plus a reader thread that fills lobby state and
/// the batch cache. Send+Sync (TcpStream write half is its own fd clone).
pub struct TcpTransport {
    stream: TcpStream,
    state: Arc<Mutex<ClientState>>,
}

impl TcpTransport {
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true).ok();
        let state: Arc<Mutex<ClientState>> = Arc::default();
        {
            let mut g = state.lock().unwrap();
            g.lobby.connected = true;
        }
        let mut reader = stream.try_clone()?;
        let s2 = state.clone();
        std::thread::spawn(move || {
            loop {
                match read_msg(&mut reader) {
                    Ok(Msg::Roster { you, host, players }) => {
                        let mut g = s2.lock().unwrap();
                        g.lobby.you = you;
                        g.lobby.host = host;
                        g.lobby.players = players;
                    }
                    Ok(Msg::Welcome { players }) => {
                        let mut g = s2.lock().unwrap();
                        g.lobby.players = players;
                        g.lobby.started = true;
                    }
                    Ok(Msg::Batch { tick, entries }) => {
                        s2.lock().unwrap().batches.insert(tick, entries);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let mut g = s2.lock().unwrap();
                        g.lobby.connected = false;
                        g.lobby.error.get_or_insert(e.to_string());
                        return;
                    }
                }
            }
        });
        Ok(TcpTransport { stream, state })
    }

    /// Snapshot of the lobby for the UI. (The reader thread keeps it fresh —
    /// no explicit poll needed; this exists to mirror the ws transport's API.)
    pub fn lobby(&self) -> LobbyState {
        self.state.lock().unwrap().lobby.clone()
    }

    /// Host: freeze the lobby and begin the match.
    pub fn request_start(&mut self) {
        let _ = write_msg(&mut self.stream, &Msg::Start);
    }
}

impl Transport for TcpTransport {
    fn submit(&mut self, tick: u64, player_id: u64, cmds: Vec<PlayerCommand>) {
        let _ = write_msg(&mut self.stream, &Msg::Submit { tick, player_id, cmds });
    }
    fn batch(&mut self, tick: u64) -> Option<Vec<(u64, Vec<PlayerCommand>)>> {
        self.state.lock().unwrap().batches.get(&tick).cloned()
    }
}

// ── relay ────────────────────────────────────────────────────────────────────

struct Shared {
    // each client's outgoing mailbox — its own writer thread drains it
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
    /// Flush every complete tick, in order.
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

fn serve(listener: TcpListener) {
    let shared = Arc::new(Mutex::new(Shared {
        clients: Vec::new(),
        host: 0,
        started: false,
        subs: HashMap::new(),
        next_tick: 0,
    }));
    let mut next_id = 1u64;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if shared.lock().unwrap().started {
            continue; // match running — late joins need the reconnect phase
        }
        stream.set_nodelay(true).ok();
        let id = next_id;
        next_id += 1;
        let (out_tx, out_rx) = mpsc::channel::<Msg>();

        // writer thread: owns the write half, drains the mailbox
        let mut writer = match stream.try_clone() {
            Ok(w) => w,
            Err(_) => continue,
        };
        std::thread::spawn(move || {
            while let Ok(m) = out_rx.recv() {
                if write_msg(&mut writer, &m).is_err() {
                    return;
                }
            }
        });

        {
            let mut g = shared.lock().unwrap();
            if g.clients.is_empty() {
                g.host = id;
            }
            g.clients.push((id, out_tx));
            println!("player {id} joined ({} in lobby)", g.clients.len());
            g.broadcast_roster();
        }

        // reader thread: owns the read half
        let shared2 = shared.clone();
        let mut reader = stream;
        std::thread::spawn(move || {
            loop {
                match read_msg(&mut reader) {
                    Ok(Msg::Start) => {
                        let mut g = shared2.lock().unwrap();
                        if g.host == id && !g.started {
                            g.started = true;
                            let players = g.roster();
                            println!("match started with players {players:?}");
                            g.broadcast(&Msg::Welcome { players });
                        }
                    }
                    Ok(Msg::Submit { tick, player_id, cmds }) => {
                        let mut g = shared2.lock().unwrap();
                        g.subs.entry((tick, player_id)).or_insert(cmds);
                        if g.started {
                            g.flush_batches();
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        let mut g = shared2.lock().unwrap();
                        g.clients.retain(|(cid, _)| *cid != id);
                        println!("player {id} left ({} remain)", g.clients.len());
                        if !g.started {
                            if g.host == id {
                                g.host = g.clients.first().map(|(c, _)| *c).unwrap_or(0);
                            }
                            g.broadcast_roster();
                        } else {
                            // remaining players' ticks complete without them
                            g.flush_batches();
                        }
                        return;
                    }
                }
            }
        });
    }
}

/// Dedicated-server entry: bind and serve forever.
pub fn run_relay(addr: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    println!("relay on {addr} — first client hosts; host starts the match");
    serve(listener);
    Ok(())
}

/// Host-a-game entry: bind, serve on a background thread, return immediately
/// so the hosting client can connect to itself.
pub fn spawn_host_relay(addr: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    std::thread::spawn(move || serve(listener));
    Ok(())
}
