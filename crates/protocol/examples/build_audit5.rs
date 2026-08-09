//! Part 5: can a farm be harvested out of existence?

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;

fn build_app(seed: u32) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    app
}
fn cmd(app: &mut App, c: PlayerCommand) {
    app.world_mut().resource_mut::<CommandQueue>().0.push(c);
}

fn main() {
    let seed = compose_seed(48514, 0);
    let mut app = build_app(seed);
    scatter_world_nodes(app.world_mut(), 1);
    cmd(&mut app, PlayerCommand::Join { player_id: 1, name: "H".into(), faction: Faction::Ayyubid, match_id: 1 });
    step(app.world_mut());
    {
        let w = app.world_mut();
        let mut q = w.query::<&mut Player>();
        for mut p in q.iter_mut(w) {
            p.stock = Stockpile { wood: 5000, stone: 5000, food: 5000, gold: 5000 };
        }
    }
    let kp = {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &Building, &Pos)>();
        q.iter(w).find(|(o, b, _)| o.0 == 1 && b.kind == BuildingKind::Keep).map(|(_, _, p)| p.pos).unwrap()
    };
    // find a farm site
    let own = vec![kp];
    let mut fp = None;
    'f: for r in 3..25i32 {
        for dy in -r..=r { for dx in -r..=r {
            if dx.abs().max(dy.abs()) != r { continue; }
            let x = kp.x.floor() + Fx::from_num(dx) + fx!("0.5");
            let y = kp.y.floor() + Fx::from_num(dy) + fx!("0.5");
            if check_place(seed, BuildingKind::Farm, x, y, |_, _| false, |_, _| true, &own).is_ok() { fp = Some(V2::new(x, y)); break 'f; }
        }}
    }
    let fp = fp.expect("farm site");
    cmd(&mut app, PlayerCommand::Build { player_id: 1, kind: BuildingKind::Farm, pos: fp, facing: 0, builders: vec![] });
    step(app.world_mut());
    let (fid, field_id, regen) = {
        let w = app.world_mut();
        let mut q = w.query::<(&GameId, &FieldOf, &ResourceNode)>();
        let (g, f, n) = q.iter(w).next().expect("field sown");
        (f.0, g.0, n.regen)
    };
    println!("farm {fid} sown a field {field_id} (regen {regen}/2s, cap {FARM_STORE}, starts at {})", FARM_STORE / 3);

    // put every peasant on the field
    {
        let w = app.world_mut();
        let mut q = w.query::<(&Owner, &mut Unit, &mut Pos)>();
        for (o, mut u, mut p) in q.iter_mut(w) {
            if o.0 == 1 {
                p.pos = fp;
                u.gather_state = GatherState::ToResource;
                u.target_node = field_id;
                u.has_target = false;
                u.carrying = 0;
            }
        }
    }
    let mut gone_at = None;
    let mut food_from_farm = 0;
    let before_food = {
        let w = app.world_mut();
        let mut q = w.query::<&Player>();
        q.iter(w).next().unwrap().stock.food
    };
    for t in 1..=4000u64 {
        step(app.world_mut());
        let alive = {
            let w = app.world_mut();
            let mut q = w.query::<&FieldOf>();
            q.iter(w).count()
        };
        if alive == 0 && gone_at.is_none() {
            gone_at = Some(t);
            let w = app.world_mut();
            let mut q = w.query::<&Player>();
            food_from_farm = q.iter(w).next().unwrap().stock.food - before_food;
            break;
        }
    }
    match gone_at {
        Some(t) => {
            let farm_alive = {
                let w = app.world_mut();
                let mut q = w.query::<&Building>();
                q.iter(w).filter(|b| b.kind == BuildingKind::Farm).count()
            };
            println!("  5 peasants stripped the field to ZERO at t{t} ({:.1}s). Total food banked from it: {food_from_farm}",
                t as f64 * 0.05);
            println!("  the Farm BUILDING is still standing ({farm_alive}) but its field entity is despawned.");
            // does it ever come back?
            for _ in 0..2000 { step(app.world_mut()); }
            let back = {
                let w = app.world_mut();
                let mut q = w.query::<&FieldOf>();
                q.iter(w).count()
            };
            println!("  100 s later the field count is still {back}. A farm is a ONE-SHOT {} food deposit for {} wood,",
                back, building_def(BuildingKind::Farm).cost.wood);
            println!("  then a permanently dead 220-hp building whose blurb says \"peasants harvest it forever\".");
            println!("  There is no re-sow command. The only fix is Demolish (+{}w) and rebuild (-{}w).",
                building_def(BuildingKind::Farm).cost.wood / 2, building_def(BuildingKind::Farm).cost.wood);
        }
        None => println!("  field survived 200 s of 5-peasant harvest (regen kept up)"),
    }
}
