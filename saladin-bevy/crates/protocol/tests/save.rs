//! Save/restore round-trip: a running match snapshotted, restored into a fresh
//! world, must continue bit-identically (same state hash trajectory).

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::{AiDifficulty, Faction, NEUTRAL_BIAS};

fn build() -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed: 1, bias: NEUTRAL_BIAS });
    app
}

#[test]
fn save_restore_resumes_bit_identically() {
    let mut a = build();
    scatter_world_nodes(a.world_mut(), 1);
    a.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::Join {
        player_id: 1,
        name: "Saladin".into(),
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    a.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
        player_id: 1000,
        host: 1,
        difficulty: AiDifficulty::Normal,
        faction: Faction::Crusader,
        match_id: 1,
    });
    for _ in 0..200 {
        step(a.world_mut());
    }

    // snapshot mid-match, push through bytes like the real save file
    let bytes = save::to_bytes(&save::snapshot(a.world_mut()));
    let snap = save::from_bytes(&bytes).expect("savegame parses");

    let mut b = build();
    save::restore(b.world_mut(), snap);

    // both worlds must now evolve identically
    for i in 0..200 {
        step(a.world_mut());
        step(b.world_mut());
        let ha = a.world().resource::<StateHash>().0;
        let hb = b.world().resource::<StateHash>().0;
        assert_eq!(ha, hb, "restored world diverged at step {i}");
    }
}
