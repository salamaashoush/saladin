use super::{find_owned, owned_building_kinds, player_match, tech_mask_of};
use crate::components::*;
use crate::NextEntityId;
use bevy_ecs::prelude::*;
use saladin_sim::*;

/// Sell wood or stone for gold at the market rate. Mirrors `marketTrade`:
/// requires an owned Market, only raw resources trade, the sale rounds down to
/// whole lots so a coin is never minted for free.
pub(crate) fn market_trade(world: &mut World, owner: u64, res: ResourceType, amount: i32) {
    if !matches!(res, ResourceType::Wood | ResourceType::Stone) {
        return;
    }
    if !owned_building_kinds(world, owner).contains(&BuildingKind::Market) {
        return;
    }
    let mut q = world.query::<&mut Player>();
    let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) else { return };
    let sale = market_sale(p.stock.get(res), amount);
    if !sale.ok {
        return;
    }
    p.stock.add(res, -sale.spent);
    p.stock.gold += sale.gold;
}

/// Begin researching a Blacksmith tech for `owner`. Mirrors `startResearchFor`:
/// validates the host is an owned Blacksmith, the tech's prereq, no duplicate
/// (done or in flight), and affordability — then pays and inserts the research
/// row. Shared by the human command and the AI brain. Returns success.
pub(crate) fn start_research_at(world: &mut World, owner: u64, building: u64, tech: u8) -> bool {
    let Some(be) = find_owned(world, owner, building) else { return false };
    if world.get::<Building>(be).is_none_or(|b| b.kind != BuildingKind::Blacksmith) {
        return false;
    }
    start_research(world, owner, tech)
}

/// The building-agnostic core: the AI calls this once it knows it has a
/// Blacksmith; the command path authorizes the specific building first.
pub(crate) fn start_research(world: &mut World, owner: u64, tech: u8) -> bool {
    let Some(t) = Tech::from_u8(tech) else { return false };
    let up = upgrade_def(t);
    let mask = tech_mask_of(world, owner);
    if has_tech(mask, t) {
        return false;
    }
    let owned = owned_building_kinds(world, owner);
    if !owned.contains(&BuildingKind::Blacksmith) {
        return false;
    }
    if !has_prereq(&owned, up.requires) {
        return false;
    }
    // one row per tech in flight
    let in_flight = {
        let mut q = world.query::<&Research>();
        q.iter(world).any(|r| r.owner == owner && r.tech == tech)
    };
    if in_flight {
        return false;
    }
    let Some(match_id) = player_match(world, owner) else { return false };
    {
        let mut q = world.query::<&mut Player>();
        let Some(mut p) = q.iter_mut(world).find(|p| p.player_id == owner) else { return false };
        if !p.stock.can_afford(&up.cost) {
            return false;
        }
        p.stock.pay(&up.cost);
    }
    let id = world.resource_mut::<NextEntityId>().alloc();
    world.spawn((
        GameId(id),
        MatchId(match_id),
        Research { owner, tech, progress: Fx::ZERO, done: false },
    ));
    true
}
