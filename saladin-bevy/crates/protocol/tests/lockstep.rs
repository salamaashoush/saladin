use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{Faction, NEUTRAL_BIAS};
use std::collections::HashMap;

fn world_app() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1, bias: NEUTRAL_BIAS });
    // identical deterministic worldgen on every client
    scatter_world_nodes(app.world_mut(), 1);
    app
}

/// Two clients connected only by a command relay (no shared state) must stay
/// bit-identical at every tick — the lockstep guarantee that lets the game scale
/// to any unit count over the wire.
#[test]
fn two_clients_stay_in_sync() {
    let relay = shared_relay(vec![1, 2]);
    let mut w1 = world_app();
    let mut w2 = world_app();
    let mut t1 = MemTransport::new(relay.clone());
    let mut t2 = MemTransport::new(relay.clone());
    let mut d1 = LockstepDriver::new(1, 2);
    let mut d2 = LockstepDriver::new(2, 2);

    // each client originates ONLY its own player's join; the relay broadcasts it
    d1.push(PlayerCommand::Join { player_id: 1, name: "A".into(), faction: Faction::Ayyubid, match_id: 1 });
    d2.push(PlayerCommand::Join { player_id: 2, name: "B".into(), faction: Faction::Crusader, match_id: 1 });

    let mut h1: HashMap<u64, u64> = HashMap::new();
    let mut h2: HashMap<u64, u64> = HashMap::new();
    for _ in 0..600 {
        if d1.advance(w1.world_mut(), &mut t1) {
            h1.insert(d1.tick - 1, w1.world().resource::<StateHash>().0);
        }
        if d2.advance(w2.world_mut(), &mut t2) {
            h2.insert(d2.tick - 1, w2.world().resource::<StateHash>().0);
        }
    }

    // every tick both clients reached must hash identically
    let mut common = 0;
    for (tick, a) in &h1 {
        if let Some(b) = h2.get(tick) {
            assert_eq!(a, b, "desync at tick {tick}");
            common += 1;
        }
    }
    assert!(common > 100, "clients should share many synced ticks, shared {common}");

    // and the match actually ran: both players founded on client 1
    let mut pq = w1.world_mut().query::<&Player>();
    assert_eq!(pq.iter(w1.world()).count(), 2, "both players present");
}
