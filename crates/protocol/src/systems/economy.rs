use crate::MatchStatuses;
use crate::WorldConfig;
use crate::components::{
    Building, Crop, FieldOf, GameId, MatchId, Owner, Player, Pos, ResourceNode, Unit,
};
use bevy_ecs::prelude::*;
use bevy_platform::collections::HashMap;
use saladin_sim::{
    AuraTarget, ECONOMY_DT, FARM_RIPE_GRACE, FOOD_PER_UNIT, FULL_RATION, Fx,
    MORALE_MAX, OUT_OF_SUPPLY_DRAW, ResourceType, SUPPLY_RADIUS, SupplyResult, V2, WorkAura,
    apply_supply, building_def, deserts, dist, dist2, draws_rations, field_growth, forage_yield,
    is_sailable, lodge_loss, operational, ration, supply::STARVE_GRACE_TICKS, supply_bill,
    unit_def,
};

/// The heart a man keeps on a given ration. Full commons cost nothing; an empty
/// larder pins him exactly ON the breaking point, so the next blow breaks him;
/// everything between scales. Hunger makes an army BRITTLE — it does not rout it
/// where it stands and it does not execute it. Starving men desert, they do not
/// flee an enemy who is not there.
///
/// This is a CEILING, not the drain `SupplyResult` also offers, and the reason
/// is measured: a packed formation recovers about +0.34 morale per economy tick
/// (six allies capped, 0.2 s cadence) against a largest-possible famine drain of
/// 0.30, so a drain-based hunger is invisible in exactly the formation that most
/// needs feeding. A ceiling binds regardless of how many friends are standing
/// round the fire.
const STARVED_CEILING: Fx = saladin_sim::ROUT_THRESHOLD;

/// Above this share of a ration men are TIRED, not dying. `apply_supply` floors
/// its attrition at 1 hp, so without this gate ANY shortfall — a single loaf
/// short of a hundred — is eventually fatal, which is the death spiral this
/// rework exists to remove.
const ATTRITION_RATION: Fx = saladin_sim::fx!("0.5");

/// Combat ticks of extra rest a man on nothing at all takes between blows,
/// added once per economy tick (ten combat ticks). Tired troops swing slower
/// before they fall over.
const FATIGUE_TICKS: i32 = 3;

/// Foragers served per player per economy tick, in `GameId` order — a budget in
/// the shape of combat's pursuit budget, so a whole starving host cannot walk
/// every wild herd on the map.
const FORAGE_BUDGET: usize = 256;

/// How close a man stands to a wild herd to live off it.
const FORAGE_RANGE: Fx = saladin_sim::fx!("3");

/// One deserter per this many mouths per economy tick. The cap is what makes an
/// army BLEED men instead of evaporating in a single tick.
const DESERT_DIVISOR: usize = 8;

/// Denominator for `one_mouth` — a single ration expressed in whole numbers so
/// `apply_supply` can be asked about one man.
const RATION_SCALE: i32 = 4096;

fn morale_ceiling(r: Fx) -> Fx {
    STARVED_CEILING + (MORALE_MAX - STARVED_CEILING) * r.clamp(Fx::ZERO, FULL_RATION)
}

fn fatigue_ticks(r: Fx) -> i32 {
    (Fx::from_num(FATIGUE_TICKS) * (FULL_RATION - r).max(Fx::ZERO)).round().to_num::<i32>()
}

/// The famine as ONE man sees it. The grace, the attrition ramp and the
/// shortfall scaling all live in `apply_supply`; this hands it a larder of a
/// single mouth's size rather than restating any of them here.
fn one_mouth(r: Fx, hunger: i32) -> SupplyResult {
    let bill = Fx::from_num(RATION_SCALE);
    let got = (r.clamp(Fx::ZERO, FULL_RATION) * bill).round().to_num::<i32>();
    apply_supply(got, bill, hunger, ECONOMY_DT)
}

struct Eater {
    owner: u64,
    gid: u64,
    entity: Entity,
    /// Beyond the reach of any friendly store.
    far: bool,
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

/// A player with no store anywhere has no supply LINE to be cut: his men carry
/// what they have and every mouth costs the same wherever it stands. Without
/// this a side would be punished for owning nothing to lose, and the case is
/// unreachable in a real match — the Keep is a drop-off and losing it is defeat.
fn in_supply(anchors: &[(u64, V2)], owner: u64, at: V2) -> bool {
    let r2 = SUPPLY_RADIUS * SUPPLY_RADIUS;
    let lo = anchors.partition_point(|(o, _)| *o < owner);
    let mut mine = anchors[lo..].iter().take_while(|(o, _)| *o == owner).peekable();
    if mine.peek().is_none() {
        return true;
    }
    mine.any(|(_, p)| dist2(*p, at) <= r2)
}

/// Supply, regrowth and the muster roll — every economy tick (2 s).
///
/// Rations are PROPORTIONAL and issued in two bands: the men in reach of a
/// store eat first, the column at the far end of the supply line eats what is
/// left and pays a carter's premium for it. A shortfall of one man's food costs
/// one man's food. The old rule was `bill > food` — one loaf short and every
/// soldier on the map starved at the same instant, which is a punishment rather
/// than a decision.
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
            far: !in_supply(&s.anchors, owner.0, pos.pos),
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
        let far_n = mine.iter().filter(|e| e.far).count() as i32;
        let near_n = mine.len() as i32 - far_n;

        // Two bands, fed in order: the garrison at the stores, then the column
        // in the field — which is also the half of a siege that costs the
        // BESIEGER something.
        let bill_near = supply_bill(near_n, 0);
        let bill_far = supply_bill(far_n, far_n);
        let food = p.stock.food;
        let out = apply_supply(food, bill_near + bill_far, p.hunger, ECONOMY_DT);
        if out.food != food {
            p.stock.food = out.food;
        }
        let r_near = ration(food, bill_near);
        let leftover = (Fx::from_num(food) - bill_near).max(Fx::ZERO).floor().to_num::<i32>();
        let r_far = ration(leftover, bill_far);

        // `hunger` is the famine clock the attrition ramp reads: it counts
        // consecutive ticks in which somebody is short enough to WASTE, not
        // merely short.
        let worst = if far_n > 0 { r_far } else { r_near };
        let hunger = p.hunger;
        let next = if worst < ATTRITION_RATION { (hunger + 1).min(1 << 20) } else { 0 };
        if next != hunger {
            p.hunger = next;
        }
        if mine.is_empty() {
            continue;
        }

        let near_eff = one_mouth(r_near, hunger);
        let far_eff = one_mouth(r_far, hunger);
        let mut forage_left = FORAGE_BUDGET;
        deserters.clear();

        for e in mine {
            let Ok((_, _, pos, _, mut u)) = q_units.get_mut(e.entity) else { continue };
            let mut r = if e.far { r_far } else { r_near };
            let mut own_eff = None;
            // An army in the field lives off the land. It is thin and it strips
            // the herd, so it buys a march and never a war.
            if e.far && r < FULL_RATION && forage_left > 0 {
                forage_left -= 1;
                let draw = Fx::from_num(FOOD_PER_UNIT) * OUT_OF_SUPPLY_DRAW;
                let want = ((FULL_RATION - r) * draw).ceil().to_num::<i32>();
                let got = forage(&mut q_nodes, wild, pos.pos, want);
                if got > 0 {
                    r = (r + Fx::from_num(got) / draw).min(FULL_RATION);
                    own_eff = Some(one_mouth(r, hunger));
                }
            }
            if u.ration != r {
                u.ration = r;
            }
            if r >= FULL_RATION {
                continue;
            }
            let eff = own_eff.as_ref().unwrap_or(if e.far { &far_eff } else { &near_eff });

            let ceiling = morale_ceiling(r);
            if u.morale > ceiling {
                u.morale = ceiling;
            }
            let fat = fatigue_ticks(r);
            if fat > 0 {
                u.attack_cd += fat;
            }
            if r < ATTRITION_RATION && eff.hp_drain > 0 {
                let hp = (u.hp - eff.hp_drain).max(0);
                if hp <= 0 {
                    stats.of(p.player_id).lost += 1;
                    commands.entity(e.entity).despawn();
                    continue;
                }
                u.hp = hp;
            }
            // Men do not walk out the first evening without supper: the same
            // grace that holds off attrition is how long they put up with it.
            if hunger >= STARVE_GRACE_TICKS
                && deserts(u.morale, unit_def(u.kind).morale_resolve, r)
            {
                deserters.push((unit_def(u.kind).morale_resolve, e.gid, e.entity));
            }
        }

        // resolve asc, then GameId: `gid` is unique, so the Entity in the key
        // never decides an ordering that would differ between peers
        deserters.sort_unstable();
        let cap = (mine.len() / DESERT_DIVISOR).max(1);
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
