//! TCP lockstep transport + relay (native). Deterministic lockstep needs
//! reliable ordered delivery, which is exactly TCP — no custom UDP reliability
//! layer buys anything at 20 Hz ticks. The relay embeds in the HOSTING client
//! for LAN play (`spawn_host_relay`) and runs standalone on a VPS for internet
//! play (`saladin-server`): both peers connect OUTBOUND, so rooms traverse NAT
//! with zero configuration.
//!
//! Flow: a client's first frame is `Hello { version, name, intent }` — the
//! relay verifies the protocol version and resolves the intent (default room
//! for LAN, create/join a coded room for internet), replying `Reject` on any
//! failure. Then the client sits in the room's LOBBY (`Roster` pushes carry
//! names/factions/ready flags and the host's map pick) until the host sends
//! `Start`; everyone gets `Welcome { seed, preset, players }` and the lockstep
//! phase begins (`Submit`/`Batch` per the `Transport` contract).

use crate::PlayerCommand;
use crate::net::Transport;
use crate::net_msg::{JoinIntent, LobbyState, Msg, PROTOCOL_VERSION};
use crate::relay_core::Rooms;
use saladin_sim::{AiDifficulty, Faction};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn encode(m: &Msg) -> Vec<u8> {
    crate::net_msg::encode(m)
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
    events: Vec<crate::net::NetEvent>,
}

/// Client side: a writer handle plus a reader thread that fills lobby state and
/// the batch cache. Send+Sync (TcpStream write half is its own fd clone).
pub struct TcpTransport {
    stream: TcpStream,
    state: Arc<Mutex<ClientState>>,
}

impl TcpTransport {
    /// Connect with a bounded timeout, send the handshake, spawn the reader.
    pub fn connect(addr: &str, name: &str, intent: JoinIntent) -> std::io::Result<Self> {
        let sock_addr = addr
            .parse()
            .or_else(|_| {
                use std::net::ToSocketAddrs;
                addr.to_socket_addrs()
                    .map_err(|e| std::io::Error::other(format!("bad address '{addr}': {e}")))?
                    .next()
                    .ok_or_else(|| std::io::Error::other(format!("'{addr}' resolves to nothing")))
            })?;
        let mut stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(5))?;
        stream.set_nodelay(true).ok();
        write_msg(&mut stream, &Msg::Hello {
            version: PROTOCOL_VERSION,
            name: name.to_string(),
            intent,
        })?;
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
                    Ok(Msg::Batch { tick, entries }) => {
                        s2.lock().unwrap().batches.insert(tick, entries);
                    }
                    Ok(Msg::PeerLeft { id }) => {
                        s2.lock().unwrap().events.push(crate::net::NetEvent::PeerLeft(id));
                    }
                    Ok(m) => s2.lock().unwrap().lobby.apply(&m),
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

    pub fn set_faction(&mut self, faction: Faction) {
        let _ = write_msg(&mut self.stream, &Msg::SetFaction { faction });
    }
    pub fn set_ready(&mut self, ready: bool) {
        let _ = write_msg(&mut self.stream, &Msg::SetReady { ready });
    }
    pub fn add_ai(&mut self, difficulty: AiDifficulty, faction: Faction) {
        let _ = write_msg(&mut self.stream, &Msg::AddAi { difficulty, faction });
    }
    pub fn remove_ai(&mut self, id: u64) {
        let _ = write_msg(&mut self.stream, &Msg::RemoveAi { id });
    }
    pub fn set_map(&mut self, seed: u32, preset: u8) {
        let _ = write_msg(&mut self.stream, &Msg::SetMap { seed, preset });
    }

    /// Host: freeze the lobby and begin the match.
    pub fn request_start(&mut self) {
        let _ = write_msg(&mut self.stream, &Msg::Start);
    }
}

impl Drop for TcpTransport {
    /// `shutdown` reaches the OS socket shared by the reader thread's fd clone
    /// — without it the relay would never see this client leave (the reader's
    /// clone keeps the connection alive past the drop).
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

impl Transport for TcpTransport {
    fn submit(&mut self, tick: u64, player_id: u64, cmds: Vec<PlayerCommand>) {
        let _ = write_msg(&mut self.stream, &Msg::Submit { tick, player_id, cmds });
    }
    fn batch(&mut self, tick: u64) -> Option<Vec<(u64, Vec<PlayerCommand>)>> {
        self.state.lock().unwrap().batches.get(&tick).cloned()
    }
    fn take_events(&mut self) -> Vec<crate::net::NetEvent> {
        std::mem::take(&mut self.state.lock().unwrap().events)
    }
}

// ── relay ────────────────────────────────────────────────────────────────────

fn serve(listener: TcpListener) {
    let rooms: Arc<Rooms> = Arc::default();
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        stream.set_nodelay(true).ok();
        let rooms = rooms.clone();
        std::thread::spawn(move || {
            let mut reader = stream;
            // handshake: first frame must be a valid, version-matched Hello
            reader.set_read_timeout(Some(Duration::from_secs(10))).ok();
            let hello = match read_msg(&mut reader) {
                Ok(m @ Msg::Hello { .. }) => m,
                _ => return,
            };
            reader.set_read_timeout(None).ok();
            let (out_tx, out_rx) = mpsc::channel::<Msg>();
            let handle = match rooms.join(&hello, out_tx) {
                Ok(h) => h,
                Err(reason) => {
                    let _ = write_msg(&mut reader, &Msg::Reject { reason });
                    return;
                }
            };
            // writer thread: owns the write half, drains the mailbox
            let mut writer = match reader.try_clone() {
                Ok(w) => w,
                Err(_) => {
                    handle.disconnect();
                    return;
                }
            };
            std::thread::spawn(move || {
                while let Ok(m) = out_rx.recv() {
                    if write_msg(&mut writer, &m).is_err() {
                        return;
                    }
                }
            });
            // reader loop: owns the read half
            loop {
                match read_msg(&mut reader) {
                    Ok(m) => handle.handle(m),
                    Err(_) => {
                        handle.disconnect();
                        return;
                    }
                }
            }
        });
    }
}

/// Dedicated-server entry: bind and serve forever (many concurrent rooms).
pub fn run_relay(addr: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    println!("relay on {addr} — protocol v{PROTOCOL_VERSION}, rooms enabled");
    serve(listener);
    Ok(())
}

/// Host-a-game entry: bind, serve on a background thread, return immediately
/// so the hosting client can connect to itself (Direct intent → default room).
pub fn spawn_host_relay(addr: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    std::thread::spawn(move || serve(listener));
    Ok(())
}
