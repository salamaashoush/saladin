//! Wire protocol shared by every relay transport (TCP, WebSocket): handshake,
//! rooms, lobby state, lockstep frames. One `Msg` enum is the whole protocol —
//! bincode-encoded, length-prefixed by the transport.

use crate::PlayerCommand;
use saladin_sim::{AiDifficulty, Faction};
use serde::{Deserialize, Serialize};

/// Bumped on every wire-incompatible change. The relay rejects mismatches with
/// `Reject(VersionMismatch)` instead of letting bincode garbage-decode.
pub const PROTOCOL_VERSION: u32 = 2;

/// Room codes: 6 chars from an unambiguous alphabet (no 0/O, 1/I/L).
pub const ROOM_CODE_LEN: usize = 6;
pub const ROOM_CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";

/// Normalize user-typed room codes: trim, uppercase, drop separators. The
/// alphabet has no 0/O/1/I/L, so ambiguous characters simply never match.
pub fn normalize_room_code(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// What a connecting client wants from the relay.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum JoinIntent {
    /// Join the relay's default room (LAN host/join — one room per relay).
    Direct,
    /// Create a fresh room; the relay replies `RoomInfo` with its code.
    CreateRoom,
    /// Join an existing room by code.
    JoinRoom { code: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum RejectReason {
    VersionMismatch { server: u32, client: u32 },
    RoomNotFound,
    RoomFull,
    MatchStarted,
    BadHandshake,
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectReason::VersionMismatch { server, client } => {
                write!(f, "version mismatch: server v{server}, your build v{client} — update the game")
            }
            RejectReason::RoomNotFound => write!(f, "room not found — check the code"),
            RejectReason::RoomFull => write!(f, "room is full"),
            RejectReason::MatchStarted => write!(f, "match already started"),
            RejectReason::BadHandshake => write!(f, "handshake failed"),
        }
    }
}

/// One seat in the lobby roster (humans and host-added AI alike).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LobbyPlayer {
    pub id: u64,
    pub name: String,
    pub faction: Faction,
    pub ready: bool,
    pub is_ai: bool,
    pub ai_difficulty: AiDifficulty,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Msg {
    /// Client → relay, must be the first frame on the connection.
    Hello { version: u32, name: String, intent: JoinIntent },
    /// Relay → client: handshake failed; the relay closes after sending.
    Reject { reason: RejectReason },
    /// Relay → client: you are in this room (echoed code for the host to share).
    RoomInfo { code: String },
    /// Lobby state push: your id, the host's id, every seat, host's map pick.
    Roster { you: u64, host: u64, players: Vec<LobbyPlayer>, seed: u32, preset: u8 },
    /// Client → relay: pick a faction for your seat.
    SetFaction { faction: Faction },
    /// Client → relay: toggle ready.
    SetReady { ready: bool },
    /// Host → relay: add an AI seat.
    AddAi { difficulty: AiDifficulty, faction: Faction },
    /// Host → relay: remove an AI seat.
    RemoveAi { id: u64 },
    /// Host → relay: choose the map (seed + preset) for the match.
    SetMap { seed: u32, preset: u8 },
    /// Host → relay: freeze the roster and begin the match.
    Start,
    /// Relay → all: the match begins — final roster + the world everyone builds.
    Welcome { seed: u32, preset: u8, players: Vec<LobbyPlayer> },
    /// Relay → all (mid-match): a peer's connection dropped; remaining players'
    /// ticks complete without it.
    PeerLeft { id: u64 },
    Submit { tick: u64, player_id: u64, cmds: Vec<PlayerCommand> },
    Batch { tick: u64, entries: Vec<(u64, Vec<PlayerCommand>)> },
}

pub fn encode(m: &Msg) -> Vec<u8> {
    bincode::serialize(m).expect("msg serializes")
}

pub fn decode(bytes: &[u8]) -> Option<Msg> {
    bincode::deserialize(bytes).ok()
}

/// What the lobby/connection currently looks like, for the client UI.
#[derive(Clone, Debug, Default)]
pub struct LobbyState {
    pub connected: bool,
    pub you: u64,
    pub host: u64,
    pub players: Vec<LobbyPlayer>,
    pub room_code: Option<String>,
    pub seed: u32,
    pub preset: u8,
    pub started: bool,
    pub error: Option<String>,
}

impl LobbyState {
    pub fn is_host(&self) -> bool {
        self.connected && self.you != 0 && self.you == self.host
    }
    pub fn me(&self) -> Option<&LobbyPlayer> {
        self.players.iter().find(|p| p.id == self.you)
    }
    /// Everyone except the host must flag ready before the host can start.
    pub fn all_ready(&self) -> bool {
        self.players.iter().filter(|p| !p.is_ai && p.id != self.host).all(|p| p.ready)
    }
    /// Apply one relay frame to this snapshot. Shared by both transports.
    pub fn apply(&mut self, m: &Msg) {
        match m {
            Msg::Roster { you, host, players, seed, preset } => {
                self.you = *you;
                self.host = *host;
                self.players = players.clone();
                self.seed = *seed;
                self.preset = *preset;
            }
            Msg::RoomInfo { code } => self.room_code = Some(code.clone()),
            Msg::Welcome { seed, preset, players } => {
                self.players = players.clone();
                self.seed = *seed;
                self.preset = *preset;
                self.started = true;
            }
            Msg::Reject { reason } => {
                self.connected = false;
                self.error = Some(reason.to_string());
            }
            _ => {}
        }
    }
}
