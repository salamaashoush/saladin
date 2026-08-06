//! Farm-AI measurement: food actually delivered FROM FIELDS, per living field,
//! plus husk/stall detection. Scratch probe — lives in /tmp, copied in to run.
//!
//! cargo run --release -p saladin-protocol --example farm_ai_probe -- [diff] [secs]

use bevy_app::prelude::*;
use saladin_protocol::*;
use saladin_sim::*;
use std::collections::HashMap;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let diff = match args.first().map(|s| s.as_str()) {
        Some("easy") => AiDifficulty::Easy,
        Some("normal") => AiDifficulty::Normal,
        _ => AiDifficulty::Hard,
    };
    let secs: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(900);
    let ticks = secs * 20;
    let base: u32 = std::env::var("PROBE_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(48514);
    let seed = compose_seed(base, 0);

    let mut app = App::new();
    app.add_plugins(SimPlugin);
    app.finish();
    app.cleanup();
    app.world_mut().insert_resource(WorldConfig { seed });
    scatter_world_nodes(app.world_mut(), 1);
    app.world_mut().resource_mut::<CommandQueue>().0.push(PlayerCommand::AddAi {
        player_id: 1,
        host: 1,
        difficulty: diff,
        faction: Faction::Ayyubid,
        match_id: 1,
    });
    step(app.world_mut());

    // per-unit: last carrying, last carry type, last node it was cutting
    let mut carry: HashMap<u64, (i32, ResourceType, u64)> = HashMap::new();
    let mut food_field: i64 = 0;
    let mut food_wild: i64 = 0;
    let mut field_seconds: i64 = 0; // living-field-seconds, for per-field rates
    let mut hand_seconds: i64 = 0; // peasant-seconds spent tending (Constructing on a farm)
    let mut min_fields_after_peak = i32::MAX;
    let mut peak_fields = 0i32;
    let mut husk_max = 0i32;
    let mut idle_max = 0i32;
    let mut stall_ticks = 0i32;
    let mut idle_seconds: i64 = 0;
    let mut peasant_seconds: i64 = 0;
    let mut run_idle: HashMap<u64, i32> = HashMap::new();
    let mut longest_idle = 0i32;
    let mut over_ticks = 0i64;
    let mut over_run = 0i32;
    let mut over_run_max = 0i32;
    let mut worst_share = (0i32, 1i32);
    let mut stuck: Vec<(u32, u64, u64, u64, bool)> = Vec::new();
    let mut prev_state: HashMap<u64, (GatherState, u64)> = HashMap::new();
    let mut into_idle: HashMap<String, i64> = HashMap::new();

    println!("{diff:?} seed {seed} ({base}) {secs}s");
    println!(
        "{:>5} {:>6} {:>6} {:>5} {:>5} {:>5} {:>6} {:>6} {:>6} {:>5} {:>6}",
        "t", "farms", "fields", "ripe", "husk", "crop", "fdFLD", "fdWLD", "f/s/fl", "hands", "food"
    );
    let rich = std::env::var("RICH").is_ok();
    let mut granary_at: Option<(u32, V2)> = None;
    for t in 0..ticks {
        if rich {
            let w = app.world_mut();
            let mut q = w.query::<&mut Player>();
            for mut p in q.iter_mut(w) {
                if p.player_id == 1 {
                    p.stock = Stockpile { wood: 9000, stone: 9000, food: 9000, gold: 9000 };
                }
            }
        }
        step(app.world_mut());
        let w = app.world_mut();
        if granary_at.is_none() {
            let mut q = w.query::<(&Owner, &Pos, &Building)>();
            if let Some((_, p, _)) = q
                .iter(w)
                .find(|(o, _, b)| o.0 == 1 && b.kind == BuildingKind::Granary && operational(b.state))
            {
                granary_at = Some((t / 20, p.pos));
            }
        }

        let fields: HashMap<u64, u64> = {
            let mut q = w.query::<(&GameId, &FieldOf)>();
            q.iter(w).map(|(g, f)| (g.0, f.0)).collect()
        };
        {
            let mut q = w.query::<(&GameId, &Owner, &Unit)>();
            for (g, o, u) in q.iter(w) {
                if o.0 != 1 {
                    continue;
                }
                let e = carry.entry(g.0).or_insert((0, ResourceType::Food, 0));
                let node = if u.target_node != 0 { u.target_node } else { e.2 };
                if u.carrying < e.0 && e.1 == ResourceType::Food {
                    let d = (e.0 - u.carrying) as i64;
                    if fields.contains_key(&e.2) {
                        food_field += d;
                    } else {
                        food_wild += d;
                    }
                }
                *e = (u.carrying, u.carry_type, node);
            }
        }

        let living = fields.len() as i32;
        field_seconds += living as i64;
        peak_fields = peak_fields.max(living);
        if living == peak_fields {
            min_fields_after_peak = living;
        } else if peak_fields > 0 {
            min_fields_after_peak = min_fields_after_peak.min(living);
        }

        let farms = {
            let mut q = w.query::<(&Owner, &Building)>();
            q.iter(w)
                .filter(|(o, b)| o.0 == 1 && b.kind == BuildingKind::Farm && operational(b.state))
                .count() as i32
        };
        husk_max = husk_max.max(farms - living);

        let (idle, tending) = {
            let mut q = w.query::<(&GameId, &Owner, &Unit)>();
            let mut idle = 0;
            let mut tending = 0;
            for (g, o, u) in q.iter(w) {
                if o.0 != 1 || u.kind != UnitKind::Peasant {
                    continue;
                }
                peasant_seconds += 1;
                if let Some((ps, pj)) = prev_state.get(&g.0).copied() {
                    if u.gather_state == GatherState::Idle && ps != GatherState::Idle {
                        let onfield = fields.values().any(|f| *f == pj);
                        *into_idle
                            .entry(format!("{ps:?}{}", if onfield { "@field" } else { "" }))
                            .or_insert(0) += 1;
                    }
                }
                prev_state.insert(g.0, (u.gather_state, u.job_site));
                let r = run_idle.entry(g.0).or_insert(0);
                if u.gather_state == GatherState::Idle {
                    idle += 1;
                    *r += 1;
                    longest_idle = longest_idle.max(*r);
                    if *r == 100 {
                        stuck.push((t, g.0, u.job_site, u.target_node, u.has_target));
                    }
                } else {
                    *r = 0;
                }
                if u.gather_state == GatherState::Constructing && fields.values().any(|f| *f == u.job_site) {
                    tending += 1;
                }
            }
            (idle, tending)
        };
        {
            let mut q = w.query::<(&Owner, &Unit)>();
            let (mut fh, mut pe) = (0i32, 0i32);
            for (o, u) in q.iter(w) {
                if o.0 != 1 || u.kind != UnitKind::Peasant {
                    continue;
                }
                pe += 1;
                if fields.values().any(|f| *f == u.job_site) {
                    fh += 1;
                }
            }
            if fh * worst_share.1 > worst_share.0 * pe.max(1) {
                worst_share = (fh, pe.max(1));
            }
            if fh * 2 > pe {
                over_ticks += 1;
                over_run += 1;
                over_run_max = over_run_max.max(over_run);
            } else {
                over_run = 0;
            }
        }
        idle_max = idle_max.max(idle);
        idle_seconds += idle as i64;
        hand_seconds += tending as i64;
        if idle > 0 {
            stall_ticks += 1;
        }

        if t % 1000 != 999 {
            continue;
        }
        let (ripe, crop) = {
            let mut q = w.query::<(&FieldOf, &ResourceNode, Option<&Crop>)>();
            let mut r = 0;
            let mut c = 0;
            for (_, n, cr) in q.iter(w) {
                c += n.remaining;
                if cr.is_some_and(|x| x.ripe) {
                    r += 1;
                }
            }
            (r, c)
        };
        let food = {
            let mut q = w.query::<&Player>();
            q.iter(w).find(|p| p.player_id == 1).map(|p| p.stock.food).unwrap_or(0)
        };
        let per_field = if field_seconds > 0 {
            food_field as f64 * 20.0 / field_seconds as f64
        } else {
            0.0
        };
        println!(
            "{:>5} {:>6} {:>6} {:>5} {:>5} {:>5} {:>6} {:>6} {:>6.3} {:>5} {:>6}",
            t / 20,
            farms,
            living,
            ripe,
            farms - living,
            crop,
            food_field,
            food_wild,
            per_field,
            tending,
            food
        );
    }
    let secs_f = secs as f64;
    println!(
        "SUMMARY seed={base} {diff:?}: field_food={food_field} ({:.2}/s)  wild_food={food_wild} ({:.2}/s)",
        food_field as f64 / secs_f,
        food_wild as f64 / secs_f
    );
    println!(
        "        field-seconds={}  per-living-field={:.3} food/s   tend-hand-seconds={}  food per tend-hand-s={:.2}",
        field_seconds / 20,
        food_field as f64 * 20.0 / field_seconds.max(1) as f64,
        hand_seconds / 20,
        food_field as f64 * 20.0 / hand_seconds.max(1) as f64
    );
    println!(
        "        peak_fields={peak_fields} min_after_peak={} max_husks={husk_max} max_idle={idle_max} ticks_with_idle={stall_ticks}",
        if min_fields_after_peak == i32::MAX { peak_fields } else { min_fields_after_peak }
    );
    println!(
        "        idle peasant-seconds={} of {} ({:.2}%)  longest single idle run={:.1}s",
        idle_seconds / 20,
        peasant_seconds / 20,
        idle_seconds as f64 * 100.0 / peasant_seconds.max(1) as f64,
        longest_idle as f64 / 20.0
    );
    println!(
        "        field hands vs peasants: worst {}/{}   ticks over half the town {} ({:.2}%), longest such run {:.1}s",
        worst_share.0, worst_share.1, over_ticks,
        over_ticks as f64 * 100.0 / ticks as f64, over_run_max as f64 / 20.0
    );
    let mut ii: Vec<(String, i64)> = into_idle.into_iter().collect();
    ii.sort_by_key(|(_, n)| -*n);
    println!("        transitions INTO idle by previous state: {ii:?}");
    for (t, id, js, tn, ht) in stuck.iter().take(12) {
        println!("        STUCK t={}s unit {id} job_site={js} target_node={tn} has_target={ht}", t / 20);
    }
    // granary coverage: how many of the bot's farms the hub's aura actually reaches
    let w = app.world_mut();
    let keep = {
        let mut q = w.query::<(&Owner, &Pos, &Building)>();
        q.iter(w)
            .find(|(o, _, b)| o.0 == 1 && b.kind == BuildingKind::Keep)
            .map(|(_, p, _)| p.pos)
    };
    let farms: Vec<V2> = {
        let mut q = w.query::<(&Owner, &Pos, &Building)>();
        q.iter(w)
            .filter(|(o, _, b)| o.0 == 1 && b.kind == BuildingKind::Farm && operational(b.state))
            .map(|(_, p, _)| p.pos)
            .collect()
    };
    match (granary_at, keep) {
        (Some((at, g)), Some(k)) => {
            let cov = farms.iter().filter(|f| dist(g, **f) <= GRANARY_RANGE).count();
            let kcov = farms.iter().filter(|f| dist(k, **f) <= GRANARY_RANGE).count();
            let mut ds: Vec<f32> = farms.iter().map(|f| dist(g, *f).to_num::<f32>()).collect();
            ds.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "        granary up at t={at}s covers {cov}/{} farms (a keep-sited hub would cover {kcov}); granary-farm dists {:?}",
                farms.len(),
                ds.iter().map(|d| (d * 10.0).round() / 10.0).collect::<Vec<_>>()
            );
        }
        _ => println!("        no granary in {secs}s ({} farms)", farms.len()),
    }
}
