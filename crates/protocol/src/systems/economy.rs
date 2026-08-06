use crate::MatchStatuses;
use crate::WorldConfig;
use crate::components::{
    Building, Crop, FieldOf, GameId, MatchId, Owner, Player, Pos, ResourceNode, Unit,
};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{
    AuraTarget, FAMINE_RATION, FARM_RIPE_GRACE, FULL_RATION, Fx, ResourceType, SUPPLY_RADIUS, V2,
    WorkAura, apply_supply, building_def, deserts, dist, dist2, draws_rations, fatigue_ticks,
    field_growth, forage_yield, fx_sqrt, is_sailable, lodge_loss, man_draw, morale_ceiling,
    operational, strain, supply::STARVE_GRACE_TICKS, supply_bill, unit_def,
};

/// Foragers served per player per economy tick, in `GameId` order — a budget in
/// the shape of combat's pursuit budget, so a whole starving host cannot walk
/// every wild herd on the map.
const FORAGE_BUDGET: usize = 256;

/// How close a man stands to a wild herd to live off it.
const FORAGE_RANGE: Fx = saladin_sim::fx!("3");

/// One deserter per this many mouths per economy tick. The cap is what makes an
/// army BLEED men instead of evaporating in a single tick.
const DESERT_DIVISOR: usize = 8;

struct Eater {
    owner: u64,
    gid: u64,
    entity: Entity,
    /// How hard the road pulls on this man. ZERO inside the supply radius, which
    /// is every man in a garrison — a defence at home bills nothing at all.
    strain: Fx,
}

/// Retained buffers — economy runs at 0.5 Hz but over every unit on the map.
#[derive(Default)]
pub struct SupplyScratch {
    anchors: Vec<(u64, V2)>,
    eaters: Vec<Eater>,
    /// Wild food nodes a soldier can walk onto, sorted by `GameId`.
    wild: Vec<(u64, Entity, V2)>,
    deserters: Vec<(Fx, u64, Entity)>,
}

/// How hard the road pulls on one man, measured to his OWN nearest store. The
/// `dist2` early exit is what keeps this off the sqrt: a man in supply — which
/// is nearly all of them, nearly all the time — never takes a root.
///
/// A player with no store anywhere has no supply LINE to be cut: his men carry
/// what they have and cost nothing wherever they stand. Without this a side
/// would be punished for owning nothing to lose, and the case is unreachable in
/// a real match — the Keep is a drop-off and losing it is defeat.
fn strain_of(anchors: &[(u64, V2)], owner: u64, at: V2) -> Fx {
    let r2 = SUPPLY_RADIUS * SUPPLY_RADIUS;
    let lo = anchors.partition_point(|(o, _)| *o < owner);
    let mut best: Option<Fx> = None;
    for (_, p) in anchors[lo..].iter().take_while(|(o, _)| *o == owner) {
        let d2 = dist2(*p, at);
        if d2 <= r2 {
            return Fx::ZERO;
        }
        best = Some(best.map_or(d2, |b| b.min(d2)));
    }
    match best {
        None => Fx::ZERO,
        Some(d2) => strain(fx_sqrt(d2)),
    }
}

/// Supply, regrowth and the muster roll — every economy tick (2 s).
///
/// THE BAGGAGE TRAIN. A man within reach of one of his own drop-offs is fed by
/// the ground he is standing on and bills NOTHING; every tile past that, the
/// road costs more, and the road is paid in food from home. Rations are then
/// PROPORTIONAL over the field force, so a shortfall of one man's food costs one
/// man's food.
///
/// What this replaced was a flat per-head tax charged wherever a man stood. It
/// had no band: at the rate that made it bite it was a death spiral, and at the
/// rate that could not spiral it was decoration (measured, 10 soldiers drawing
/// 1.25 food/s against an 1868 stockpile).
#[allow(clippy::too_many_arguments)]
pub fn economy(
    statuses: Res<MatchStatuses>,
    cfg: Res<WorldConfig>,
    mut commands: Commands,
    mut scratch: Local<SupplyScratch>,
    mut q_players: Query<(&GameId, &mut Player, &MatchId)>,
    mut q_units: Query<(Entity, &GameId, &Pos, &Owner, &mut Unit)>,
    q_buildings: Query<(&GameId, &Pos, &Owner, &Building)>,
    mut q_nodes: Query<NodeData>,
    mut stats: ResMut<crate::MatchStats>,
) {
    let seed = cfg.seed;
    // The season. A sown field GROWS — the soil says how big, the crew standing
    // on the farm says how fast — ripens, and lodges if nobody cuts it; a fishing
    // hut tends the waters in its reach. Everything else (timber, ore, wild
    // herds) is finite and stays mined out.
    // Additive + clamped, so iteration order can never desync the lockstep.
    let auras: Vec<(u64, V2, WorkAura)> = q_buildings
        .iter()
        .filter(|(_, _, _, b)| operational(b.state))
        .filter_map(|(_, p, o, b)| building_def(b.kind).aura.map(|a| (o.0, p.pos, a)))
        .collect();
    // A field belongs to the farm that sowed it, so only that player's granary
    // may tend it and only that farm's crew works it. A wild fishery belongs to
    // nobody: whoever plants the hut tends the water, and the ground is
    // contested on purpose. One walk over the buildings answers both.
    let farm_of: HashMap<u64, (u64, i32, bool)> = q_buildings
        .iter()
        .map(|(g, _, o, b)| (g.0, (o.0, b.builders, operational(b.state))))
        .collect();
    let tended = |at: V2, target: AuraTarget, node_owner: Option<u64>| -> i32 {
        auras
            .iter()
            .filter(|(o, p, a)| {
                a.target == target
                    && node_owner.is_none_or(|n| n == *o)
                    && dist(*p, at) <= a.radius
            })
            .map(|(_, _, a)| a.regen)
            .max()
            .unwrap_or(0)
    };
    let s = &mut *scratch;
    s.wild.clear();
    for (ent, gid, np, mut n, field, crop) in &mut q_nodes {
        // DRY, not "walkable": a cliff and a mountainside are impassable and dry,
        // and calling them water put a herd under a scarp into the fishery's
        // restock branch and out of the forage pool at the same time.
        let dry = !is_sailable(seed, np.pos.x.to_num::<i32>(), np.pos.y.to_num::<i32>());
        if let Some(f) = field {
            let (owner, hands) = match farm_of.get(&f.0).copied() {
                Some((o, crew, up)) => (o, if up { crew } else { 0 }),
                None => (0, 0),
            };
            let aura = if auras.is_empty() {
                0
            } else {
                tended(np.pos, AuraTarget::Field, Some(owner))
            };
            match crop {
                Some(mut c) => {
                    let (rem, next) = season(&n, &c, hands, aura);
                    if n.remaining != rem {
                        n.remaining = rem;
                    }
                    if *c != next {
                        *c = next;
                    }
                }
                // a field sown before crops existed (a harness row, an old save):
                // the plain renewable it used to be
                None if n.regen > 0 && n.remaining < n.cap => {
                    n.remaining = (n.remaining + n.regen + aura).min(n.cap);
                }
                None => {}
            }
        } else if n.regen > 0 && n.remaining < n.cap {
            // A hut MULTIPLIES a fishery's own regrowth; it does not supply it.
            // The flat top-up it replaces made a TENDED school empty 20% faster
            // than an untended one — the same aura doubles the DRAW — for +32
            // fish over a match, so the building was measurably negative at the
            // one job it has. It also topped up to `FOOD_YIELD` and not to the
            // node's own cap, which overfilled an inshore school and starved a
            // deep one.
            let nets = if n.res_type == ResourceType::Food && !dry && !auras.is_empty() {
                tended(np.pos, AuraTarget::WaterFood, None).max(1)
            } else {
                1
            };
            n.remaining = (n.remaining + n.regen * nets).min(n.cap);
        }
        // A herd nobody has sown and nobody owns: what an army in the field
        // lives on. Fish are not forage — a spearman cannot net them.
        if n.res_type == ResourceType::Food && n.remaining > 0 && dry && field.is_none() {
            s.wild.push((gid.0, ent, np.pos));
        }
    }
    s.wild.sort_unstable_by_key(|(g, _, _)| *g);

    // Anywhere a haul can be dropped feeds the men around it. A player who owns
    // no store has nothing to be in supply of, and every mouth pays the road.
    s.anchors.clear();
    s.anchors.extend(
        q_buildings
            .iter()
            .filter(|(_, _, _, b)| operational(b.state) && building_def(b.kind).accepts != 0)
            .map(|(_, p, o, _)| (o.0, p.pos)),
    );
    s.anchors.sort_unstable_by_key(|(o, _)| *o);

    // The muster roll. ROLE decides who eats, not `attack > 0`: arming a
    // peasant must never silently put it on the roll.
    s.eaters.clear();
    for (ent, gid, pos, owner, unit) in q_units.iter() {
        if !draws_rations(unit.kind) {
            continue;
        }
        s.eaters.push(Eater {
            owner: owner.0,
            gid: gid.0,
            entity: ent,
            strain: strain_of(&s.anchors, owner.0, pos.pos),
        });
    }
    s.eaters.sort_unstable_by_key(|e| (e.owner, e.gid));

    let SupplyScratch { eaters, wild, deserters, .. } = s;

    for (_gid, mut p, mid) in &mut q_players {
        if p.defeated || !statuses.simulates(mid.0) {
            continue;
        }
        let lo = eaters.partition_point(|e| e.owner < p.player_id);
        let hi = eaters.partition_point(|e| e.owner <= p.player_id);
        let mine = &eaters[lo..hi];

        // ONE band, not two. The garrison bills zero by construction (its strain
        // is zero), so the whole shortfall falls where the decision was made:
        // on the column that marched out. This is also the half of a siege that
        // costs the BESIEGER something.
        let total_strain = mine.iter().fold(Fx::ZERO, |acc, e| acc + e.strain);
        let bill = supply_bill(total_strain);
        let food = p.stock.food;
        let out = apply_supply(food, bill);
        if out.food != food {
            p.stock.food = out.food;
        }
        let r_field = out.ration;

        // `hunger` is the famine clock desertion waits on: consecutive ticks in
        // which the column is short enough to BREAK, not merely short.
        let hunger = p.hunger;
        let next = if r_field < FAMINE_RATION { (hunger + 1).min(1 << 20) } else { 0 };
        if next != hunger {
            p.hunger = next;
        }
        if mine.is_empty() {
            continue;
        }

        let mut forage_left = FORAGE_BUDGET;
        deserters.clear();

        for e in mine {
            let Ok((_, _, pos, _, mut u)) = q_units.get_mut(e.entity) else { continue };
            let mut r = if e.strain > Fx::ZERO { r_field } else { FULL_RATION };
            // An army in the field lives off the land. It is thin and it strips
            // the herd, so it buys a march and never a war.
            if r < FULL_RATION && forage_left > 0 {
                forage_left -= 1;
                let draw = man_draw(e.strain);
                let want = ((FULL_RATION - r) * draw).ceil().to_num::<i32>();
                let got = forage(&mut q_nodes, wild, pos.pos, want);
                if got > 0 && draw > Fx::ZERO {
                    r = (r + Fx::from_num(got) / draw).min(FULL_RATION);
                }
            }
            if u.ration != r {
                u.ration = r;
            }
            if r >= FULL_RATION {
                continue;
            }

            let ceiling = morale_ceiling(r);
            if u.morale > ceiling {
                u.morale = ceiling;
            }
            let fat = fatigue_ticks(r);
            if fat > 0 {
                u.attack_cd += fat;
            }
            // Hunger never kills a soldier. No game in this genre starves men
            // to death on the march - it costs you their spirit and then it
            // costs you the men themselves, who walk away rather than die
            // standing. Morale, fatigue and desertion carry the whole penalty.
            //
            // Men do not walk out the first evening without supper: the same
            // grace that used to hold off attrition is how long they put up
            // with it.
            if hunger >= STARVE_GRACE_TICKS
                && deserts(u.morale, unit_def(u.kind).morale_resolve, r)
            {
                deserters.push((unit_def(u.kind).morale_resolve, e.gid, e.entity));
            }
        }

        // resolve asc, then GameId: `gid` is unique, so the Entity in the key
        // never decides an ordering that would differ between peers
        deserters.sort_unstable();
        // A bleed measured against the COLUMN, not the whole muster roll. A
        // garrison at home is not what is breaking, and counting it would let a
        // big home army set the rate at which a small lost one empties.
        let afield = mine.iter().filter(|e| e.strain > Fx::ZERO).count();
        let cap = (afield / DESERT_DIVISOR).max(1);
        for &(_, _, ent) in deserters.iter().take(cap) {
            stats.of(p.player_id).lost += 1;
            commands.entity(ent).despawn();
        }
    }
}

type NodeData = (
    Entity,
    &'static GameId,
    &'static Pos,
    &'static mut ResourceNode,
    Option<&'static FieldOf>,
    Option<&'static mut Crop>,
);

/// One economy tick of a field's season, as a pure step over the two fields that
/// carry it. Growing, it takes what the soil and the crew give it and LATCHES
/// ripe at capacity — latched rather than derived, so a reaper drawing the field
/// down cannot un-ripen it at the first sheaf and thrash. Ripe, it counts the
/// ticks it has stood; past the grace (which a farm hub doubles) it lodges and
/// bleeds, still fully harvestable the whole way down. Reaped or lodged to
/// nothing, the season simply starts again — the row is never deleted.
fn season(n: &ResourceNode, c: &Crop, hands: i32, aura: i32) -> (i32, Crop) {
    let mut rem = n.remaining;
    let mut crop = *c;
    if !crop.ripe {
        rem = (rem + field_growth(hands, n.cap, aura)).min(n.cap);
        if rem >= n.cap {
            crop.ripe = true;
            crop.standing = 0;
        }
    } else {
        crop.standing += 1;
        let grace = if aura > 0 { FARM_RIPE_GRACE * 2 } else { FARM_RIPE_GRACE };
        if crop.standing > grace {
            rem = (rem - lodge_loss(n.cap)).max(0);
        }
    }
    if rem <= 0 {
        crop.ripe = false;
        crop.standing = 0;
    }
    (rem, crop)
}

/// Strip a wild herd within reach for at most `want` food — a man takes his
/// supper, not the whole beast, so one herd carries a column for a while.
/// `wild` is `GameId`-sorted, so which herd a hungry man finds is fixed across
/// peers.
fn forage(q_nodes: &mut Query<NodeData>, wild: &[(u64, Entity, V2)], at: V2, want: i32) -> i32 {
    if want <= 0 {
        return 0;
    }
    let r2 = FORAGE_RANGE * FORAGE_RANGE;
    for &(_, ent, p) in wild {
        if dist2(p, at) > r2 {
            continue;
        }
        let Ok((_, _, _, mut n, _, _)) = q_nodes.get_mut(ent) else { continue };
        let take = forage_yield(n.remaining).min(want);
        if take > 0 {
            n.remaining -= take;
            return take;
        }
    }
    0
}
