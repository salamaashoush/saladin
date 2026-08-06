use crate::economy::ResourceCost;
use crate::enums::{ArmorClass, BuildingKind, DamageType, UnitKind, UnitRole};
use crate::math::{Fx, ONE};
use crate::roster::{FACTION_AYYUBID, FACTION_BOTH, FACTION_CRUSADER};
use crate::terrain::Domain;

/// Stats + presentation for one trainable unit. The generic combat/gather/move
/// systems dispatch on `UnitKind`, so a new roster entry never touches systems.
/// Everything here is STATIC: none of it is serialized, so a new field costs
/// nothing on the wire or in a save.
#[derive(Clone, Copy, Debug)]
pub struct UnitDef {
    pub label: &'static str,
    pub icon: &'static str,
    pub role: UnitRole,
    /// Which factions may train this kind (`roster.rs` bitmask). Exclusivity is
    /// a filter over one shared table, never a deletion.
    pub factions: u8,
    /// Which passability grid this kind walks. Every closure-building call site
    /// asks this rather than assuming land.
    pub domain: Domain,
    pub speed: Fx,
    pub carry: i32,
    /// Units this kind can FERRY (0 = none). Distinct from `carry`, which is a
    /// load of resources: a barge hauls men, a skiff hauls fish.
    pub cargo_cap: i32,
    /// Housing this kind occupies. A hull is a crew and a vessel, not a man.
    pub pop_cost: i32,
    pub radius: Fx,
    pub height: Fx,
    pub max_hp: i32,
    pub attack: i32,
    pub damage_type: DamageType,
    pub armor_class: ArmorClass,
    /// Flat damage subtracted from every incoming hit (never below 1 dealt).
    /// This is what armour RESEARCH grants — promoting a unit's armour class
    /// would empty the Leather column of DAMAGE_MATRIX mid-match.
    pub damage_reduction: i32,
    /// Specialist multiplier vs each armor class (1.0 == none).
    pub bonus_vs_armor: [Fx; 4],
    pub range: Fx,
    /// Closer than this the engine cannot depress its arc (0 = none).
    pub min_range: Fx,
    pub attack_rate: Fx,
    /// The rate the engine can ACTUALLY hit, in whole combat ticks. Combat runs
    /// on a 200 ms cadence, so a declared 1.1 s swing is a 1.2 s swing; this is
    /// the honest number, and a test pins it to `attack_rate`.
    pub attack_ticks: i32,
    pub aggro_range: Fx,
    pub cost: ResourceCost,
    pub tint: Option<u32>,
    pub requires: Option<BuildingKind>,
    pub prefers_buildings: bool,
    /// Radius over which allies recover morale faster (passive sustain).
    pub morale_aura: Fx,
    /// Radius over which allies rally sooner and break later (discipline).
    pub rally_aura: Fx,
    /// Scales how hard a wound dents this unit's morale (1.0 = the baseline;
    /// higher holds longer).
    pub morale_resolve: Fx,
    /// Damage multiplier on the first blow of a charge (1.0 = no charge).
    pub charge_mult: Fx,
    /// Sets against cavalry: negates a charge frontally, and gates this unit's
    /// own `bonus_vs_armor` on standing still.
    pub brace: bool,
    /// Splash radius of one hit (0 = single target).
    pub splash: Fx,
    /// Fires over walls and units — the one exemption from line of sight.
    pub arcs: bool,
    /// Seconds to emplace before the first shot after moving (0 = none).
    pub setup_time: Fx,
    pub garrisonable: bool,
    pub ranged: bool,
    /// Seconds of production this unit takes at its training hall.
    pub train_time: Fx,
}

const NO_BONUS: [Fx; 4] = [ONE; 4];

/// `bonus_vs_armor` array with one override at `ArmorClass` index.
const fn bonus(ac: ArmorClass, mult: Fx) -> [Fx; 4] {
    let mut b = [ONE; 4];
    b[ac as usize] = mult;
    b
}

const DEFAULT: UnitDef = UnitDef {
    label: "",
    icon: "",
    role: UnitRole::Foot,
    factions: FACTION_BOTH,
    domain: Domain::Land,
    speed: crate::fx!("2.2"),
    carry: 0,
    cargo_cap: 0,
    pop_cost: 1,
    radius: crate::fx!("0.26"),
    height: crate::fx!("0.85"),
    max_hp: 60,
    attack: 0,
    damage_type: DamageType::Slash,
    armor_class: ArmorClass::Leather,
    damage_reduction: 0,
    bonus_vs_armor: NO_BONUS,
    range: crate::fx!("1"),
    min_range: crate::fx!("0"),
    attack_rate: crate::fx!("1"),
    attack_ticks: 5,
    aggro_range: crate::fx!("6"),
    cost: ResourceCost::ZERO,
    tint: None,
    requires: None,
    prefers_buildings: false,
    morale_aura: crate::fx!("0"),
    rally_aura: crate::fx!("0"),
    morale_resolve: crate::fx!("1"),
    charge_mult: crate::fx!("1"),
    brace: false,
    splash: crate::fx!("0"),
    arcs: false,
    setup_time: crate::fx!("0"),
    garrisonable: false,
    ranged: false,
    train_time: crate::fx!("12"),
};

const UNIT_DEFS: [UnitDef; 15] = [
    // 0 Peasant — the whole economy, and never a soldier.
    UnitDef {
        label: "Peasant",
        icon: "🧑‍🌾",
        role: UnitRole::Worker,
        speed: crate::fx!("2.5"),
        carry: 8,
        radius: crate::fx!("0.22"),
        height: crate::fx!("0.7"),
        max_hp: 30,
        damage_type: DamageType::Blunt,
        armor_class: ArmorClass::Unarmored,
        range: crate::fx!("0.8"),
        attack_rate: crate::fx!("0"),
        attack_ticks: 0,
        aggro_range: crate::fx!("0"),
        cost: ResourceCost::new(20, 0, 0, 0),
        morale_resolve: crate::fx!("0.7"),
        train_time: crate::fx!("6"),
        ..DEFAULT
    },
    // 1 Spearman — levy, shared. Cheap, fragile, and only dangerous to armour
    // while it is SET: the anti-mail bonus is gated on `brace`, so a spear line
    // that chases cavalry gives up the only thing it is good at.
    UnitDef {
        label: "Spearman",
        icon: "🛡️",
        role: UnitRole::Foot,
        max_hp: 65,
        attack: 10,
        damage_type: DamageType::Pierce,
        bonus_vs_armor: bonus(ArmorClass::Mail, crate::fx!("1.8")),
        range: crate::fx!("1.2"),
        cost: ResourceCost::new(40, 0, 0, 0),
        tint: Some(0x3a3a3a),
        morale_resolve: crate::fx!("0.9"),
        brace: true,
        garrisonable: true,
        ..DEFAULT
    },
    // 2 Archer — Ayyubid volume. Cheapest bow on the field; wins by numbers and
    // by never being reached.
    UnitDef {
        label: "Archer",
        icon: "🏹",
        role: UnitRole::Archer,
        factions: FACTION_AYYUBID,
        speed: crate::fx!("2.4"),
        radius: crate::fx!("0.24"),
        height: crate::fx!("0.8"),
        max_hp: 40,
        attack: 9,
        damage_type: DamageType::Pierce,
        range: crate::fx!("5"),
        attack_rate: crate::fx!("1.2"),
        attack_ticks: 6,
        aggro_range: crate::fx!("7"),
        cost: ResourceCost::new(32, 0, 0, 0),
        tint: Some(0x5a3a1a),
        morale_resolve: crate::fx!("0.8"),
        garrisonable: true,
        ranged: true,
        train_time: crate::fx!("10"),
        ..DEFAULT
    },
    // 3 Knight — Crusader shock. Mediocre sustained damage; the entire unit is
    // its CHARGE, which a braced spear wall frontally cancels.
    UnitDef {
        label: "Knight",
        icon: "🐎",
        role: UnitRole::Cavalry,
        factions: FACTION_CRUSADER,
        speed: crate::fx!("3.4"),
        radius: crate::fx!("0.3"),
        height: crate::fx!("1.0"),
        max_hp: 140,
        attack: 15,
        armor_class: ArmorClass::Mail,
        attack_rate: crate::fx!("1.2"),
        attack_ticks: 6,
        aggro_range: crate::fx!("7"),
        cost: ResourceCost::new(55, 0, 0, 35),
        tint: Some(0x9a8050),
        requires: Some(BuildingKind::Stable),
        morale_resolve: crate::fx!("1.15"),
        charge_mult: crate::fx!("3"),
        train_time: crate::fx!("20"),
        ..DEFAULT
    },
    // 4 Horse Archer — the Ayyubid core. Outruns everything that can hurt it.
    UnitDef {
        label: "Horse Archer",
        icon: "🏇",
        role: UnitRole::HorseArcher,
        factions: FACTION_AYYUBID,
        speed: crate::fx!("4.0"),
        radius: crate::fx!("0.28"),
        height: crate::fx!("0.95"),
        max_hp: 60,
        attack: 9,
        damage_type: DamageType::Pierce,
        range: crate::fx!("4.5"),
        attack_rate: crate::fx!("1.2"),
        attack_ticks: 6,
        aggro_range: crate::fx!("8"),
        cost: ResourceCost::new(35, 0, 0, 15),
        tint: Some(0x7a5a2a),
        requires: Some(BuildingKind::Stable),
        morale_resolve: crate::fx!("0.9"),
        ranged: true,
        train_time: crate::fx!("20"),
        ..DEFAULT
    },
    // 5 Mamluk — ghulam heavy horse. LOWER charge than a Knight, far better
    // sustained damage and staying power: the finisher, not the opener.
    UnitDef {
        label: "Mamluk",
        icon: "🗡️",
        role: UnitRole::Cavalry,
        factions: FACTION_AYYUBID,
        speed: crate::fx!("3.6"),
        radius: crate::fx!("0.31"),
        height: crate::fx!("1.05"),
        max_hp: 150,
        attack: 18,
        armor_class: ArmorClass::Mail,
        bonus_vs_armor: bonus(ArmorClass::Leather, crate::fx!("1.3")),
        cost: ResourceCost::new(0, 0, 55, 40),
        tint: Some(0xc9a24a),
        requires: Some(BuildingKind::Stable),
        morale_resolve: crate::fx!("1.25"),
        charge_mult: crate::fx!("1.6"),
        train_time: crate::fx!("22"),
        ..DEFAULT
    },
    // 6 Crossbowman — Crusader answer to mail and to engines. Slowest reload in
    // the game, hardest single bolt, and it keeps both inside a garrison.
    UnitDef {
        label: "Crossbowman",
        icon: "🎯",
        role: UnitRole::Archer,
        factions: FACTION_CRUSADER,
        speed: crate::fx!("2.0"),
        radius: crate::fx!("0.25"),
        height: crate::fx!("0.82"),
        max_hp: 52,
        attack: 15,
        damage_type: DamageType::Pierce,
        bonus_vs_armor: bonus(ArmorClass::Mail, crate::fx!("2.2")),
        range: crate::fx!("5.5"),
        attack_rate: crate::fx!("2.0"),
        attack_ticks: 10,
        aggro_range: crate::fx!("7"),
        cost: ResourceCost::new(40, 0, 0, 12),
        tint: Some(0x4a3a2a),
        morale_resolve: crate::fx!("0.85"),
        garrisonable: true,
        ranged: true,
        train_time: crate::fx!("16"),
        ..DEFAULT
    },
    // 7 Battering Ram — gate breaker. `aggro_range` was 0, which made it the one
    // unit in the game that could not act without a hand-click per segment.
    UnitDef {
        label: "Battering Ram",
        icon: "🪵",
        role: UnitRole::Siege,
        speed: crate::fx!("1.2"),
        radius: crate::fx!("0.5"),
        height: crate::fx!("1.1"),
        max_hp: 400,
        attack: 40,
        damage_type: DamageType::Siege,
        armor_class: ArmorClass::Mail,
        range: crate::fx!("1.5"),
        attack_rate: crate::fx!("2.4"),
        attack_ticks: 12,
        aggro_range: crate::fx!("4"),
        cost: ResourceCost::new(120, 0, 0, 0),
        tint: Some(0x6b4a2b),
        requires: Some(BuildingKind::SiegeWorkshop),
        prefers_buildings: true,
        train_time: crate::fx!("24"),
        ..DEFAULT
    },
    // 8 Mangonel — arcing bombardment. Its Shot event was dead code (`ranged`
    // was false), so the client's ballistic-boulder path was unreachable. The
    // trio `arcs` + `min_range` + `setup_time` is the emplacement decision: the
    // one thing that shells a parapet without felling the tower under it.
    UnitDef {
        label: "Mangonel",
        icon: "💥",
        role: UnitRole::Siege,
        speed: crate::fx!("1.0"),
        radius: crate::fx!("0.45"),
        height: crate::fx!("1.0"),
        max_hp: 90,
        attack: 30,
        damage_type: DamageType::Siege,
        armor_class: ArmorClass::Unarmored,
        range: crate::fx!("10"),
        min_range: crate::fx!("2"),
        attack_rate: crate::fx!("3.0"),
        attack_ticks: 15,
        aggro_range: crate::fx!("9"),
        cost: ResourceCost::new(100, 0, 0, 60),
        tint: Some(0x5a4632),
        requires: Some(BuildingKind::SiegeWorkshop),
        prefers_buildings: true,
        splash: crate::fx!("1.5"),
        arcs: true,
        setup_time: crate::fx!("3"),
        ranged: true,
        train_time: crate::fx!("28"),
        ..DEFAULT
    },
    // 9 Imam — the Ayyubid holding doctrine: wide passive sustain, no discipline
    // term. Attrition wins the ground the horse archers wore down.
    UnitDef {
        label: "Imam",
        icon: "🕌",
        role: UnitRole::Support,
        factions: FACTION_AYYUBID,
        speed: crate::fx!("2.6"),
        radius: crate::fx!("0.24"),
        armor_class: ArmorClass::Unarmored,
        max_hp: 50,
        damage_type: DamageType::Blunt,
        range: crate::fx!("0"),
        attack_rate: crate::fx!("0"),
        attack_ticks: 0,
        aggro_range: crate::fx!("0"),
        cost: ResourceCost::new(0, 0, 40, 0),
        tint: Some(0xe8e2d0),
        morale_aura: crate::fx!("9"),
        garrisonable: true,
        train_time: crate::fx!("18"),
        ..DEFAULT
    },
    // 10 Sergeant — professional mail foot, the discipline half of the
    // asymmetry. Highest resolve in the game: a Crusader line does not break at
    // half strength, and it braces.
    UnitDef {
        label: "Sergeant",
        icon: "⚔️",
        role: UnitRole::Foot,
        factions: FACTION_CRUSADER,
        speed: crate::fx!("2.0"),
        radius: crate::fx!("0.27"),
        height: crate::fx!("0.9"),
        max_hp: 110,
        attack: 13,
        armor_class: ArmorClass::Mail,
        range: crate::fx!("1.1"),
        cost: ResourceCost::new(48, 0, 0, 17),
        tint: Some(0x7a7f8a),
        requires: Some(BuildingKind::Blacksmith),
        morale_resolve: crate::fx!("1.4"),
        brace: true,
        garrisonable: true,
        train_time: crate::fx!("20"),
        ..DEFAULT
    },
    // 11 Chaplain — a different verb from the Imam, not a smaller radius: it
    // buys RECOVERY and DISCIPLINE over a narrow front instead of standing
    // sustain over a wide one.
    UnitDef {
        label: "Chaplain",
        icon: "✝",
        role: UnitRole::Support,
        factions: FACTION_CRUSADER,
        speed: crate::fx!("2.4"),
        radius: crate::fx!("0.24"),
        armor_class: ArmorClass::Unarmored,
        max_hp: 50,
        damage_type: DamageType::Blunt,
        range: crate::fx!("0"),
        attack_rate: crate::fx!("0"),
        attack_ticks: 0,
        aggro_range: crate::fx!("0"),
        cost: ResourceCost::new(0, 0, 30, 20),
        tint: Some(0xdcd6c4),
        rally_aura: crate::fx!("5"),
        garrisonable: true,
        train_time: crate::fx!("18"),
        ..DEFAULT
    },
    // 12 Naffatun — naft throwers. Splash makes them lethal to anything FORMED
    // or clumped and to an engine crew, and worthless against loose order: the
    // counterweight that stops "always march in Line" being correct.
    UnitDef {
        label: "Naffatun",
        icon: "🔥",
        role: UnitRole::Foot,
        factions: FACTION_AYYUBID,
        speed: crate::fx!("2.1"),
        radius: crate::fx!("0.25"),
        height: crate::fx!("0.84"),
        max_hp: 55,
        attack: 12,
        damage_type: DamageType::Blunt,
        bonus_vs_armor: bonus(ArmorClass::Mail, crate::fx!("1.3")),
        range: crate::fx!("1.6"),
        attack_rate: crate::fx!("1.6"),
        attack_ticks: 8,
        cost: ResourceCost::new(32, 0, 0, 23),
        tint: Some(0xb4552a),
        requires: Some(BuildingKind::Blacksmith),
        morale_resolve: crate::fx!("0.8"),
        splash: crate::fx!("1.2"),
        garrisonable: true,
        train_time: crate::fx!("16"),
        ..DEFAULT
    },
    // 13 Fishing Skiff — the only hand that can work a fishery, and the whole
    // on-ramp to the sea: 30 wood over a 40-wood hut. Carries two and a half
    // times a peasant's load because a haul is a round trip out and back.
    UnitDef {
        label: "Fishing Skiff",
        icon: "⛵",
        role: UnitRole::Boat,
        domain: Domain::Sea,
        speed: crate::fx!("2.6"),
        carry: 20,
        radius: crate::fx!("0.28"),
        height: crate::fx!("0.5"),
        max_hp: 90,
        armor_class: ArmorClass::Leather,
        range: crate::fx!("0.8"),
        attack_rate: crate::fx!("0"),
        attack_ticks: 0,
        aggro_range: crate::fx!("0"),
        cost: ResourceCost::new(30, 0, 0, 0),
        tint: Some(0x8a6a3a),
        morale_resolve: crate::fx!("0.7"),
        train_time: crate::fx!("10"),
        ..DEFAULT
    },
    // 14 Barge — the ferry, and the only thing that reaches the other island.
    // `radius` is EXACTLY the Ram's: the widest body already in the roster, so
    // the separation cell scan does not widen for every unit on the map.
    UnitDef {
        label: "Barge",
        icon: "🚢",
        role: UnitRole::Boat,
        domain: Domain::Sea,
        speed: crate::fx!("3.0"),
        carry: 0,
        cargo_cap: 6,
        pop_cost: 2,
        radius: crate::fx!("0.45"),
        height: crate::fx!("0.62"),
        max_hp: 220,
        armor_class: ArmorClass::Leather,
        range: crate::fx!("0.8"),
        attack_rate: crate::fx!("0"),
        attack_ticks: 0,
        aggro_range: crate::fx!("0"),
        cost: ResourceCost::new(60, 0, 0, 20),
        tint: Some(0x6b4a2b),
        morale_resolve: crate::fx!("1"),
        train_time: crate::fx!("20"),
        ..DEFAULT
    },
];

pub fn unit_def(kind: UnitKind) -> &'static UnitDef {
    &UNIT_DEFS[kind as usize]
}

impl UnitDef {
    /// Total resources one of these costs — the denominator every value
    /// judgement divides by.
    pub fn resource_cost(&self) -> i32 {
        (self.cost.wood + self.cost.stone + self.cost.food + self.cost.gold).max(1)
    }

    pub fn is_combatant(&self) -> bool {
        self.attack > 0
    }

    /// Draws rations. ROLE, not `attack > 0`: arming a peasant for self-defence
    /// must never silently put it on the muster roll. A hull is off the roll
    /// too — there is no supply model at sea, and a crew that deserts
    /// mid-crossing is not a feature.
    /// A siege engine is timber, rope and iron. It has no stomach, and neither
    /// does a hull — the crews are abstracted into the men who built them.
    pub fn draws_rations(&self) -> bool {
        !matches!(self.role, UnitRole::Worker | UnitRole::Support | UnitRole::Boat | UnitRole::Siege)
    }

    /// Raises and mends structures. ROLE, not `carry > 0`: a fishing skiff
    /// carries more than a peasant and cannot reach a building site at all.
    pub fn builds(&self) -> bool {
        self.role == UnitRole::Worker
    }

    /// Moves in `Domain::Sea`.
    pub fn afloat(&self) -> bool {
        self.domain == Domain::Sea
    }

    /// A pair of HANDS — what a land work order (gather that seam, mend that
    /// wall) may be handed to. `carry > 0` alone is no longer that question: a
    /// skiff carries two and a half times a peasant's load and cannot stand on
    /// any of the ground those orders point at.
    pub fn hands(&self) -> bool {
        self.carry > 0 && !self.afloat()
    }
}

/// The specialist bonus a unit actually applies this swing. A bracing unit only
/// gets its anti-armour multiplier while it is SET — that is the whole reason a
/// spear wall is a position rather than a stat line.
pub fn applied_bonus(def: &UnitDef, braced: bool) -> [Fx; 4] {
    if def.brace && !braced { NO_BONUS } else { def.bonus_vs_armor }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::COMBAT_TICK_MS;
    use crate::enums::Faction;
    use crate::roster::fields_unit;

    #[test]
    fn every_kind_indexes() {
        for &k in UnitKind::ALL {
            assert!(!unit_def(k).label.is_empty());
        }
        assert_eq!(UNIT_DEFS.len(), UnitKind::ALL.len());
        assert_eq!(unit_def(UnitKind::Naffatun).label, "Naffatun");
        assert_eq!(unit_def(UnitKind::Barge).label, "Barge");
    }

    /// The declared swing has to be a swing the 200 ms combat cadence can
    /// actually deliver. Eight kinds were out by 6.7-20.0% before this pinned
    /// them: a "1.1 s" Knight really swung every 1.2 s.
    #[test]
    fn the_declared_swing_is_the_swing_the_engine_can_deliver() {
        for &k in UnitKind::ALL {
            let d = unit_def(k);
            let declared_ms = (d.attack_rate * Fx::from_num(1000)).round().to_num::<i64>();
            assert_eq!(
                d.attack_ticks as i64 * COMBAT_TICK_MS,
                declared_ms,
                "{k:?} declares {declared_ms} ms but swings every {} ms",
                d.attack_ticks as i64 * COMBAT_TICK_MS
            );
            assert!(d.attack_ticks >= 0);
            assert_eq!(d.attack <= 0, d.attack_ticks == 0, "{k:?} cadence/attack disagree");
        }
    }

    #[test]
    fn a_spear_wall_only_bites_armour_while_it_is_set() {
        let d = unit_def(UnitKind::Spearman);
        assert!(d.brace);
        assert_eq!(applied_bonus(d, true)[ArmorClass::Mail as usize], crate::fx!("1.8"));
        assert_eq!(applied_bonus(d, false)[ArmorClass::Mail as usize], ONE);
        // a crossbow's bolt does not care whether the man moved
        let x = unit_def(UnitKind::Crossbowman);
        assert!(!x.brace);
        assert_eq!(applied_bonus(x, false)[ArmorClass::Mail as usize], crate::fx!("2.2"));
    }

    #[test]
    fn cavalry_requires_stable() {
        assert_eq!(unit_def(UnitKind::Knight).requires, Some(BuildingKind::Stable));
        assert_eq!(unit_def(UnitKind::Peasant).requires, None);
    }

    /// b804c80's delete test, moved from buildings to units: if two kinds answer
    /// "what do I give up by not having this?" identically, one should not
    /// exist. FACTION IS DELIBERATELY NOT IN THE SIGNATURE — two units doing the
    /// same job on opposite sides is still two units doing the same job.
    #[test]
    fn no_two_unit_kinds_share_a_role_signature() {
        #[derive(PartialEq, Debug)]
        struct Sig {
            role: u8,
            damage: u8,
            armor: u8,
            bonus: [Fx; 4],
            ranged: bool,
            arcs: bool,
            brace: bool,
            splash: bool,
            charge: bool,
            garrison: bool,
            siege: bool,
            sustain: bool,
            discipline: bool,
            min_range: bool,
            /// The two hulls are identical on every combat axis (they have
            /// none). What they haul IS the distinction: fish, or men.
            hauls: bool,
            ferries: bool,
        }
        let sig = |k: UnitKind| {
            let d = unit_def(k);
            Sig {
                role: d.role as u8,
                damage: d.damage_type as u8,
                armor: d.armor_class as u8,
                bonus: d.bonus_vs_armor,
                ranged: d.ranged,
                arcs: d.arcs,
                brace: d.brace,
                splash: d.splash > Fx::ZERO,
                charge: d.charge_mult > ONE,
                garrison: d.garrisonable,
                siege: d.prefers_buildings,
                sustain: d.morale_aura > Fx::ZERO,
                discipline: d.rally_aura > Fx::ZERO,
                min_range: d.min_range > Fx::ZERO,
                hauls: d.carry > 0,
                ferries: d.cargo_cap > 0,
            }
        };
        for (i, a) in UnitKind::ALL.iter().enumerate() {
            for b in &UnitKind::ALL[i + 1..] {
                assert_ne!(sig(*a), sig(*b), "{a:?} and {b:?} do the same job");
            }
        }
    }

    /// The two support kinds are the pair most at risk of being one unit with
    /// two labels, so they get their own assertion: different VERBS, not a
    /// radius tweak.
    #[test]
    fn the_two_supports_do_different_jobs() {
        let imam = unit_def(UnitKind::Imam);
        let chaplain = unit_def(UnitKind::Chaplain);
        assert!(imam.morale_aura > Fx::ZERO && imam.rally_aura == Fx::ZERO);
        assert!(chaplain.rally_aura > Fx::ZERO && chaplain.morale_aura == Fx::ZERO);
        assert!(!fields_unit(UnitKind::Imam, Faction::Crusader));
        assert!(!fields_unit(UnitKind::Chaplain, Faction::Ayyubid));
    }

    /// Roles are the single source of truth the upgrade table and the supply
    /// ledger read. A shape-derived role is what handed a ram plate barding.
    #[test]
    fn roles_are_consistent_with_the_stat_lines_they_describe() {
        for &k in UnitKind::ALL {
            let d = unit_def(k);
            match d.role {
                UnitRole::Worker => assert!(d.carry > 0 && d.attack == 0, "{k:?}"),
                UnitRole::Support => assert!(d.attack == 0 && !d.draws_rations(), "{k:?}"),
                UnitRole::Archer | UnitRole::HorseArcher => {
                    assert!(d.ranged && d.range >= crate::fx!("4"), "{k:?}")
                }
                UnitRole::Foot | UnitRole::Cavalry => {
                    assert!(d.attack > 0 && !d.ranged && d.range <= crate::fx!("2"), "{k:?}")
                }
                UnitRole::Siege => assert!(d.prefers_buildings && d.attack > 0, "{k:?}"),
                UnitRole::Boat => {
                    assert!(d.domain == Domain::Sea, "{k:?} is a boat on dry land");
                    assert!(d.attack == 0 && !d.draws_rations() && !d.builds(), "{k:?}");
                    assert!(!d.garrisonable, "{k:?} cannot be stuffed into a tower");
                    // a hull hauls fish OR men, never both and never neither
                    assert!((d.carry > 0) != (d.cargo_cap > 0), "{k:?} carries nothing at all");
                }
            }
            assert_eq!(d.domain == Domain::Sea, d.role == UnitRole::Boat, "{k:?} domain/role");
            assert!(d.pop_cost >= 1, "{k:?} is housed for free");
            assert!(d.morale_resolve > Fx::ZERO, "{k:?} has no resolve at all");
            assert!(d.charge_mult >= ONE, "{k:?} charge would REDUCE damage");
            assert_eq!(d.factions & !crate::roster::FACTION_BOTH, 0, "{k:?} stray faction bit");
            assert!(d.factions != 0, "{k:?} belongs to nobody");
            assert!(d.min_range < d.range || d.min_range == Fx::ZERO, "{k:?} dead zone eats its range");
        }
        // only workers haul, and only one kind is a worker
        let workers: Vec<_> =
            UnitKind::ALL.iter().filter(|k| unit_def(**k).role == UnitRole::Worker).collect();
        assert_eq!(workers.len(), 1);
    }

    #[test]
    fn the_siege_train_engages_and_arcs() {
        let ram = unit_def(UnitKind::Ram);
        assert!(ram.aggro_range > Fx::ZERO, "the ram cannot start a fight by itself");
        assert!(!ram.arcs && ram.splash == Fx::ZERO);
        let m = unit_def(UnitKind::Mangonel);
        assert!(m.ranged, "a mangonel that is not `ranged` never pushes a Shot");
        assert!(m.arcs && m.min_range > Fx::ZERO && m.setup_time > Fx::ZERO && m.splash > Fx::ZERO);
    }

    // ── equal-resource matchup model ────────────────────────────────────────
    // A stack of A against a stack of B bought with the SAME pile of resources,
    // stepped on the real 200 ms combat cadence. It credits the four things a
    // stat line alone cannot see: free shots taken while the enemy closes,
    // FRONTAGE (a line is only so many men wide, which is why the cheap unit
    // does not simply win by arithmetic), splash against a stack, and the
    // one-off blow a charge lands on contact. It is a model, not the engine —
    // its job is to catch a kind that has no answer and a kind that answers
    // everything.

    const DUEL_BUDGET: i32 = 1000;
    const DUEL_CAP_S: i32 = 240;
    /// Tiles of line the two stacks meet along.
    const FRONTAGE: Fx = crate::fx!("10");
    /// Ranks a missile block shoots from — depth helps a bow and not a spear.
    const RANKS: Fx = crate::fx!("2.5");
    /// Even a faster shooter is eventually brought to contact by terrain and by
    /// having somewhere to be; without a floor, kiting is worth infinity.
    const CLOSING_FLOOR: Fx = crate::fx!("1");

    fn per_hit(a: &UnitDef, b: &UnitDef) -> Fx {
        let atk = crate::combat::Attacker {
            attack: Fx::from_num(a.attack),
            damage_type: a.damage_type,
            bonus_vs_armor: applied_bonus(a, true),
        };
        Fx::from_num(crate::combat::effective_damage_vs(&atk, b))
    }

    fn dps(a: &UnitDef, b: &UnitDef, enemy_stack: Fx) -> Fx {
        if a.attack <= 0 || a.attack_ticks <= 0 {
            return Fx::ZERO;
        }
        let splash = if a.splash > Fx::ZERO && enemy_stack > ONE { ONE + a.splash } else { ONE };
        per_hit(a, b) * splash / (Fx::from_num(a.attack_ticks) * crate::constants::COMBAT_DT)
    }

    /// How many of `n` can reach the enemy at once.
    fn engaged(u: &UnitDef, n: Fx) -> Fx {
        let files = FRONTAGE / (u.radius * Fx::from_num(2));
        if u.range > crate::fx!("2") { n.min(files * RANKS) } else { n.min(files) }
    }

    /// Seconds the longer-ranged side shoots for free while the other closes.
    fn free_seconds(a: &UnitDef, b: &UnitDef) -> Fx {
        let gap = a.range - b.range;
        if gap <= Fx::ZERO {
            return Fx::ZERO;
        }
        (gap / (b.speed - a.speed).max(CLOSING_FLOOR)).min(crate::fx!("8"))
    }

    /// Positive when A wins, as surviving percentage of A minus that of B.
    fn duel(ka: UnitKind, kb: UnitKind) -> i32 {
        let (a, b) = (unit_def(ka), unit_def(kb));
        let na = Fx::from_num(DUEL_BUDGET) / Fx::from_num(a.resource_cost());
        let nb = Fx::from_num(DUEL_BUDGET) / Fx::from_num(b.resource_cost());
        let (full_a, full_b) = (na * Fx::from_num(a.max_hp), nb * Fx::from_num(b.max_hp));
        let (mut pool_a, mut pool_b) = (full_a, full_b);
        let dt = crate::constants::COMBAT_DT;
        let alive = |pool: Fx, hp: i32| (pool / Fx::from_num(hp)).max(Fx::ZERO);

        let (ta, tb) = (free_seconds(a, b), free_seconds(b, a));
        pool_b -= dps(a, b, nb) * engaged(a, na) * ta;
        pool_a -= dps(b, a, na) * engaged(b, nb) * tb;

        // the charge lands once, on contact, and a set spear cancels it
        let charge = |x: &UnitDef, y: &UnitDef, n: Fx| -> Fx {
            let mult = crate::combat::charge_multiplier(x.charge_mult, y.brace, true) - ONE;
            if mult <= Fx::ZERO { Fx::ZERO } else { per_hit(x, y) * mult * n }
        };
        pool_b -= charge(a, b, na);
        pool_a -= charge(b, a, nb);

        // an engine that cannot depress its arc contributes nothing in contact
        let fires = |x: &UnitDef, y: &UnitDef| x.min_range <= Fx::ZERO || y.range >= x.min_range;
        let (fa, fb) = (fires(a, b), fires(b, a));
        let mut t = ta.max(tb);
        while pool_a > Fx::ZERO && pool_b > Fx::ZERO && t < Fx::from_num(DUEL_CAP_S) {
            let (ca, cb) = (alive(pool_a, a.max_hp), alive(pool_b, b.max_hp));
            let da = if fa { dps(a, b, cb) * engaged(a, ca) } else { Fx::ZERO };
            let db = if fb { dps(b, a, ca) * engaged(b, cb) } else { Fx::ZERO };
            pool_a -= db * dt;
            pool_b -= da * dt;
            t += dt;
        }
        let pct = |pool: Fx, full: Fx| (pool.max(Fx::ZERO) * Fx::from_num(100) / full).to_num::<i32>();
        pct(pool_a, full_a) - pct(pool_b, full_b)
    }

    /// The measured state of the old roster: the Spearman beat every other kind
    /// at equal resources (29-0, 41-0, 41-0, 37-0, 33-0), the Mamluk strictly
    /// dominated the Knight and the Ram strictly dominated the Mangonel. A kind
    /// that answers everything deletes the other twelve.
    ///
    /// Siege sits outside this: an engine's job is a structure, and a mangonel
    /// losing a field duel to a spear block is the correct answer, not a bug —
    /// `a_siege_train_that_answers_two_different_questions` grades those two.
    #[test]
    fn no_field_kind_answers_everything_and_none_answers_nothing() {
        let fighters: Vec<UnitKind> = UnitKind::ALL
            .iter()
            .copied()
            .filter(|k| unit_def(*k).attack > 0 && unit_def(*k).role != UnitRole::Siege)
            .collect();
        let mut report = String::from("equal-resource margins (surviving % of A minus B)\n");
        report.push_str(&format!("{:<14}", ""));
        for &b in &fighters {
            report.push_str(&format!("{:>8}", &unit_def(b).label[..7.min(unit_def(b).label.len())]));
        }
        report.push_str("   W  L\n");
        let mut tally = Vec::new();
        for &a in &fighters {
            let (mut wins, mut losses) = (0, 0);
            report.push_str(&format!("{:<14}", unit_def(a).label));
            for &b in &fighters {
                if a == b {
                    report.push_str(&format!("{:>8}", "."));
                    continue;
                }
                let m = duel(a, b);
                report.push_str(&format!("{m:>8}"));
                if m > 0 {
                    wins += 1;
                } else if m < 0 {
                    losses += 1;
                }
            }
            report.push_str(&format!("{wins:>4}{losses:>3}\n"));
            tally.push((a, wins, losses));
        }
        println!("{report}");
        for (k, wins, losses) in tally {
            assert!(wins > 0, "{k:?} beats nothing at equal cost\n{report}");
            assert!(losses > 0, "{k:?} loses to nothing at equal cost\n{report}");
        }
    }

    /// The two pairs that were STRICTLY dominated, and were both sold from the
    /// same building to both factions — the clearest possible evidence that
    /// faction identity did not exist.
    #[test]
    fn the_strictly_dominated_pairs_are_gone() {
        // the knight's case is the charge, and a set spear is the answer to it
        assert!(duel(UnitKind::Knight, UnitKind::Mamluk) != 0);
        assert!(
            crate::combat::charge_multiplier(unit_def(UnitKind::Knight).charge_mult, true, true)
                < unit_def(UnitKind::Knight).charge_mult,
            "a braced spear has to cost the charge something"
        );
        assert!(
            unit_def(UnitKind::Knight).charge_mult > unit_def(UnitKind::Mamluk).charge_mult,
            "the knight has to hit harder on the charge than the ghulam it lost to"
        );
        assert!(
            unit_def(UnitKind::Mamluk).morale_resolve > unit_def(UnitKind::Knight).morale_resolve
                || unit_def(UnitKind::Mamluk).max_hp > unit_def(UnitKind::Knight).max_hp,
            "and the ghulam has to be the one that stays"
        );
    }

    /// Both engines exist because each answers a question the other cannot.
    #[test]
    fn a_siege_train_that_answers_two_different_questions() {
        use crate::buildings_defs::building_def;
        use crate::enums::BuildingKind;
        let seconds_to_breach = |k: UnitKind, b: BuildingKind| {
            let d = unit_def(k);
            let def = building_def(b);
            let atk = crate::combat::Attacker {
                attack: Fx::from_num(d.attack),
                damage_type: d.damage_type,
                bonus_vs_armor: d.bonus_vs_armor,
            };
            let per = crate::combat::building_damage(&atk, def).max(1);
            Fx::from_num((def.max_hp + per - 1) / per)
                * Fx::from_num(d.attack_ticks)
                * crate::constants::COMBAT_DT
        };
        assert!(
            seconds_to_breach(UnitKind::Ram, BuildingKind::Wall)
                < seconds_to_breach(UnitKind::Mangonel, BuildingKind::Wall),
            "the ram must be the thing that opens a wall"
        );
        // and the mangonel must be able to shell a manned parapet from outside
        // its reply — otherwise there is no reason to build one
        let watchtower = building_def(BuildingKind::Watchtower);
        assert!(
            unit_def(UnitKind::Mangonel).range > watchtower.range,
            "an engine that a tower outranges is an engine nobody builds"
        );
        assert!(unit_def(UnitKind::Ram).range < watchtower.range);
    }

    /// Each faction has to be able to answer every threat class with something,
    /// or the asymmetry is just a missing unit.
    #[test]
    fn both_rosters_cover_every_job() {
        for f in [Faction::Ayyubid, Faction::Crusader] {
            let mine: Vec<&UnitDef> = UnitKind::ALL
                .iter()
                .filter(|k| fields_unit(**k, f))
                .map(|k| unit_def(*k))
                .collect();
            let has = |r: UnitRole| mine.iter().any(|d| d.role == r);
            assert!(has(UnitRole::Worker), "{f:?} has no economy");
            assert!(has(UnitRole::Foot), "{f:?} has no line infantry");
            assert!(has(UnitRole::Siege), "{f:?} cannot open a wall");
            assert!(has(UnitRole::Support), "{f:?} has no morale answer");
            assert!(has(UnitRole::Cavalry), "{f:?} has no shock arm");
            assert!(
                mine.iter().any(|d| d.ranged && d.role != UnitRole::Siege),
                "{f:?} has no missile arm"
            );
            assert!(
                mine.iter().any(|d| d.bonus_vs_armor[ArmorClass::Mail as usize] > ONE),
                "{f:?} has no answer to armour"
            );
            // crossing the sea is not an asymmetry; WHAT you land is
            assert!(mine.iter().any(|d| d.carry > 0 && d.afloat()), "{f:?} cannot fish");
            assert!(mine.iter().any(|d| d.cargo_cap > 0), "{f:?} cannot cross water");
        }
    }
}
