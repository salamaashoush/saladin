/// A match is a first-class entity; scheduled systems simulate only `Active`
/// matches, so `Paused` freezes one in place and `Ended` tears it down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum MatchStatus {
    Active = 0,
    Paused = 1,
    Ended = 2,
}

impl MatchStatus {
    pub fn from_u8(v: u8) -> Option<MatchStatus> {
        match v {
            0 => Some(MatchStatus::Active),
            1 => Some(MatchStatus::Paused),
            2 => Some(MatchStatus::Ended),
            _ => None,
        }
    }
}

/// Only an Active match advances under the simulation loops.
pub fn match_simulates(status: MatchStatus) -> bool {
    status == MatchStatus::Active
}
