//! Transport-agnostic relay brain: many concurrent ROOMS, each its own lobby +
//! lockstep batch stream. The TCP and WebSocket relays are thin socket loops
//! around this — they hand every decoded `Msg` to a `ClientHandle` and drain a
//! per-client mailbox. Both sides of an internet match connect OUTBOUND to a
//! relay running this, so NAT traversal needs zero configuration.

use crate::PlayerCommand;
use crate::net_msg::{
    JoinIntent, LobbyPlayer, Msg, PROTOCOL_VERSION, ROOM_CODE_ALPHABET, ROOM_CODE_LEN,
    RejectReason, normalize_room_code,
};
use saladin_sim::{AiDifficulty, Faction};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

pub const MAX_ROOM_PLAYERS: usize = 8;
/// AI seats get ids in this range so they never collide with relay-assigned
/// human ids (which start at 1 and count up per room).
const AI_ID_BASE: u64 = 1000;

struct Seat {
    player: LobbyPlayer,
    mailbox: Option<Sender<Msg>>, // None for AI seats
}

struct Room {
    seats: Vec<Seat>,
    host: u64,
    started: bool,
    seed: u32,
    preset: u8,
    next_player: u64,
    next_ai: u64,
    subs: HashMap<(u64, u64), Vec<PlayerCommand>>,
    next_tick: u64,
}

impl Room {
    fn new(seed: u32) -> Self {
        Room {
            seats: Vec::new(),
            host: 0,
            started: false,
            seed,
            preset: 0,
            next_player: 1,
            next_ai: AI_ID_BASE,
            subs: HashMap::new(),
            next_tick: 0,
        }
    }

    fn humans(&self) -> impl Iterator<Item = &Seat> {
        self.seats.iter().filter(|s| !s.player.is_ai)
    }

    fn roster(&self) -> Vec<LobbyPlayer> {
        self.seats.iter().map(|s| s.player.clone()).collect()
    }

    fn broadcast(&self, m: &Msg) {
        for s in &self.seats {
            if let Some(tx) = &s.mailbox {
                let _ = tx.send(m.clone());
            }
        }
    }

    fn broadcast_roster(&self) {
        let players = self.roster();
        for s in &self.seats {
            if let Some(tx) = &s.mailbox {
                let _ = tx.send(Msg::Roster {
                    you: s.player.id,
                    host: self.host,
                    players: players.clone(),
                    seed: self.seed,
                    preset: self.preset,
                });
            }
        }
    }

    /// Flush every complete tick in order. Only HUMAN seats submit — AI runs
    /// inside every client's deterministic sim, not on the relay.
    fn flush_batches(&mut self) {
        loop {
            let ids: Vec<u64> = self.humans().map(|s| s.player.id).collect();
            let mut entries = Vec::with_capacity(ids.len());
            for &p in &ids {
                match self.subs.get(&(self.next_tick, p)) {
                    Some(c) => entries.push((p, c.clone())),
                    None => return,
                }
            }
            entries.sort_by_key(|(p, _)| *p);
            self.broadcast(&Msg::Batch { tick: self.next_tick, entries });
            self.next_tick += 1;
        }
    }

    fn all_ready(&self) -> bool {
        self.humans().all(|s| s.player.ready)
    }
}

/// All rooms on one relay. `Direct` joins land in the unnamed default room
/// (the LAN host/join path); `CreateRoom`/`JoinRoom` use coded rooms.
pub struct Rooms {
    rooms: Mutex<HashMap<String, Arc<Mutex<Room>>>>,
    code_nonce: AtomicU64,
}

impl Default for Rooms {
    fn default() -> Self {
        Rooms { rooms: Mutex::new(HashMap::new()), code_nonce: AtomicU64::new(0) }
    }
}

impl Rooms {
    /// Room codes only need to be unguessable-ish and unique per relay; this is
    /// not sim code, so wall clock is fine as entropy.
    fn fresh_code(&self, taken: &HashMap<String, Arc<Mutex<Room>>>) -> String {
        loop {
            let nonce = self.code_nonce.fetch_add(1, Ordering::Relaxed);
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let mut x = t ^ nonce.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let mut code = String::with_capacity(ROOM_CODE_LEN);
            for _ in 0..ROOM_CODE_LEN {
                x ^= x >> 33;
                x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
                x ^= x >> 33;
                code.push(ROOM_CODE_ALPHABET[(x % ROOM_CODE_ALPHABET.len() as u64) as usize] as char);
            }
            if !taken.contains_key(&code) {
                return code;
            }
        }
    }

    /// Process a handshake. On success the seat is created, the client gets
    /// `RoomInfo` + `Roster`, and the caller receives a handle for the rest of
    /// the connection's life.
    pub fn join(
        self: &Arc<Self>,
        hello: &Msg,
        mailbox: Sender<Msg>,
    ) -> Result<ClientHandle, RejectReason> {
        let Msg::Hello { version, name, intent } = hello else {
            return Err(RejectReason::BadHandshake);
        };
        if *version != PROTOCOL_VERSION {
            return Err(RejectReason::VersionMismatch { server: PROTOCOL_VERSION, client: *version });
        }

        let mut rooms = self.rooms.lock().unwrap();
        let (code, room_arc) = match intent {
            JoinIntent::Direct => {
                let r = rooms.entry(String::new()).or_insert_with(|| Arc::new(Mutex::new(Room::new(1))));
                (String::new(), r.clone())
            }
            JoinIntent::CreateRoom => {
                let code = self.fresh_code(&rooms);
                let r = Arc::new(Mutex::new(Room::new(1)));
                rooms.insert(code.clone(), r.clone());
                (code, r)
            }
            JoinIntent::JoinRoom { code } => {
                let code = normalize_room_code(code);
                match rooms.get(&code) {
                    Some(r) => (code, r.clone()),
                    None => return Err(RejectReason::RoomNotFound),
                }
            }
        };
        drop(rooms);

        let id = {
            let mut room = room_arc.lock().unwrap();
            if room.started {
                return Err(RejectReason::MatchStarted);
            }
            if room.humans().count() >= MAX_ROOM_PLAYERS {
                return Err(RejectReason::RoomFull);
            }
            let id = room.next_player;
            room.next_player += 1;
            if room.humans().count() == 0 {
                room.host = id;
            }
            let display = if name.trim().is_empty() { format!("Player {id}") } else { name.trim().to_string() };
            room.seats.push(Seat {
                player: LobbyPlayer {
                    id,
                    name: display,
                    faction: Faction::Ayyubid,
                    ready: false,
                    is_ai: false,
                    ai_difficulty: AiDifficulty::Normal,
                },
                mailbox: Some(mailbox.clone()),
            });
            let _ = mailbox.send(Msg::RoomInfo { code: code.clone() });
            room.broadcast_roster();
            println!("room '{code}': player {id} joined ({} seats)", room.seats.len());
            id
        };

        Ok(ClientHandle { rooms: self.clone(), room: room_arc, code, id })
    }
}

/// One connected client's view into its room. The socket loop calls `handle`
/// per decoded frame and `disconnect` when the socket dies.
pub struct ClientHandle {
    rooms: Arc<Rooms>,
    room: Arc<Mutex<Room>>,
    code: String,
    id: u64,
}

impl ClientHandle {
    pub fn handle(&self, msg: Msg) {
        let mut room = self.room.lock().unwrap();
        match msg {
            Msg::SetFaction { faction } => {
                if let Some(s) = room.seats.iter_mut().find(|s| s.player.id == self.id) {
                    s.player.faction = faction;
                }
                room.broadcast_roster();
            }
            Msg::SetReady { ready } => {
                if let Some(s) = room.seats.iter_mut().find(|s| s.player.id == self.id) {
                    s.player.ready = ready;
                }
                room.broadcast_roster();
            }
            Msg::AddAi { difficulty, faction } => {
                if room.host == self.id && !room.started && room.seats.len() < MAX_ROOM_PLAYERS {
                    let id = room.next_ai;
                    room.next_ai += 1;
                    room.seats.push(Seat {
                        player: LobbyPlayer {
                            id,
                            name: format!("AI {}", id - AI_ID_BASE + 1),
                            faction,
                            ready: true,
                            is_ai: true,
                            ai_difficulty: difficulty,
                        },
                        mailbox: None,
                    });
                    room.broadcast_roster();
                }
            }
            Msg::RemoveAi { id } => {
                if room.host == self.id && !room.started {
                    room.seats.retain(|s| !(s.player.is_ai && s.player.id == id));
                    room.broadcast_roster();
                }
            }
            Msg::SetMap { seed, preset } => {
                if room.host == self.id && !room.started {
                    room.seed = seed;
                    room.preset = preset;
                    room.broadcast_roster();
                }
            }
            Msg::Start => {
                if room.host == self.id && !room.started && room.all_ready() {
                    room.started = true;
                    let m = Msg::Welcome { seed: room.seed, preset: room.preset, players: room.roster() };
                    println!(
                        "room '{}': match started, seed {} preset {} ({} seats)",
                        self.code, room.seed, room.preset, room.seats.len()
                    );
                    room.broadcast(&m);
                }
            }
            Msg::Submit { tick, player_id, cmds } => {
                room.subs.entry((tick, player_id)).or_insert(cmds);
                if room.started {
                    room.flush_batches();
                }
            }
            _ => {}
        }
    }

    pub fn disconnect(&self) {
        let empty = {
            let mut room = self.room.lock().unwrap();
            room.seats.retain(|s| s.player.id != self.id);
            println!("room '{}': player {} left ({} seats remain)", self.code, self.id, room.seats.len());
            if !room.started {
                if room.host == self.id {
                    let next_host = room.humans().next().map(|s| s.player.id).unwrap_or(0);
                    room.host = next_host;
                }
                room.broadcast_roster();
            } else {
                // remaining players' ticks complete without the leaver
                room.broadcast(&Msg::PeerLeft { id: self.id });
                room.flush_batches();
            }
            room.humans().count() == 0
        };
        if empty {
            let mut rooms = self.rooms.rooms.lock().unwrap();
            // re-check under the registry lock: someone may have joined between
            let still_empty =
                self.room.lock().unwrap().humans().count() == 0;
            if still_empty {
                rooms.remove(&self.code);
                println!("room '{}' closed", self.code);
            }
        }
    }
}
