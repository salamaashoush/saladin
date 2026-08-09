//! In-game HUD (port of HUD.tsx + ResourceBar/CommandCard/BuildBar/
//! ResearchPanel/Toasts): a top resource bar, top-right match buttons, and a
//! bottom bar with the command card (selection / building), the tabbed build
//! bar with costs + tech locks, research panel and garrison group.
//!
//! The resource bar updates its Text values in place; the bottom bar sections
//! rebuild when their state digest changes (selection, stock, tab, mode...).

use super::actions::{BuildTab, MARKET_LOT, UiAction};
use super::assets::UiAssets;
use super::theme::*;
use super::widgets::*;
use crate::input::{InputMode, PlaceHint};
use crate::selection::{CropInfo, FormationPick, SelectedBuilding, SelectionInfo};
use crate::{LocalPlayer, UiFont};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use saladin_sim::{
    AuraTarget, BUILD_CATEGORIES, BuildState, BuildStatus, BuildingDef, BuildingKind, FULL_RATION,
    FormationShape, Fx, ResearchProgressRow, ResearchStatus, ResourceCost, ResourceType,
    Stance, UnitRole, V2, accepts, building_def, build_panel_state, can_host_garrison,
    cancel_refund, demolish_refund, dist, draws_rations, hall_label, operational,
    place_error_text, research_panel_state, roster_for, techs_in_mask, unit_def, upgrade_def,
};
use saladin_protocol::{Building, Owner, Player, Pos, Research, Unit};
use std::collections::HashSet;

#[derive(Component)]
pub struct HudRoot;

/// A panel that eats pointer input. The click-through band used to be a
/// hardcoded `y > height - 120` against a bar measuring 182 logical px, so 67px
/// of live panel swallowed nothing — and UI scale moved it in both directions.
/// The band is now the panels' own measured rects.
#[derive(Component)]
pub struct HudBlocker;

/// Logical-pixel rects of every `HudBlocker`, refreshed each frame.
#[derive(Resource, Default)]
pub struct HudRects(pub Vec<Rect>);

impl HudRects {
    pub fn hit(&self, cursor: Vec2) -> bool {
        self.0.iter().any(|r| r.contains(cursor))
    }
}

pub fn measure_hud(
    mut rects: ResMut<HudRects>,
    q: Query<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), With<HudBlocker>>,
) {
    rects.0.clear();
    for (n, t, vis) in &q {
        if !vis.get() {
            continue;
        }
        let inv = n.inverse_scale_factor();
        let size = n.size() * inv;
        if size.x <= 0.0 || size.y <= 0.0 {
            continue;
        }
        rects.0.push(Rect::from_center_size(t.translation * inv, size));
    }
}

#[derive(Component)]
pub struct ResourceText(pub usize); // 0..=7: name,wood,stone,food,gold,peasants,army,pop

/// Command card geometry. The card is a fixed-width column, so every text node
/// in it gets that exact width (see `wrap_label`).
///
/// The inset is a MARGIN on the content, never padding on the panel: an
/// `ImageNode` fills the node's CONTENT box, so padding would push the
/// parchment inward and leave the text hugging its own frame.
const CARD_W: f32 = 250.0;
const CARD_PAD: f32 = 16.0;
const CARD_TEXT_W: f32 = CARD_W - CARD_PAD * 2.0;
const BAR_PAD_X: f32 = 18.0;
const BAR_PAD_Y: f32 = 14.0;

#[derive(Component)]
pub struct BottomLeft; // command card container
#[derive(Component)]
pub struct BottomCenter; // build bar container

/// Digest of everything the bottom bar renders — rebuild when it changes.
#[derive(Resource, Default, PartialEq, Clone)]
pub struct HudDigest(String);

pub fn setup_hud(mut commands: Commands, font: Res<UiFont>, assets: Res<UiAssets>) {
    // top resource bar
    commands
        .spawn((
            HudRoot,
            HudBlocker,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(26.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(9.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                ..default()
            },
            panel_bg_dark(&assets),
        ))
        .with_children(|p| {
            let icon_keys = [None, Some("res:wood"), Some("res:stone"), Some("res:food"), Some("res:gold"), None, None, None];
            for i in 0..8 {
                if let Some(h) = icon_keys[i].and_then(|k| assets.icon(k)) {
                    p.spawn((
                        Node { width: Val::Px(16.0), height: Val::Px(16.0), margin: UiRect::right(Val::Px(-5.0)), ..default() },
                        ImageNode::new(h),
                    ));
                }
                p.spawn((
                    ResourceText(i),
                    Text::new(""),
                    TextFont { font: font.0.clone().into(), font_size: FontSize::Px(FONT_MD), font_smoothing: bevy::text::FontSmoothing::None, ..default() },
                    TextColor(if i == 0 { ACCENT } else { TEXT }),
                    bevy::text::LineHeight::RelativeToFont(1.3),
                ));
            }
        });

    // bottom bar containers (left card / center build bar; right = minimap viewport)
    commands.spawn((
        HudRoot,
        HudBlocker,
        BottomLeft,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(5.0),
            left: Val::Px(5.0),
            width: Val::Px(CARD_W),
            min_height: Val::Px(150.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            ..default()
        },
        panel_bg(&assets),
    ));
    commands.spawn((
        HudRoot,
        HudBlocker,
        BottomCenter,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(5.0),
            left: Val::Px(CARD_W + 10.0),
            right: Val::Px(172.0),
            min_height: Val::Px(178.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::FlexStart,
            overflow: Overflow::clip(),
            ..default()
        },
        panel_bg(&assets),
    ));

    // minimap frame (the minimap itself is a camera viewport bottom-right)
    commands.spawn((
        HudRoot,
        HudBlocker,
        super::assets::MinimapFrame,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(0.0),
            height: Val::Px(0.0),
            ..default()
        },
        ImageNode::new(assets.bar_frame.clone())
            .with_mode(bevy::ui::widget::NodeImageMode::Sliced(UiAssets::bar_slicer())),
    ));
}

pub fn cleanup_hud(mut commands: Commands, q: Query<Entity, With<HudRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

fn my_player<'a>(players: &'a Query<&Player>, me: u64) -> Option<&'a Player> {
    players.iter().find(|p| p.player_id == me)
}

/// What the army is actually eating, and what the road is costing. A garrison
/// draws NOTHING, so a food number alone tells a player nothing about the
/// decision he has made — the bar has to say how many men are past the end of
/// the supply line and what they are billing for it.
///
/// `afield` and `bill` are recomputed here through the sim's own `strain`, not
/// mirrored off a sim field: `Unit.ration` is FULL for a man who is merely
/// expensive, and the cost of a march has to be visible BEFORE it turns into a
/// shortage.
pub struct SupplyReadout {
    pub eaters: i32,
    /// Men beyond the reach of any friendly store.
    pub afield: i32,
    pub short: i32,
    /// Worst ration in the army, as a percentage.
    pub worst: i32,
    /// What the column costs, in food per minute.
    pub bill: i32,
}

impl SupplyReadout {
    pub fn line(&self, food: i32) -> (String, bool) {
        match (self.short, self.worst, self.afield) {
            (0, _, 0) => (format!("{food}"), false),
            (0, _, a) => (format!("{food}  {a} AFIELD  -{}/min", self.bill), false),
            (n, 0, _) => (format!("{food}  NO RATIONS  {n} men"), true),
            (n, w, _) => (format!("{food}  RATIONS {w}%  {n} men"), true),
        }
    }
}

/// Economy ticks in a minute — what turns a per-tick bill into a rate a player
/// can compare with the food bar he is watching.
const TICKS_PER_MIN: f32 = 30.0;

pub fn supply_readout(
    army: impl Iterator<Item = (saladin_sim::UnitKind, Fx, V2)>,
    stores: &[V2],
) -> SupplyReadout {
    let (mut eaters, mut afield, mut short, mut worst) = (0, 0, 0, FULL_RATION);
    let mut strain_sum = Fx::ZERO;
    for (kind, r, at) in army {
        if !draws_rations(kind) {
            continue;
        }
        eaters += 1;
        if r < FULL_RATION {
            short += 1;
            worst = worst.min(r);
        }
        // no store anywhere is no supply LINE to be cut, exactly as the sim has it
        let Some(nearest) = stores.iter().map(|s| dist(*s, at)).reduce(Fx::min) else { continue };
        let st = saladin_sim::strain(nearest);
        if st > Fx::ZERO {
            afield += 1;
            strain_sum += st;
        }
    }
    SupplyReadout {
        eaters,
        afield,
        short,
        worst: (worst.to_num::<f32>() * 100.0).clamp(0.0, 100.0) as i32,
        bill: (saladin_sim::supply_bill(strain_sum).to_num::<f32>() * TICKS_PER_MIN).ceil() as i32,
    }
}

/// Where a haul may be dropped, which is also where an army is fed from — the
/// same gate `systems::economy` uses to build its anchor list.
pub fn supply_stores<'a>(
    buildings: impl Iterator<Item = (&'a Owner, &'a Pos, &'a Building)>,
    me: u64,
) -> Vec<V2> {
    buildings
        .filter(|(o, _, b)| o.0 == me && operational(b.state) && building_def(b.kind).accepts != 0)
        .map(|(_, p, _)| p.pos)
        .collect()
}

/// Refresh the top bar texts in place.
pub fn update_resource_bar(
    local: Res<LocalPlayer>,
    q_players: Query<&Player>,
    q_units: Query<(&Owner, &Pos, &Unit)>,
    q_buildings: Query<(&Owner, &Pos, &Building)>,
    mut q_text: Query<(&ResourceText, &mut Text, &mut TextColor)>,
) {
    let Some(p) = my_player(&q_players, local.0) else { return };
    let (mut peasants, mut soldiers, mut pop) = (0, 0, 0);
    for (o, _, u) in &q_units {
        if o.0 != local.0 {
            continue;
        }
        pop += unit_def(u.kind).pop_cost;
        if u.kind == saladin_sim::UnitKind::Peasant {
            peasants += 1;
        }
        // an Imam is in the army even though he never swings: role, not attack.
        // A hull is not — it is shipping.
        if !matches!(unit_def(u.kind).role, UnitRole::Worker | UnitRole::Boat) {
            soldiers += 1;
        }
    }
    // a hole in the ground shelters nobody — mirror the sim's pop_room gate
    let cap: i32 = q_buildings
        .iter()
        .filter(|(o, _, b)| o.0 == local.0 && operational(b.state))
        .map(|(_, _, b)| building_def(b.kind).pop)
        .sum();
    let stores = supply_stores(q_buildings.iter(), local.0);
    let supply = supply_readout(
        q_units.iter().filter(|(o, _, _)| o.0 == local.0).map(|(_, pos, u)| (u.kind, u.ration, pos.pos)),
        &stores,
    );
    let (food_line, short) = supply.line(p.stock.food);

    for (slot, mut text, mut color) in &mut q_text {
        let (s, c) = match slot.0 {
            0 => (format!("{}  ({:?})", p.name, p.faction), ACCENT),
            1 => (format!("{}", p.stock.wood), TEXT),
            2 => (format!("{}", p.stock.stone), TEXT),
            3 => (food_line.clone(), if short { WARN } else { TEXT }),
            4 => (format!("{}", p.stock.gold), GOLD),
            5 => (format!("Peasants {peasants}"), TEXT),
            6 => (format!("Army {soldiers}"), TEXT),
            _ => (format!("Pop {pop}/{cap}"), if pop >= cap { WARN } else { TEXT }),
        };
        if **text != s {
            **text = s;
        }
        color.0 = c;
    }
}

/// Rebuild the bottom bar when its digest changes.
#[allow(clippy::too_many_arguments)]
pub fn update_bottom_bar(
    mut commands: Commands,
    font: Res<UiFont>,
    assets: Res<UiAssets>,
    local: Res<LocalPlayer>,
    info: Res<SelectionInfo>,
    sel_building: Res<SelectedBuilding>,
    tab: Res<BuildTab>,
    mode: Res<InputMode>,
    shape: Res<FormationPick>,
    mut digest: ResMut<HudDigest>,
    q_players: Query<&Player>,
    q_buildings: Query<(&Owner, &Building)>,
    q_research: Query<&Research>,
    q_left: Query<Entity, With<BottomLeft>>,
    q_center: Query<Entity, With<BottomCenter>>,
) {
    let Some(p) = my_player(&q_players, local.0) else { return };
    // mirror the sim's BuildContext exactly: a SITE counts toward the per-kind
    // limit but satisfies no prereq, because an unfinished barracks trains
    // nothing. A build card that disagrees with the command is a UI lie.
    let mut owned: HashSet<BuildingKind> = HashSet::new();
    let mut counts = [0i32; BuildingKind::ALL.len()];
    for (o, b) in &q_buildings {
        if o.0 != local.0 {
            continue;
        }
        counts[b.kind as usize] += 1;
        if saladin_sim::operational(b.state) {
            owned.insert(b.kind);
        }
    }
    let rows: Vec<ResearchProgressRow> = q_research
        .iter()
        .filter(|r| r.owner == local.0)
        .map(|r| ResearchProgressRow { tech: r.tech, progress: r.progress, done: r.done })
        .collect();

    let sb = &*sel_building;
    let key = format!(
        "{:?}|{:?}|{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{:.2}|{:.2}|{}|{}/{}|{:?}|{:.2}|{}|{:?}|{:.2}|{}|{:?}|{:?}|{}|{}|{:?}|{:?}",
        p.stock,
        info.by_kind,
        info.total,
        info.routing,
        sb.id,
        sb.occupants,
        tab.0,
        *mode,
        counts,
        owned.len(),
        info.avg_hp,
        info.avg_morale,
        rows.iter().map(|r| format!("{}:{:.2}:{}", r.tech, r.progress.to_num::<f32>(), r.done)).collect::<Vec<_>>().join(","),
        sb.hp,
        sb.max_hp,
        sb.state,
        sb.work,
        sb.builders,
        sb.queue,
        sb.train_progress,
        sb.rally.is_some(),
        sb.upgrade.map(|(k, _)| k),
        shape.0,
        info.short,
        (info.worst_ration * 100.0) as i32,
        p.faction,
        sb.crop,
    ) + &format!("|{}/{}", info.aboard, info.berths);
    if digest.0 == key {
        return;
    }
    digest.0 = key;

    let Ok(left) = q_left.single() else { return };
    let Ok(center) = q_center.single() else { return };
    commands.entity(left).despawn_related::<Children>();
    commands.entity(center).despawn_related::<Children>();

    build_command_card(&mut commands, left, &font, &assets, &info, sb, p, shape.0, *mode);
    build_build_bar(
        &mut commands, center, &font, &assets, p, &owned, &counts, &rows, sb, tab.0, *mode,
    );
}

/// "70W 20S" plus the labour it takes — a price is a time as well as a cost.
fn price_line(cost: &ResourceCost, secs: Fx) -> String {
    let c = cost_line(cost);
    let s = secs.to_num::<i32>();
    match (c.is_empty(), s > 0) {
        (true, true) => format!("{s}s"),
        (false, true) => format!("{c}  {s}s"),
        (true, false) => "Free".into(),
        (false, false) => c,
    }
}

/// Where the season stands, in three words a player can read without counting.
fn crop_stage_line(crop: &CropInfo) -> (&'static str, Color) {
    match (crop.lodging, crop.ripe) {
        (true, _) => ("Crop is lodging", WARN),
        (_, true) => ("Harvest ready", GOLD),
        _ => ("Growing", ACCENT),
    }
}

fn farm_hands(hands: i32) -> String {
    match hands {
        0 => "no farmhands".into(),
        1 => "1 farmhand".into(),
        n => format!("{n} farmhands"),
    }
}

/// The two axes a field is a function of, on their OWN lines. They shared one
/// before, and `line` takes one colour, so an empty field painted "Rich soil"
/// in alarm red — the card's colour rule is "this needs you", and good ground
/// never does.
fn farm_line(crop: &CropInfo) -> String {
    format!("Soil  {}", crop.soil_word())
}

fn hands_line(crop: &CropInfo) -> (String, Color) {
    (farm_hands(crop.hands), if crop.hands == 0 { WARN } else { TEXT_DIM })
}

/// The hold of a selected ferry, and how to work it. A laden barge and an empty
/// one were the same row on the card, and unloading is a right-click on nearby
/// ground with nothing else to tell the player it is about to happen.
fn cargo_lines(aboard: u32, berths: u32) -> (String, &'static str) {
    let how = if aboard > 0 {
        "Right-click near shore to land"
    } else {
        "Right-click the hull with men picked"
    };
    (format!("Aboard {aboard}/{berths}"), how)
}

fn crew_line(builders: i32) -> String {
    match builders {
        0 => "no builders".into(),
        1 => "1 builder".into(),
        n => format!("{n} builders"),
    }
}

/// What this structure DOES, read straight off the def fields. A new role is a
/// row in BUILDING_DEFS, so it shows up here without touching the HUD.
fn role_lines(def: &BuildingDef, sel: &SelectedBuilding) -> Vec<String> {
    let mut out = Vec::new();
    if def.accepts != 0 {
        let names: Vec<&str> = [
            (ResourceType::Wood, "Wood"),
            (ResourceType::Stone, "Stone"),
            (ResourceType::Food, "Food"),
            (ResourceType::Gold, "Gold"),
        ]
        .iter()
        .filter(|(r, _)| accepts(def, *r))
        .map(|(_, n)| *n)
        .collect();
        out.push(format!("Drops off {}", names.join(" ")));
    }
    // the roster a hall offers is the faction's, not the table's: an Ayyubid
    // Stable must not advertise a Knight it can never train
    if !def.trains.is_empty() {
        let roster = roster_for(sel.kind, sel.faction);
        let names: Vec<&str> = roster.iter().map(|k| unit_def(*k).label).collect();
        out.push(format!("Trains {}", names.join(", ")));
    }
    if let Some(a) = def.aura {
        out.push(match a.target {
            AuraTarget::Field => "Speeds nearby fields".into(),
            AuraTarget::WaterFood => "Speeds and restocks nearby fishing".into(),
        });
    }
    if def.min_fertility > Fx::ZERO {
        out.push("Farmhands work and reap this field".into());
    }
    if def.hosts_research {
        out.push("Researches upgrades".into());
    }
    if def.enables_trade {
        out.push("Trades goods for gold".into());
    }
    if def.morale_radius > Fx::ZERO {
        out.push("Steadies nearby troops".into());
    }
    if def.attack > 0 {
        out.push(format!("Fires on raiders at {}", def.range.to_num::<i32>()));
    }
    match (def.pop > 0, def.garrison_cap > 0) {
        (true, true) => {
            out.push(format!("Houses {}   Garrison {}/{}", def.pop, sel.occupants, sel.garrison_cap))
        }
        (true, false) => out.push(format!("Houses {}", def.pop)),
        (false, true) => out.push(format!("Garrison {}/{}", sel.occupants, sel.garrison_cap)),
        (false, false) => {}
    }
    if let Some(into) = def.upgrades_to {
        out.push(format!("Becomes a {}", building_def(into).label));
    }
    if def.defeat_on_death {
        out.push("Its fall ends the war".into());
    }
    out
}

/// Marching order names. ASCII only and short enough for a 40px chip — the
/// embedded font has no other glyphs and the atlas pre-warm is ASCII-only.
pub const FORMATION_NAMES: [(FormationShape, &str); 4] = [
    (FormationShape::Line, "Line"),
    (FormationShape::Column, "Col"),
    (FormationShape::Wedge, "Wedge"),
    (FormationShape::Box, "Box"),
];

#[allow(clippy::too_many_arguments)]
fn build_command_card(
    commands: &mut Commands,
    left: Entity,
    font: &UiFont,
    assets: &UiAssets,
    info: &SelectionInfo,
    sel_building: &SelectedBuilding,
    p: &Player,
    shape: FormationShape,
    mode: InputMode,
) {
    commands.entity(left).with_children(|c| {
      c.spawn((Node {
          flex_direction: FlexDirection::Column,
          row_gap: Val::Px(4.0),
          margin: UiRect::all(Val::Px(CARD_PAD)),
          ..default()
      },))
      .with_children(|c| {
        let line = |c: &mut ChildSpawnerCommands, t: &str, size: f32, col: Color| {
            wrap_label(c, font, t, size, col, CARD_TEXT_W);
        };
        if info.total > 0 {
            line(c, "Selection", FONT_SM, TEXT_DIM);
            line(c, &format!("{} unit{}", info.total, if info.total > 1 { "s" } else { "" }), FONT_MD, TEXT);
            for (kind_idx, &count) in info.by_kind.iter().enumerate() {
                if count == 0 {
                    continue;
                }
                let kind = saladin_sim::UnitKind::from_u8(kind_idx as u8).unwrap();
                let base = unit_def(kind);
                let eff = saladin_sim::effective_unit_def(kind, p.tech_mask);
                let up = if eff.attack != base.attack || eff.max_hp != base.max_hp { " ^" } else { "" };
                line(c, &format!("{}{}  x{}", base.label, up, count), FONT_SM, TEXT);
            }
            if info.has_combat {
                c.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(2.0), ..default() },))
                    .with_children(|c| {
                        for (stance, name, key) in [
                            (Stance::Aggressive, "Attack", "G"),
                            (Stance::Defensive, "Defend", "F"),
                            (Stance::HoldGround, "Hold", "H"),
                        ] {
                            tool_button(
                                c,
                                font,
                                assets,
                                UiAction::Stance(stance),
                                name,
                                Some(key.to_string()),
                                BtnStyle { min_width: 40.0, icon: assets.stance_icon(stance), ..default() },
                            );
                        }
                    });
                line(c, "Orders", 12.0, TEXT_DIM);
                c.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(2.0), ..default() },))
                    .with_children(|c| {
                        tool_button(
                            c,
                            font,
                            assets,
                            UiAction::ArmAttackMove,
                            "Adv",
                            Some("V".into()),
                            BtnStyle {
                                min_width: 40.0,
                                active: mode == InputMode::AttackMove,
                                tint: TINT_RED,
                                ..default()
                            },
                        );
                        tool_button(
                            c,
                            font,
                            assets,
                            UiAction::StopSelected,
                            "Stop",
                            Some("X".into()),
                            BtnStyle { min_width: 40.0, ..default() },
                        );
                    });
                line(c, "March", 12.0, TEXT_DIM);
                // four chips at the standard 64px width overflow the 218px card
                // and the last one is clipped off the panel
                c.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    width: Val::Px(CARD_TEXT_W),
                    column_gap: Val::Px(2.0),
                    row_gap: Val::Px(2.0),
                    ..default()
                },))
                    .with_children(|c| {
                        for (s, name) in FORMATION_NAMES {
                            tool_button(
                                c,
                                font,
                                assets,
                                UiAction::Formation(s),
                                name,
                                None,
                                BtnStyle {
                                    min_width: 50.0,
                                    min_height: 28.0,
                                    active: s == shape,
                                    ..default()
                                },
                            );
                        }
                    });
            }
            if info.berths > 0 {
                let (hold, how) = cargo_lines(info.aboard, info.berths);
                line(c, &hold, 12.0, TEXT_DIM);
                line(c, how, 11.0, TEXT_DIM);
            }
            if info.short > 0 {
                let pct = (info.worst_ration * 100.0) as i32;
                line(c, &format!("On {pct}% rations   {} men", info.short), 12.0, WARN);
            }
            line(c, "Health", 12.0, TEXT_DIM);
            ratio_bar(c, assets, CARD_TEXT_W, info.avg_hp, hp_color(info.avg_hp));
            if info.has_combat {
                let routing = if info.routing > 0 { format!("Morale   {} routing!", info.routing) } else { "Morale".into() };
                line(c, &routing, 12.0, if info.routing > 0 { WARN } else { TEXT_DIM });
                ratio_bar(c, assets, CARD_TEXT_W, info.avg_morale, morale_color(info.avg_morale));
            }
        } else if sel_building.id.is_some() {
            building_panel(c, font, assets, sel_building);
        } else {
            line(c, "No selection", FONT_SM, TEXT_DIM);
            line(c, "Drag to select units.", 12.0, TEXT_DIM);
            line(c, "Right-click to order.", 12.0, TEXT_DIM);
            for (key, what) in crate::input::HOTKEY_HELP {
                line(c, &format!("{key}  {what}"), 11.0, TEXT_DIM);
            }
        }
      });
    });
}

/// The selected building's own panel: what it is, how it is, what it is doing.
fn building_panel(
    c: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    sel: &SelectedBuilding,
) {
    let def = building_def(sel.kind);
    let line = |c: &mut ChildSpawnerCommands, t: &str, size: f32, col: Color| {
        wrap_label(c, font, t, size, col, CARD_TEXT_W);
    };
    line(c, hall_label(sel.kind, sel.faction), FONT_MD, ACCENT);
    line(c, def.blurb, 11.0, TEXT_DIM);
    match sel.state {
        BuildState::Site => line(c, "Under construction", FONT_SM, GOLD),
        BuildState::Upgrading => {
            line(c, &format!("Becoming {}", hall_label(sel.target_kind, sel.faction)), FONT_SM, GOLD)
        }
        BuildState::Complete if sel.damaged() => line(c, "Damaged", FONT_SM, WARN),
        BuildState::Complete => {}
    }

    // on a farm the SEASON is the headline and health is the footnote: a plot
    // is never the thing an enemy shoots first
    if let Some(crop) = sel.crop {
        let (stage, col) = crop_stage_line(&crop);
        line(c, stage, FONT_SM, col);
        line(c, &format!("Crop  {}/{}", crop.remaining, crop.cap), 12.0, TEXT_DIM);
        ratio_bar(c, assets, CARD_TEXT_W, crop.fill(), if crop.ripe { GOLD } else { HP_GREEN });
        line(c, &farm_line(&crop), 12.0, TEXT_DIM);
        let (hands, col) = hands_line(&crop);
        line(c, &hands, 12.0, col);
        if let (true, Some((kind, _))) = (crop.tended, sel.hub) {
            line(c, &format!("Tended by {}", building_def(kind).label), 12.0, ACCENT);
        }
    }

    let hp = (sel.hp as f32 / sel.max_hp.max(1) as f32).clamp(0.0, 1.0);
    line(c, &format!("Health  {}/{}", sel.hp, sel.max_hp), 12.0, TEXT_DIM);
    ratio_bar(c, assets, CARD_TEXT_W, hp, hp_color(hp));

    // progress and health answer two different questions, so they are two bars
    if sel.state != BuildState::Complete {
        let idle = sel.builders == 0;
        let pct = (sel.work * 100.0) as i32;
        line(
            c,
            &format!("Building {pct}%  {}", crew_line(sel.builders)),
            12.0,
            if idle { WARN } else { TEXT_DIM },
        );
        ratio_bar(c, assets, CARD_TEXT_W, sel.work, GOLD);
    } else if sel.builders > 0 && (sel.crop.is_none() || sel.damaged()) {
        // a farm's crew is not mending it — the farm line already counts them,
        // and calling a reaper a builder is exactly the lie this card is for
        line(c, &format!("Mending  {}", crew_line(sel.builders)), 12.0, TEXT_DIM);
    }

    if let Some(k) = sel.queue.first() {
        let more = sel.queue.len() - 1;
        let tail = if more > 0 { format!("  +{more}") } else { String::new() };
        line(c, &format!("Training {}{tail}", unit_def(*k).label), 12.0, TEXT_DIM);
        ratio_bar(c, assets, CARD_TEXT_W, sel.train_progress, ACCENT);
    }

    for role in role_lines(def, sel) {
        line(c, &role, FONT_SM, TEXT);
    }
    match sel.rally {
        Some(r) => {
            line(c, &format!("Rally {}, {}", r.x.to_num::<i32>(), r.y.to_num::<i32>()), 11.0, GOLD)
        }
        None if !def.trains.is_empty() => line(c, "Right-click sets rally", 11.0, TEXT_DIM),
        None => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn build_build_bar(
    commands: &mut Commands,
    center: Entity,
    font: &UiFont,
    assets: &UiAssets,
    p: &Player,
    owned: &HashSet<BuildingKind>,
    counts: &[i32; BuildingKind::ALL.len()],
    rows: &[ResearchProgressRow],
    sel_building: &SelectedBuilding,
    tab: usize,
    mode: InputMode,
) {
    let stock = p.stock;
    commands.entity(center).with_children(|c| {
      c.spawn((Node {
          flex_direction: FlexDirection::Row,
          align_items: AlignItems::FlexStart,
          column_gap: Val::Px(16.0),
          margin: UiRect::axes(Val::Px(BAR_PAD_X), Val::Px(BAR_PAD_Y)),
          ..default()
      },))
      .with_children(|c| {
        if sel_building.id.is_some() {
            let bdef = building_def(sel_building.kind);
            let live = saladin_sim::operational(sel_building.state);
            if !bdef.trains.is_empty() {
                production_group(c, font, assets, sel_building, &stock, owned, live);
            }
            orders_group(c, font, assets, bdef, sel_building, &stock);
            // trade group on the market: sell goods for gold, buy them back
            // at the merchant's spread
            if bdef.enables_trade && live {
                group(c, font, "Sell", |c, font| {
                    for (res, key, name) in [
                        (ResourceType::Wood, "res:wood", "Sell Wood"),
                        (ResourceType::Stone, "res:stone", "Sell Stone"),
                        (ResourceType::Food, "res:food", "Sell Food"),
                    ] {
                        let have = stock.get(res);
                        tool_button(
                            c,
                            font,
                            assets,
                            UiAction::Sell(res),
                            name,
                            Some(format!("{MARKET_LOT} for {} Gold", MARKET_LOT / saladin_sim::MARKET_RATE)),
                            BtnStyle { disabled: have < MARKET_LOT, icon: assets.icon(key), ..default() },
                        );
                    }
                });
                group(c, font, "Buy", |c, font| {
                    for (res, key, name) in [
                        (ResourceType::Wood, "res:wood", "Buy Wood"),
                        (ResourceType::Stone, "res:stone", "Buy Stone"),
                        (ResourceType::Food, "res:food", "Buy Food"),
                    ] {
                        let cost = MARKET_LOT * saladin_sim::MARKET_BUY_RATE;
                        tool_button(
                            c,
                            font,
                            assets,
                            UiAction::Buy(res),
                            name,
                            Some(format!("{MARKET_LOT} for {cost} Gold")),
                            BtnStyle { disabled: stock.gold < cost, icon: assets.icon(key), ..default() },
                        );
                    }
                });
            }
            // research panel wherever research is hosted
            if bdef.hosts_research && live {
                let states = research_panel_state(p.tech_mask, rows, &stock, owned);
                group_wrapping(c, font, "Research", |c, font| {
                    for r in states {
                        let (sub, disabled) = match r.status {
                            ResearchStatus::Done => (Some("Done".to_string()), true),
                            ResearchStatus::InProgress => {
                                (Some(format!("{}%", (r.progress.to_num::<f32>() * 100.0) as i32)), true)
                            }
                            ResearchStatus::Locked => (r.lock_note.clone(), true),
                            ResearchStatus::Unaffordable => (Some(cost_line(&r.cost)), true),
                            ResearchStatus::Available => (Some(cost_line(&r.cost)), false),
                        };
                        tool_button(
                            c,
                            font,
                            assets,
                            UiAction::Research(r.tech as u8),
                            r.label,
                            sub,
                            BtnStyle { disabled, icon: assets.icon("tech:scroll"), ..default() },
                        );
                    }
                });
            }
            if can_host_garrison(bdef) {
                group(c, font, "Garrison", |c, font| {
                    tool_button(
                        c,
                        font,
                        assets,
                        UiAction::Ungarrison,
                        "Ungarrison",
                        Some(format!("{}/{}", sel_building.occupants, sel_building.garrison_cap)),
                        BtnStyle { disabled: sel_building.occupants == 0, ..default() },
                    );
                });
            }
        } else {
            // build menu: category tabs ABOVE the building cards (AoE-style)
            c.spawn((Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(5.0),
                flex_grow: 0.0,
                flex_shrink: 0.0,
                ..default()
            },))
                .with_children(|c| {
                    label(c, font, "BUILD", 11.0, GOLD);
                    c.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(2.0), ..default() },))
                        .with_children(|c| {
                            for (i, cat) in BUILD_CATEGORIES.iter().enumerate() {
                                tool_button(
                                    c,
                                    font,
                                    assets,
                                    UiAction::Tab(i),
                                    cat.label,
                                    None,
                                    BtnStyle { active: i == tab, ..BtnStyle::chip() },
                                );
                            }
                        });
                    c.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(2.0), ..default() },))
                        .with_children(|c| {
                            // the COST rides on every card, locked or not: a
                            // player has to be able to learn what a Siege
                            // Workshop costs before he can build one
                            for r in build_panel_state(tab, owned, counts, &stock) {
                                let active = mode == InputMode::Build(r.kind);
                                tool_button(
                                    c,
                                    font,
                                    assets,
                                    UiAction::Build(r.kind),
                                    hall_label(r.kind, p.faction),
                                    Some(price_line(&r.cost, r.build_time)),
                                    BtnStyle {
                                        active,
                                        disabled: !active && r.status != BuildStatus::Available,
                                        icon: assets.building_icon(r.kind),
                                        note: r.note,
                                        ..default()
                                    },
                                );
                            }
                        });
                });
        }

        // orders group — general commands, only on the no-selection view (a
        // selected building shows just its own commands)
        if sel_building.id.is_none() {
        group(c, font, "Orders", |c, font| {
            tool_button(
                c,
                font,
                assets,
                UiAction::GatherAll,
                "Gather",
                Some("idle peasants".into()),
                BtnStyle { tint: TINT_GREEN, icon: assets.icon("res:food"), ..default() },
            );
            tool_button(
                c,
                font,
                assets,
                UiAction::ToggleDemolish,
                "Demolish",
                Some("click buildings".into()),
                BtnStyle {
                    tint: TINT_RED,
                    active: mode == InputMode::Demolish,
                    icon: assets.icon("act:demolish"),
                    ..default()
                },
            );
        });
        }

        // completed techs badge row
        let done: Vec<_> = techs_in_mask(p.tech_mask);
        if !done.is_empty() {
            group(c, font, "Upgrades", |c, font| {
                for t in done {
                    label(c, font, upgrade_def(t).label, FONT_SM, GOLD);
                }
            });
        }
      });
    });
}

/// Train-at-this-hall cards plus the live queue. Orders are addressed to the
/// selected building by GameId, so "which barracks" is finally defined.
#[allow(clippy::too_many_arguments)]
fn production_group(
    c: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    sel: &SelectedBuilding,
    stock: &saladin_sim::Stockpile,
    owned: &HashSet<BuildingKind>,
    live: bool,
) {
    let full = sel.queue.len() >= saladin_sim::QUEUE_CAP;
    // faction exclusivity is a FILTER over one shared table, and the card is
    // where a player learns it: an Ayyubid Stable offers Mamluks, never Knights
    let roster = roster_for(sel.kind, sel.faction);
    group(c, font, "Train", |c, font| {
        for &kind in &roster {
            let u = unit_def(kind);
            let missing = u.requires.filter(|r| !owned.contains(r));
            let note = match (live, missing, full) {
                (false, _, _) => Some("Not finished".to_string()),
                (_, Some(r), _) => Some(format!("Needs {}", building_def(r).label)),
                (_, None, true) => Some("Queue full".to_string()),
                _ => None,
            };
            tool_button(
                c,
                font,
                assets,
                UiAction::TrainAt(kind),
                u.label,
                Some(price_line(&u.cost, u.train_time)),
                BtnStyle {
                    disabled: note.is_some() || !stock.can_afford(&u.cost),
                    icon: assets.unit_icon(kind),
                    note,
                    ..default()
                },
            );
        }
    });
    if sel.queue.is_empty() {
        return;
    }
    // only the LAST order can be dropped — that is what the sim does, so that
    // is the only chip that is a button
    let last = sel.queue.len() - 1;
    group(c, font, "Queue", |c, font| {
        for (i, kind) in sel.queue.iter().enumerate() {
            let sub = if i == 0 { format!("{}%", (sel.train_progress * 100.0) as i32) } else { String::new() };
            tool_button(
                c,
                font,
                assets,
                UiAction::CancelTrain,
                &sub,
                None,
                BtnStyle {
                    tint: if i == last { TINT_RED } else { TINT_NORMAL },
                    disabled: i != last,
                    icon: assets.unit_icon(*kind),
                    ..BtnStyle::slot()
                },
            );
        }
    });
}

/// Send Builders / Upgrade / Cancel / Demolish, each carrying its live price.
fn orders_group(
    c: &mut ChildSpawnerCommands,
    font: &UiFont,
    assets: &UiAssets,
    bdef: &BuildingDef,
    sel: &SelectedBuilding,
    stock: &saladin_sim::Stockpile,
) {
    let anything = sel.wants_work() || sel.upgrade.is_some() || bdef.buildable;
    if !anything {
        return;
    }
    group(c, font, "Orders", |c, font| {
        if sel.wants_work() {
            // a farm asks for FARMHANDS, and it asks for them while it is whole
            let farm = sel.crop.is_some() && !sel.damaged();
            let sub = if sel.builders > 0 && farm {
                sel.crop.map(|c| farm_hands(c.hands)).unwrap_or_default()
            } else if sel.builders > 0 {
                crew_line(sel.builders)
            } else if farm {
                "work the field".into()
            } else if sel.state == BuildState::Complete {
                "repair it".into()
            } else {
                "finish it".into()
            };
            tool_button(
                c,
                font,
                assets,
                UiAction::SendBuilders,
                if farm { "Send Farmhands" } else { "Send Builders" },
                Some(sub),
                BtnStyle {
                    tint: TINT_GREEN,
                    disabled: sel.builders >= saladin_sim::MAX_BUILDERS,
                    icon: assets.icon(if farm { "res:food" } else { "act:builders" }),
                    ..default()
                },
            );
        }
        if let Some((into, cost)) = sel.upgrade {
            let time = building_def(sel.kind).upgrade_time;
            tool_button(
                c,
                font,
                assets,
                UiAction::UpgradeSelected,
                building_def(into).label,
                Some(price_line(&cost, time)),
                BtnStyle {
                    disabled: !stock.can_afford(&cost),
                    icon: assets.icon("act:upgrade"),
                    note: (!stock.can_afford(&cost)).then(|| "Cannot afford".to_string()),
                    ..default()
                },
            );
        }
        if sel.state == BuildState::Site {
            let back = cancel_refund(&bdef.cost, Fx::from_num(sel.work));
            tool_button(
                c,
                font,
                assets,
                UiAction::CancelSite,
                "Cancel",
                Some(format!("back {}", cost_line(&back))),
                BtnStyle { tint: TINT_RED, icon: assets.icon("act:cancel"), ..default() },
            );
        } else if bdef.buildable {
            let back = demolish_refund(&bdef.cost, sel.hp, bdef.max_hp);
            tool_button(
                c,
                font,
                assets,
                UiAction::DemolishSelected,
                "Demolish",
                Some(format!("back {}", cost_line(&back))),
                BtnStyle { tint: TINT_RED, icon: assets.icon("act:demolish"), ..default() },
            );
        }
    });
}

fn group(
    c: &mut ChildSpawnerCommands,
    font: &UiFont,
    title: &str,
    body: impl FnOnce(&mut ChildSpawnerCommands, &UiFont),
) {
    group_inner(c, font, title, false, body);
}

/// A card group whose row wraps rather than running off the bar, which clips.
/// Opt-in: a wrapping row is line-broken against available width, not against
/// its own content, so a three-card row breaks after two even with the bar
/// half empty. Only the research shelf is long enough to need it.
fn group_wrapping(
    c: &mut ChildSpawnerCommands,
    font: &UiFont,
    title: &str,
    body: impl FnOnce(&mut ChildSpawnerCommands, &UiFont),
) {
    group_inner(c, font, title, true, body);
}

fn group_inner(
    c: &mut ChildSpawnerCommands,
    font: &UiFont,
    title: &str,
    wrap: bool,
    body: impl FnOnce(&mut ChildSpawnerCommands, &UiFont),
) {
    c.spawn((Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(6.0),
        flex_grow: 0.0,
        flex_shrink: if wrap { 1.0 } else { 0.0 },
        ..default()
    },))
        .with_children(|c| {
            label(c, font, &title.to_uppercase(), 11.0, GOLD);
            c.spawn((Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                flex_wrap: if wrap { FlexWrap::Wrap } else { FlexWrap::NoWrap },
                column_gap: Val::Px(4.0),
                row_gap: Val::Px(4.0),
                ..default()
            },))
                .with_children(|c| body(c, font));
        });
}

/// Build/demolish mode hint chip (top-center): rotation + cancel shortcuts.
#[derive(Component)]
pub struct ModeHint;

/// What the mode chip says. Ten distinct refusals used to render as one silent
/// red box; the ghost publishes which one it is and the chip says it out loud.
fn mode_hint_text(mode: InputMode, refused: Option<saladin_sim::PlaceError>) -> String {
    match (mode, refused) {
        (InputMode::Build(_), Some(e)) => format!("{}  -  Esc cancels", place_error_text(e)),
        (InputMode::Build(BuildingKind::Wall), None) => {
            "Drag to draw a wall (any direction)  -  Esc cancels".into()
        }
        (InputMode::Build(k), None) if building_def(k).min_fertility > saladin_sim::ZERO => {
            "Green ground is fertile - fields only take on good soil  -  Esc cancels".into()
        }
        (InputMode::Build(_), None) => "R rotates the building  -  Esc cancels".into(),
        (InputMode::Demolish, _) => "Click your buildings to demolish  -  Esc cancels".into(),
        (InputMode::AttackMove, _) => {
            "Click where to advance - they fight what they meet  -  Esc cancels".into()
        }
        (InputMode::Normal, _) => String::new(),
    }
}

pub fn build_mode_hint(
    mut commands: Commands,
    font: Res<UiFont>,
    assets: Res<UiAssets>,
    mode: Res<InputMode>,
    hint: Res<PlaceHint>,
    q: Query<Entity, With<ModeHint>>,
    mut shown: Local<String>,
) {
    let refused = matches!(*mode, InputMode::Build(_)).then(|| hint.0).flatten();
    let text = mode_hint_text(*mode, refused);
    if *shown == text {
        return;
    }
    shown.clone_from(&text);
    for e in &q {
        commands.entity(e).despawn();
    }
    if text.is_empty() {
        return;
    }
    let color = if refused.is_some() { WARN } else { GOLD };
    commands
        .spawn((
            ModeHint,
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(34.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(30),
        ))
        .with_children(|p| {
            p.spawn((
                Node { padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)), ..default() },
                panel_bg_dark(&assets),
            ))
            .with_children(|p| label(p, &font, &text, FONT_SM, color));
        });
}

/// Starvation toast trigger: food low while owning soldiers.
#[derive(Resource, Default)]
pub struct Toasts(pub Vec<(String, f32)>);

pub fn tick_toasts(time: Res<Time>, mut toasts: ResMut<Toasts>) {
    for t in toasts.0.iter_mut() {
        t.1 -= time.delta_secs();
    }
    toasts.0.retain(|t| t.1 > 0.0);
}

#[derive(Component)]
pub struct ToastUi;

pub fn render_toasts(
    mut commands: Commands,
    font: Res<UiFont>,
    assets: Res<UiAssets>,
    toasts: Res<Toasts>,
    q: Query<Entity, With<ToastUi>>,
) {
    if !toasts.is_changed() {
        return;
    }
    for e in &q {
        commands.entity(e).despawn();
    }
    if toasts.0.is_empty() {
        return;
    }
    commands
        .spawn((
            ToastUi,
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(30.0),
                left: Val::Percent(38.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|p| {
            for (text, _) in &toasts.0 {
                p.spawn((
                    Node { padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)), ..default() },
                    panel_bg_dark(&assets),
                ))
                .with_children(|p| label(p, &font, text, FONT_SM, TEXT));
            }
        });
}

/// Say out loud why the sim threw a Build away. The ghost mirrors `check_build`
/// locally, so most refusals are visible before the click - but not the ones
/// only the sim can see: a tile someone else took on the same tick, coin spent
/// by an earlier command in the same batch. `CommandFeedback` has recorded them
/// since the command path was rewritten and nothing has ever read it, so those
/// orders vanished in silence.
pub fn watch_refusals(
    local: Res<LocalPlayer>,
    tick: Res<saladin_protocol::Tick>,
    feedback: Res<saladin_protocol::CommandFeedback>,
    mut toasts: ResMut<Toasts>,
    mut last: Local<u64>,
) {
    // the resource lives for one sim tick and the frame rate is higher, so the
    // tick number is what stops one refusal becoming five toasts
    if *last == tick.0 || feedback.0.is_empty() {
        return;
    }
    *last = tick.0;
    let mut said: HashSet<String> = HashSet::new();
    for (owner, err) in &feedback.0 {
        if *owner != local.0 {
            continue;
        }
        let text = place_error_text(*err);
        if said.insert(text.clone()) {
            toasts.0.push((text, 2.2));
        }
    }
}

/// Fire gameplay toasts off sim state edges (starving start, research done).
pub fn watch_toasts(
    local: Res<LocalPlayer>,
    q_players: Query<&Player>,
    q_units: Query<(&Owner, &Pos, &Unit)>,
    q_buildings: Query<(&Owner, &Pos, &Building)>,
    mut toasts: ResMut<Toasts>,
    mut prev_short: Local<bool>,
    mut prev_mask: Local<u64>,
) {
    let Some(p) = q_players.iter().find(|p| p.player_id == local.0) else { return };
    let stores = supply_stores(q_buildings.iter(), local.0);
    let supply = supply_readout(
        q_units.iter().filter(|(o, _, _)| o.0 == local.0).map(|(_, pos, u)| (u.kind, u.ration, pos.pos)),
        &stores,
    );
    let short = supply.short > 0;
    if short && !*prev_short {
        toasts.0.push((
            format!("{} of {} men on {}% rations.", supply.short, supply.eaters, supply.worst),
            2.6,
        ));
    }
    *prev_short = short;
    if p.tech_mask != *prev_mask {
        for t in techs_in_mask(p.tech_mask & !*prev_mask) {
            toasts.0.push((format!("Research complete: {}", upgrade_def(t).label), 2.6));
        }
        *prev_mask = p.tech_mask;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saladin_sim::{Faction, Stockpile, UnitKind};

    fn sel_of(kind: BuildingKind, faction: Faction) -> SelectedBuilding {
        let d = building_def(kind);
        SelectedBuilding {
            id: Some(1),
            kind,
            faction,
            target_kind: kind,
            garrison_cap: d.garrison_cap,
            max_hp: d.max_hp.max(1),
            hp: d.max_hp.max(1),
            ..default()
        }
    }

    fn sel(kind: BuildingKind) -> SelectedBuilding {
        sel_of(kind, Faction::Ayyubid)
    }

    /// The card is generated from def FIELDS, so a kind with a capability the
    /// HUD cannot describe is a role the player is never told about.
    #[test]
    fn every_structure_says_what_it_is_for() {
      for faction in [Faction::Ayyubid, Faction::Crusader] {
        for &kind in BuildingKind::ALL {
            let d = building_def(kind);
            let lines = role_lines(d, &sel_of(kind, faction));
            let has_role = d.accepts != 0
                || !d.trains.is_empty()
                || d.aura.is_some()
                || d.min_fertility > Fx::ZERO
                || d.hosts_research
                || d.enables_trade
                || d.morale_radius > Fx::ZERO
                || d.attack > 0
                || d.pop > 0
                || d.garrison_cap > 0
                || d.upgrades_to.is_some()
                || d.defeat_on_death;
            assert_eq!(!lines.is_empty(), has_role, "{kind:?} role lines vs def fields");
        }
        // the Farm's mechanic IS its soil: `min_fertility` is what the sim gates
        // siting, growth and the tending crew on, so it is what the card reads
        assert!(
            role_lines(building_def(BuildingKind::Farm), &sel(BuildingKind::Farm))
                .iter()
                .any(|l| l.contains("field")),
            "the Farm's card says nothing about the crop it grows"
        );
        for &kind in BuildingKind::ALL {
            let b = building_def(kind).blurb;
            assert!(!b.is_empty(), "{kind:?} has no blurb, so its card says nothing it does");
            assert!(b.is_ascii(), "{kind:?} blurb is not ASCII: {b:?}");
        }
      }
    }

    /// The embedded font has no non-ASCII glyphs and the atlas pre-warm is
    /// ASCII-only, so one em dash shreds the card.
    #[test]
    fn every_generated_card_string_is_ascii_and_fits() {
        // widest column the card can render before a line wraps
        const MAX_CHARS: usize = 37;
        for faction in [Faction::Ayyubid, Faction::Crusader] {
            for &kind in BuildingKind::ALL {
                let d = building_def(kind);
                let mut strings = role_lines(d, &sel_of(kind, faction));
                strings.push(d.blurb.to_string());
                strings.push(price_line(&d.cost, d.build_time));
                strings.push(price_line(&d.upgrade_cost, d.upgrade_time));
                strings.push(crew_line(0));
                strings.push(crew_line(3));
                strings.push(hall_label(kind, faction).to_string());
                strings.push(format!(
                    "Becoming {}",
                    hall_label(d.upgrades_to.unwrap_or(kind), faction)
                ));
                strings.push(format!("Tended by {}", d.label));
                for s in strings {
                    assert!(s.is_ascii(), "{kind:?}: {s:?} is not ASCII");
                    assert!(s.len() <= MAX_CHARS * 2, "{kind:?}: {s:?} is unreadably long");
                }
            }
        }
        for &kind in UnitKind::ALL {
            let u = unit_def(kind);
            assert!(u.label.is_ascii() && price_line(&u.cost, u.train_time).is_ascii());
        }
        // the march chips and the supply line are generated too
        for (_, name) in FORMATION_NAMES {
            assert!(name.is_ascii() && name.len() <= 6, "{name:?}");
        }
        for (short, worst, afield) in [(0, 100, 0), (0, 100, 14), (3, 0, 3), (12, 62, 12)] {
            let r = SupplyReadout { eaters: 20, afield, short, worst, bill: 126 };
            let (line, warn) = r.line(140);
            assert!(line.is_ascii() && line.len() <= MAX_CHARS, "{line:?}");
            assert_eq!(warn, short > 0);
        }
        for h in crate::input::HOTKEY_HELP {
            assert!(h.0.is_ascii() && h.1.is_ascii(), "{h:?}");
        }
        // the ferry hold, over every capacity the roster can actually offer
        for k in UnitKind::ALL {
            let cap = unit_def(*k).cargo_cap;
            if cap <= 0 {
                continue;
            }
            for aboard in 0..=cap as u32 {
                let (hold, how) = cargo_lines(aboard, cap as u32);
                assert!(hold.is_ascii() && how.is_ascii(), "{hold:?} {how:?}");
                assert!(hold.len() <= MAX_CHARS && how.len() <= MAX_CHARS, "{hold:?} {how:?}");
            }
        }
        // the farm card is generated from live sim numbers, so it is generated
        // over the whole range the sim can hand it
        for cap in [saladin_sim::FARM_CAP_MIN, 102, saladin_sim::FARM_CAP_MAX] {
            for hands in [0, 1, saladin_sim::MAX_BUILDERS] {
                let crop = CropInfo { remaining: cap, cap, hands, ..default() };
                for s in [
                    farm_line(&crop),
                    hands_line(&crop).0,
                    format!("Crop  {}/{}", crop.remaining, crop.cap),
                    crop_stage_line(&crop).0.to_string(),
                    farm_hands(hands),
                ] {
                    assert!(s.is_ascii() && s.len() <= MAX_CHARS, "{s:?}");
                }
            }
        }
    }

    /// The card has to say which of the five states the field is in — that is
    /// the whole point of it, and until now a farm's card said nothing at all.
    #[test]
    fn the_farm_card_names_the_season_the_soil_and_the_crew() {
        let growing = CropInfo { remaining: 40, cap: 102, hands: 2, ..default() };
        assert_eq!(crop_stage_line(&growing).0, "Growing");
        assert_eq!(hands_line(&growing), ("2 farmhands".into(), TEXT_DIM));

        let ripe = CropInfo { remaining: 102, cap: 102, ripe: true, ..default() };
        assert_eq!(crop_stage_line(&ripe).0, "Harvest ready");
        assert_eq!(crop_stage_line(&ripe).1, GOLD);
        // an unworked field is the only half of this pair that ever shouts:
        // the soil is a fact the player chose, the empty crew is a cost he is
        // paying right now. They share no colour because they share no line.
        assert_eq!(hands_line(&ripe), ("no farmhands".into(), WARN));
        assert!(farm_line(&ripe).starts_with("Soil "));
        for cap in [saladin_sim::FARM_CAP_MIN, 102, saladin_sim::FARM_CAP_MAX] {
            let rich = CropInfo { cap, hands: 3, ..default() };
            assert!(farm_line(&rich).contains(rich.soil_word()));
            assert_eq!(hands_line(&rich).1, TEXT_DIM, "worked ground is not a warning");
        }

        // lodging is the only farm state that shouts, because it is the only
        // one that is costing the player something right now
        let lodged = CropInfo { lodging: true, ripe: true, ..ripe };
        assert_eq!(crop_stage_line(&lodged), ("Crop is lodging", WARN));

        // the bar is the crop, not the health: a stripped field reads empty
        assert_eq!(CropInfo { remaining: 0, cap: 102, ..default() }.fill(), 0.0);
        assert_eq!(ripe.fill(), 1.0);
        assert_eq!(CropInfo { remaining: 51, cap: 102, ..default() }.fill(), 0.5);
        // a field with no cap at all must not divide by zero
        assert_eq!(CropInfo::default().fill(), 0.0);
    }

    /// `cap` IS the soil the sim computed at siting time. The word has to move
    /// across the range the worldgen actually produces, or the fertility overlay
    /// still pays off for two seconds and never again.
    #[test]
    fn the_soil_word_reads_the_yield_the_ground_bought() {
        use saladin_sim::{FARM_CAP_MAX, FARM_CAP_MIN, FARM_MIN_FERTILITY, FARM_SOIL_RICH, field_cap};
        let word = |soil| CropInfo { cap: field_cap(soil), ..default() }.soil_word();
        assert_eq!(word(FARM_MIN_FERTILITY), "Thin");
        assert_eq!(word(FARM_SOIL_RICH), "Rich");
        assert_eq!(CropInfo { cap: FARM_CAP_MIN, ..default() }.soil_word(), "Thin");
        assert_eq!(CropInfo { cap: FARM_CAP_MAX, ..default() }.soil_word(), "Rich");
        // every word the card can say is reachable from real ground
        let mut seen: HashSet<&str> = HashSet::new();
        for n in 22..=60 {
            seen.insert(word(Fx::from_num(n) / Fx::from_num(100)));
        }
        assert_eq!(seen.len(), 3, "the soil word collapses to {seen:?}");

        // ...and it has to DISCRIMINATE over the ground a player actually sites
        // on, not merely over the range `field_cap` could theoretically return.
        // Equal thirds of that range called four plots in five "Thin".
        use saladin_sim::{BuildingKind, building_def, check_place, compose_seed, soil_quality, start_point};
        let fp = building_def(BuildingKind::Farm).footprint;
        let half = saladin_sim::fx!("0.5");
        let mut tally: std::collections::BTreeMap<&str, u32> = Default::default();
        let mut plots = 0u32;
        for (base, preset) in [(11u32, 1u8), (48514, 0), (1234, 2)] {
            let seed = compose_seed(base, preset);
            let start = start_point(seed, 0);
            let (sx, sy) = (start.x.to_num::<i32>(), start.y.to_num::<i32>());
            let reach = saladin_sim::TOWN_RADIUS.to_num::<i32>();
            for ty in sy - reach..=sy + reach {
                for tx in sx - reach..=sx + reach {
                    let (x, y) = (Fx::from_num(tx) + half, Fx::from_num(ty) + half);
                    if check_place(seed, BuildingKind::Farm, x, y, |_, _| false, |_, _| true, &[]).is_err() {
                        continue;
                    }
                    plots += 1;
                    let cap = saladin_sim::field_cap(soil_quality(seed, fp, x, y));
                    *tally.entry(CropInfo { cap, ..default() }.soil_word()).or_default() += 1;
                }
            }
        }
        assert!(plots > 500, "only {plots} sitable plots sampled");
        assert_eq!(tally.len(), 3, "real ground only ever says {tally:?}");
        // measured on these three worlds, equal thirds of the cap SPAN scored
        // Thin 72.5% / Fair 22.8% / Rich 4.8%; the bands the card ships score
        // 19.3 / 31.9 / 48.8. 10..60% is the band that separates the two.
        for (w, n) in &tally {
            assert!(
                *n * 10 > plots && *n * 5 < plots * 3,
                "{w:?} claims {n} of {plots} plots: the word carries no information ({tally:?})"
            );
        }
    }

    /// The card only offers what the player's faction can actually field. This
    /// is the whole of faction identity as far as the HUD is concerned, and it
    /// was invisible before: both sides were shown all ten kinds.
    #[test]
    fn a_hall_offers_its_own_factions_roster_and_wears_its_own_name() {
        let stable = |f| role_lines(building_def(BuildingKind::Stable), &sel_of(BuildingKind::Stable, f));
        let ayy = stable(Faction::Ayyubid).join(" ");
        let cru = stable(Faction::Crusader).join(" ");
        assert!(ayy.contains("Mamluk") && !ayy.contains("Knight"), "{ayy}");
        assert!(cru.contains("Knight") && !cru.contains("Mamluk"), "{cru}");
        // one hall, two liturgies — the kind is index-stable, only the name moves
        assert_eq!(hall_label(BuildingKind::Mosque, Faction::Ayyubid), "Mosque");
        assert_eq!(hall_label(BuildingKind::Mosque, Faction::Crusader), "Chapel");
    }

    /// Hunger used to be a single word: STARVING or nothing. Rationing is
    /// proportional AND a garrison is free, so the readout has to report a
    /// share, a count, and — the new half — WHAT THE ROAD COSTS before it turns
    /// into a shortage. A player who cannot see the bill cannot answer it.
    #[test]
    fn the_supply_line_reports_the_road_and_the_share() {
        use saladin_sim::UnitKind::*;
        let keep = V2::new(Fx::from_num(60), Fx::from_num(60));
        let at_home = V2::new(Fx::from_num(64), Fx::from_num(60));
        let out = V2::new(Fx::from_num(300), Fx::from_num(60));

        let full = supply_readout(
            [(Spearman, FULL_RATION, at_home), (Peasant, Fx::ZERO, at_home)].into_iter(),
            &[keep],
        );
        // a peasant on nothing is not on short rations — it never drew any
        assert_eq!((full.eaters, full.short, full.afield, full.bill), (1, 0, 0, 0));
        assert_eq!(full.line(50).0, "50", "a garrison must read as free");
        assert!(!full.line(50).1);

        // fed, but out past the stores: no warning, and the cost is on the bar
        let marching = supply_readout([(Spearman, FULL_RATION, out)].into_iter(), &[keep]);
        assert_eq!((marching.afield, marching.short), (1, 0));
        assert!(marching.bill > 0, "a march that costs nothing to look at");
        let (line, warn) = marching.line(200);
        assert!(!warn && line.contains("1 AFIELD") && line.contains("/min"), "{line}");

        let half = supply_readout(
            [
                (Spearman, saladin_sim::fx!("0.5"), out),
                (Knight, FULL_RATION, out),
                (Chaplain, Fx::ZERO, out),
            ]
            .into_iter(),
            &[keep],
        );
        assert_eq!((half.eaters, half.short, half.worst, half.afield), (2, 1, 50, 2));
        let (line, warn) = half.line(9);
        assert!(warn && line.contains("50%") && line.contains('9'), "{line}");
    }

    /// A locked card keeps its price and gains a reason — never swaps one for
    /// the other, which is how a player learns what to save up for.
    #[test]
    fn a_locked_build_card_still_shows_its_price() {
        let broke = Stockpile::default();
        let rows = build_panel_state(3, &HashSet::new(), &[0i32; BuildingKind::ALL.len()], &broke);
        assert!(!rows.is_empty());
        for r in rows {
            let price = price_line(&r.cost, r.build_time);
            assert!(!price.is_empty() && price != "Free", "{:?} has no price line", r.kind);
            if let BuildStatus::Locked { .. } = r.status {
                assert!(r.note.is_some(), "{:?} is locked with no reason given", r.kind);
            }
        }
    }

    /// Every refusal reaches the player as its own ASCII sentence.
    #[test]
    fn every_placement_refusal_is_spoken_aloud() {
        use saladin_sim::PlaceError::*;
        let all = [
            Terrain, Occupied, NeedsWaterside, OutsideTown, NoApproach, PoorSoil, TooSteep,
            NotBuildable, MissingPrereq(BuildingKind::Barracks), TooMany, CannotAfford,
        ];
        let mut seen: HashSet<String> = HashSet::new();
        for e in all {
            let t = mode_hint_text(InputMode::Build(BuildingKind::House), Some(e));
            assert!(t.is_ascii(), "{t:?}");
            assert!(seen.insert(t.clone()), "{e:?} shares its wording with another refusal: {t}");
        }
        // and a valid spot says what the controls are, not what is wrong
        let ok = mode_hint_text(InputMode::Build(BuildingKind::House), None);
        assert!(ok.contains("rotates"));
        assert!(mode_hint_text(InputMode::Normal, None).is_empty());
        let adv = mode_hint_text(InputMode::AttackMove, None);
        assert!(adv.is_ascii() && adv.contains("advance"), "{adv}");
    }

    #[test]
    fn a_price_states_cost_and_time() {
        let c = ResourceCost::new(70, 20, 0, 0);
        assert_eq!(price_line(&c, Fx::from_num(16)), "70W 20S  16s");
        assert_eq!(price_line(&c, Fx::ZERO), "70W 20S");
        assert_eq!(price_line(&ResourceCost::ZERO, Fx::from_num(3)), "3s");
        assert_eq!(price_line(&ResourceCost::ZERO, Fx::ZERO), "Free");
    }

    /// The click-through band is the panels' MEASURED rects. The old hardcoded
    /// `y > height - 120` left 63 logical px of live build bar passing clicks
    /// through to the map, and blocked bare map either side of it.
    #[test]
    fn the_hud_band_is_exactly_the_panels() {
        let mut rects = HudRects::default();
        rects.0.push(Rect::new(0.0, 0.0, 1280.0, 26.0));
        rects.0.push(Rect::new(260.0, 537.0, 1108.0, 715.0));
        assert!(rects.hit(Vec2::new(600.0, 545.0)), "the top of the build bar eats clicks");
        assert!(rects.hit(Vec2::new(600.0, 10.0)), "the resource bar eats clicks");
        assert!(!rects.hit(Vec2::new(1200.0, 650.0)), "bare map beside the bar does not");
        assert!(!rects.hit(Vec2::new(120.0, 600.0)), "bare map beside the bar does not");
        assert!(!rects.hit(Vec2::new(600.0, 500.0)), "map above the bar does not");
    }
}
