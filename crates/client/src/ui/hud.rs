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
use crate::input::InputMode;
use crate::selection::{SelectedBuilding, SelectionInfo};
use crate::{LocalPlayer, UiFont};
use bevy::prelude::*;
use saladin_protocol::{Building, Owner, Player, Research, Unit};
use saladin_sim::{
    BUILD_CATEGORIES, BuildingKind, ResearchProgressRow, ResearchStatus, ResourceType, Stance,
    building_def, can_host_garrison, food_low, has_prereq, research_panel_state,
    techs_in_mask, unit_def, upgrade_def,
};
use std::collections::HashSet;

#[derive(Component)]
pub struct HudRoot;

#[derive(Component)]
pub struct ResourceText(pub usize); // 0..=7: name,wood,stone,food,gold,peasants,army,pop

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
        BottomLeft,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(5.0),
            left: Val::Px(5.0),
            width: Val::Px(210.0),
            min_height: Val::Px(150.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(Val::Px(26.0), Val::Px(24.0)),
            row_gap: Val::Px(4.0),
            ..default()
        },
        panel_bg(&assets),
    ));
    commands.spawn((
        HudRoot,
        BottomCenter,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(5.0),
            left: Val::Px(220.0),
            right: Val::Px(172.0),
            min_height: Val::Px(178.0),
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::FlexStart,
            column_gap: Val::Px(16.0),
            overflow: Overflow::clip(),
            padding: UiRect::axes(Val::Px(26.0), Val::Px(24.0)),
            ..default()
        },
        panel_bg(&assets),
    ));

    // minimap frame (the minimap itself is a camera viewport bottom-right)
    commands.spawn((
        HudRoot,
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

/// Refresh the top bar texts in place.
pub fn update_resource_bar(
    local: Res<LocalPlayer>,
    q_players: Query<&Player>,
    q_units: Query<(&Owner, &Unit)>,
    q_buildings: Query<(&Owner, &Building)>,
    mut q_text: Query<(&ResourceText, &mut Text, &mut TextColor)>,
) {
    let Some(p) = my_player(&q_players, local.0) else { return };
    let (mut peasants, mut soldiers, mut pop) = (0, 0, 0);
    for (o, u) in &q_units {
        if o.0 != local.0 {
            continue;
        }
        pop += 1;
        if u.kind == saladin_sim::UnitKind::Peasant {
            peasants += 1;
        }
        if unit_def(u.kind).attack > 0 {
            soldiers += 1;
        }
    }
    let cap: i32 = q_buildings.iter().filter(|(o, _)| o.0 == local.0).map(|(_, b)| building_def(b.kind).pop).sum();
    let starving = food_low(p.stock.food, pop);

    for (slot, mut text, mut color) in &mut q_text {
        let (s, c) = match slot.0 {
            0 => (format!("{}  ({:?})", p.name, p.faction), ACCENT),
            1 => (format!("{}", p.stock.wood), TEXT),
            2 => (format!("{}", p.stock.stone), TEXT),
            3 => (
                format!("{}{}", p.stock.food, if starving { "  STARVING" } else { "" }),
                if starving { WARN } else { TEXT },
            ),
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
    mut digest: ResMut<HudDigest>,
    q_players: Query<&Player>,
    q_buildings: Query<(&Owner, &Building)>,
    q_research: Query<&Research>,
    q_left: Query<Entity, With<BottomLeft>>,
    q_center: Query<Entity, With<BottomCenter>>,
) {
    let Some(p) = my_player(&q_players, local.0) else { return };
    let owned: HashSet<BuildingKind> =
        q_buildings.iter().filter(|(o, _)| o.0 == local.0).map(|(_, b)| b.kind).collect();
    let rows: Vec<ResearchProgressRow> = q_research
        .iter()
        .filter(|r| r.owner == local.0)
        .map(|r| ResearchProgressRow { tech: r.tech, progress: r.progress, done: r.done })
        .collect();

    let key = format!(
        "{:?}|{:?}|{}|{}|{:?}|{:?}|{:?}|{:?}|{}|{:.2}|{:.2}|{}",
        p.stock,
        info.by_kind,
        info.total,
        info.routing,
        sel_building.id,
        sel_building.occupants,
        tab.0,
        *mode,
        owned.len(),
        info.avg_hp,
        info.avg_morale,
        rows.iter().map(|r| format!("{}:{:.2}:{}", r.tech, r.progress.to_num::<f32>(), r.done)).collect::<Vec<_>>().join(","),
    );
    if digest.0 == key {
        return;
    }
    digest.0 = key;

    let Ok(left) = q_left.single() else { return };
    let Ok(center) = q_center.single() else { return };
    commands.entity(left).despawn_related::<Children>();
    commands.entity(center).despawn_related::<Children>();

    build_command_card(&mut commands, left, &font, &assets, &info, &sel_building, p);
    build_build_bar(
        &mut commands,
        center,
        &font,
        &assets,
        p,
        &owned,
        &rows,
        &sel_building,
        tab.0,
        *mode,
    );
}

fn build_command_card(
    commands: &mut Commands,
    left: Entity,
    font: &UiFont,
    assets: &UiAssets,
    info: &SelectionInfo,
    sel_building: &SelectedBuilding,
    p: &Player,
) {
    commands.entity(left).with_children(|c| {
        if info.total > 0 {
            label(c, font, "Selection", FONT_SM, TEXT_DIM);
            label(c, font, &format!("{} unit{}", info.total, if info.total > 1 { "s" } else { "" }), FONT_MD, TEXT);
            for (kind_idx, &count) in info.by_kind.iter().enumerate() {
                if count == 0 {
                    continue;
                }
                let kind = saladin_sim::UnitKind::from_u8(kind_idx as u8).unwrap();
                let base = unit_def(kind);
                let eff = saladin_sim::effective_unit_def(kind, p.tech_mask);
                let up = if eff.attack != base.attack || eff.max_hp != base.max_hp { " ^" } else { "" };
                label(c, font, &format!("{}{}  x{}", base.label, up, count), FONT_SM, TEXT);
            }
            if info.has_combat {
                c.spawn((Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(2.0), ..default() },))
                    .with_children(|c| {
                        for (stance, name) in [
                            (Stance::Aggressive, "Attack"),
                            (Stance::Defensive, "Defend"),
                            (Stance::HoldGround, "Hold"),
                        ] {
                            tool_button(
                                c,
                                font,
                                assets,
                                UiAction::Stance(stance),
                                name,
                                None,
                                BtnStyle { min_width: 40.0, icon: assets.stance_icon(stance), ..default() },
                            );
                        }
                    });
            }
            label(c, font, "Health", 12.0, TEXT_DIM);
            ratio_bar(c, assets, 126.0, info.avg_hp, hp_color(info.avg_hp));
            if info.has_combat {
                let routing = if info.routing > 0 { format!("Morale   {} routing!", info.routing) } else { "Morale".into() };
                label(c, font, &routing, 12.0, if info.routing > 0 { WARN } else { TEXT_DIM });
                ratio_bar(c, assets, 126.0, info.avg_morale, morale_color(info.avg_morale));
            }
        } else if let Some(_id) = sel_building.id {
            let def = building_def(sel_building.kind);
            label(c, font, def.label, FONT_MD, ACCENT);
            label(c, font, def.blurb, 11.0, TEXT_DIM);
            if def.garrison_cap > 0 {
                label(
                    c,
                    font,
                    &format!("Garrison {}/{}", sel_building.occupants, sel_building.garrison_cap),
                    FONT_SM,
                    TEXT,
                );
            }
        } else {
            label(c, font, "No selection", FONT_SM, TEXT_DIM);
            label(c, font, "Drag to select units.", 12.0, TEXT_DIM);
            label(c, font, "Right-click to order.", 12.0, TEXT_DIM);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn build_build_bar(
    commands: &mut Commands,
    center: Entity,
    font: &UiFont,
    assets: &UiAssets,
    p: &Player,
    owned: &HashSet<BuildingKind>,
    rows: &[ResearchProgressRow],
    sel_building: &SelectedBuilding,
    tab: usize,
    mode: InputMode,
) {
    let stock = p.stock;
    commands.entity(center).with_children(|c| {
        if let Some(_id) = sel_building.id {
            // production group for the selected building
            let bdef = building_def(sel_building.kind);
            group(c, font, bdef.label, |c, font| {
                for &kind in bdef.trains {
                    let u = unit_def(kind);
                    let locked = !has_prereq(owned, u.requires);
                    let sub = if locked {
                        Some(format!("needs {}", building_def(u.requires.unwrap()).label))
                    } else {
                        Some(cost_line(&u.cost))
                    };
                    tool_button(
                        c,
                        font,
                        assets,
                        UiAction::Train(kind),
                        u.label,
                        sub,
                        BtnStyle {
                            disabled: locked || !stock.can_afford(&u.cost),
                            icon: assets.unit_icon(kind),
                            ..default()
                        },
                    );
                }
                if bdef.buildable {
                    tool_button(
                        c,
                        font,
                        assets,
                        UiAction::DemolishSelected,
                        "Demolish",
                        None,
                        BtnStyle { tint: TINT_RED, icon: assets.icon("act:demolish"), ..default() },
                    );
                }
            });
            // trade group on the market: sell goods for gold, buy them back
            // at the merchant's spread
            if sel_building.kind == BuildingKind::Market {
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
            // research panel on the blacksmith
            if sel_building.kind == BuildingKind::Blacksmith {
                let states = research_panel_state(p.tech_mask, rows, &stock, owned);
                group(c, font, "Research", |c, font| {
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
            if can_host_garrison(&bdef) {
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
                            for &kind in BUILD_CATEGORIES[tab.min(BUILD_CATEGORIES.len() - 1)].kinds {
                                let d = building_def(kind);
                                let locked = !has_prereq(owned, d.requires);
                                let sub = if locked {
                                    Some(format!("needs {}", building_def(d.requires.unwrap()).label))
                                } else {
                                    Some(cost_line(&d.cost))
                                };
                                let active = mode == InputMode::Build(kind);
                                tool_button(
                                    c,
                                    font,
                                    assets,
                                    UiAction::Build(kind),
                                    d.label,
                                    sub,
                                    BtnStyle {
                                        active,
                                        disabled: !active && (locked || !stock.can_afford(&d.cost)),
                                        icon: assets.building_icon(kind),
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
}

fn group(
    c: &mut ChildSpawnerCommands,
    font: &UiFont,
    title: &str,
    body: impl FnOnce(&mut ChildSpawnerCommands, &UiFont),
) {
    c.spawn((Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(6.0),
        flex_grow: 0.0,
        flex_shrink: 0.0,
        ..default()
    },))
        .with_children(|c| {
            label(c, font, &title.to_uppercase(), 11.0, GOLD);
            c.spawn((Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                column_gap: Val::Px(4.0),
                ..default()
            },))
                .with_children(|c| body(c, font));
        });
}

/// Build/demolish mode hint chip (top-center): rotation + cancel shortcuts.
#[derive(Component)]
pub struct ModeHint;

pub fn build_mode_hint(
    mut commands: Commands,
    font: Res<UiFont>,
    assets: Res<UiAssets>,
    mode: Res<InputMode>,
    q: Query<Entity, With<ModeHint>>,
    mut shown: Local<String>,
) {
    let text = match *mode {
        InputMode::Build(k) if k == saladin_sim::BuildingKind::Wall => {
            "Drag to draw a wall (any direction)  -  Esc cancels"
        }
        InputMode::Build(_) => "R rotates the building  -  Esc cancels",
        InputMode::Demolish => "Click your buildings to demolish  -  Esc cancels",
        InputMode::Normal => "",
    };
    if *shown == text {
        return;
    }
    *shown = text.to_string();
    for e in &q {
        commands.entity(e).despawn();
    }
    if text.is_empty() {
        return;
    }
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
            .with_children(|p| label(p, &font, text, FONT_SM, GOLD));
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

/// Fire gameplay toasts off sim state edges (starving start, research done).
pub fn watch_toasts(
    local: Res<LocalPlayer>,
    q_players: Query<&Player>,
    q_units: Query<&Owner, With<Unit>>,
    mut toasts: ResMut<Toasts>,
    mut prev_starving: Local<bool>,
    mut prev_mask: Local<u64>,
) {
    let Some(p) = q_players.iter().find(|p| p.player_id == local.0) else { return };
    let pop = q_units.iter().filter(|o| o.0 == local.0).count() as i32;
    let starving = food_low(p.stock.food, pop);
    if starving && !*prev_starving {
        toasts.0.push(("Your army is starving! Gather food.".into(), 2.6));
    }
    *prev_starving = starving;
    if p.tech_mask != *prev_mask {
        for t in techs_in_mask(p.tech_mask & !*prev_mask) {
            toasts.0.push((format!("Research complete: {}", upgrade_def(t).label), 2.6));
        }
        *prev_mask = p.tech_mask;
    }
}
