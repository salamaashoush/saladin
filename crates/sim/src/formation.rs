use crate::math::{Fx, V2, dist2};

// Formations, facing and flanking, all on a 16-point compass. A heading is a
// `u8` index into a CONST table of unit vectors, so there is no trig anywhere
// and two peers cannot disagree about which way a man is looking.

pub const HEADINGS: usize = 16;

/// Unit vectors at 22.5-degree steps, counter-clockwise from +X.
pub const HEADING_DIRS: [V2; HEADINGS] = [
    V2::new(crate::fx!("1"), crate::fx!("0")),
    V2::new(crate::fx!("0.92388"), crate::fx!("0.38268")),
    V2::new(crate::fx!("0.70711"), crate::fx!("0.70711")),
    V2::new(crate::fx!("0.38268"), crate::fx!("0.92388")),
    V2::new(crate::fx!("0"), crate::fx!("1")),
    V2::new(crate::fx!("-0.38268"), crate::fx!("0.92388")),
    V2::new(crate::fx!("-0.70711"), crate::fx!("0.70711")),
    V2::new(crate::fx!("-0.92388"), crate::fx!("0.38268")),
    V2::new(crate::fx!("-1"), crate::fx!("0")),
    V2::new(crate::fx!("-0.92388"), crate::fx!("-0.38268")),
    V2::new(crate::fx!("-0.70711"), crate::fx!("-0.70711")),
    V2::new(crate::fx!("-0.38268"), crate::fx!("-0.92388")),
    V2::new(crate::fx!("0"), crate::fx!("-1")),
    V2::new(crate::fx!("0.38268"), crate::fx!("-0.92388")),
    V2::new(crate::fx!("0.70711"), crate::fx!("-0.70711")),
    V2::new(crate::fx!("0.92388"), crate::fx!("-0.38268")),
];

/// Nearest compass point to `dir`. Sixteen dot products, no division, no trig;
/// ties break to the lower index so every peer agrees.
pub fn heading_of(dir: V2) -> u8 {
    let mut best = 0u8;
    let mut best_dot = Fx::MIN;
    for (i, d) in HEADING_DIRS.iter().enumerate() {
        let dot = dir.x * d.x + dir.y * d.y;
        if dot > best_dot {
            best_dot = dot;
            best = i as u8;
        }
    }
    best
}

pub fn heading_dir(heading: u8) -> V2 {
    HEADING_DIRS[heading as usize % HEADINGS]
}

/// The march axis and the axis across the line, for a given heading.
pub fn heading_axes(heading: u8) -> (V2, V2) {
    let f = heading_dir(heading);
    (f, V2::new(f.y, -f.x))
}

/// Take a slot offset authored in local space (+Y along the march, +X to the
/// right of the line) into world space.
pub fn rotate(local: V2, heading: u8) -> V2 {
    let (f, r) = heading_axes(heading);
    V2::new(r.x * local.x + f.x * local.y, r.y * local.x + f.y * local.y)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FormationShape {
    Line = 0,
    Column = 1,
    Wedge = 2,
    Box = 3,
}

impl FormationShape {
    pub const ALL: [FormationShape; 4] =
        [FormationShape::Line, FormationShape::Column, FormationShape::Wedge, FormationShape::Box];

    pub fn from_u8(v: u8) -> Option<FormationShape> {
        Self::ALL.iter().copied().find(|s| *s as u8 == v)
    }
}

/// Nothing marches tighter than this, whatever the radii say.
pub const MIN_PITCH: Fx = crate::fx!("0.9");
/// Breathing room on top of two bodies touching.
pub const PITCH_SLACK: Fx = crate::fx!("0.1");

/// Spacing a group of these radii can actually hold. The client's 0.85 grid is
/// TIGHTER than two Rams touching (0.5 + 0.5), so engines arrived
/// interpenetrating and then fought the separation pass for the rest of the
/// battle. Pitch is the widest PAIR in the group, not the average.
pub fn formation_pitch(radii: &[Fx]) -> Fx {
    let (mut first, mut second) = (Fx::ZERO, Fx::ZERO);
    for &r in radii {
        if r > first {
            second = first;
            first = r;
        } else if r > second {
            second = r;
        }
    }
    if radii.len() == 1 {
        second = first;
    }
    (first + second + PITCH_SLACK).max(MIN_PITCH)
}

fn isqrt_ceil(n: i32) -> i32 {
    let mut w = 1;
    while w * w < n.max(1) {
        w += 1;
    }
    w
}

/// Slot `i` of `count`, in local space: +Y is the march direction, +X the right
/// of the line. Slot 0 is always the anchor's own place.
pub fn slot_offset(shape: FormationShape, i: i32, count: i32, pitch: Fx) -> V2 {
    let n = count.max(1);
    let i = i.clamp(0, n - 1);
    match shape {
        FormationShape::Line => {
            let centred = Fx::from_num(i) - Fx::from_num(n - 1) / Fx::from_num(2);
            V2::new(centred * pitch, Fx::ZERO)
        }
        FormationShape::Column => V2::new(Fx::ZERO, -Fx::from_num(i) * pitch),
        FormationShape::Wedge => {
            let row = (i + 1) / 2;
            let side = if i % 2 == 0 { Fx::ONE } else { -Fx::ONE };
            V2::new(side * Fx::from_num(row) * pitch, -Fx::from_num(row) * pitch)
        }
        FormationShape::Box => {
            let w = isqrt_ceil(n);
            let col = Fx::from_num(i % w) - Fx::from_num(w - 1) / Fx::from_num(2);
            V2::new(col * pitch, -Fx::from_num(i / w) * pitch)
        }
    }
}

/// Sort key that orders both slots and men the same way: front rank first, then
/// left to right. Sorting the two by ONE key is what stops the crossings — slots
/// used to go out in `GameId` order, and 402 of 435 measured pairs crossed.
fn march_key(local: V2) -> (Fx, Fx) {
    (-local.y, local.x)
}

/// Give every member of `members` (GameId, position) its own slot around
/// `anchor`. Scratch buffers are caller-owned so a group order allocates
/// nothing per call after the first.
#[allow(clippy::too_many_arguments)]
pub fn assign_slots(
    members: &[(u64, V2)],
    anchor: V2,
    heading: u8,
    shape: FormationShape,
    pitch: Fx,
    scratch: &mut Vec<(Fx, Fx, u64)>,
    slots: &mut Vec<(Fx, Fx, V2)>,
    out: &mut Vec<(u64, V2)>,
) {
    out.clear();
    if members.is_empty() {
        return;
    }
    let n = members.len() as i32;
    let (fwd, right) = heading_axes(heading);

    slots.clear();
    for i in 0..n {
        let local = slot_offset(shape, i, n, pitch);
        let (a, b) = march_key(local);
        let w = rotate(local, heading);
        slots.push((a, b, V2::new(anchor.x + w.x, anchor.y + w.y)));
    }
    slots.sort_by_key(|s| (s.0, s.1));

    scratch.clear();
    for (id, pos) in members {
        let rel = V2::new(pos.x - anchor.x, pos.y - anchor.y);
        let local = V2::new(rel.x * right.x + rel.y * right.y, rel.x * fwd.x + rel.y * fwd.y);
        let (a, b) = march_key(local);
        scratch.push((a, b, *id));
    }
    // GameId is the tie-break, so two men standing on the same spot on two
    // different clients still take the same slots.
    scratch.sort_by_key(|s| (s.0, s.1, s.2));

    for (i, (_, _, id)) in scratch.iter().enumerate() {
        out.push((*id, slots[i].2));
    }
}

// ── facing ───────────────────────────────────────────────────────────────────

/// Half-width of the frontal arc, as the cosine of the angle: 0.5 == 60 degrees
/// either side of where the man is looking.
pub const FRONT_COS: Fx = crate::fx!("0.5");
/// Behind this cosine the blow lands on the back — the rear attack.
pub const REAR_COS: Fx = crate::fx!("-0.5");

/// Cosine-squared compare against a threshold, without a square root: the whole
/// point of `dist2` discipline applied to angles.
fn arc_cmp(heading: u8, at: V2, from: V2, cos_t: Fx, want_front: bool) -> bool {
    let rel = V2::new(from.x - at.x, from.y - at.y);
    let d2 = dist2(at, from);
    if d2 <= Fx::ZERO {
        return want_front;
    }
    let f = heading_dir(heading);
    let dot = rel.x * f.x + rel.y * f.y;
    let rhs = cos_t * cos_t * d2;
    if want_front {
        dot > Fx::ZERO && dot * dot >= rhs
    } else {
        dot < Fx::ZERO && dot * dot >= rhs
    }
}

/// Is the blow coming at the face of a man at `at` looking along `heading`?
pub fn is_frontal(heading: u8, at: V2, from: V2) -> bool {
    arc_cmp(heading, at, from, FRONT_COS, true)
}

/// Is it coming at his back?
pub fn is_rear(heading: u8, at: V2, from: V2) -> bool {
    arc_cmp(heading, at, from, REAR_COS, false)
}

/// Everything that is neither.
pub fn is_flank(heading: u8, at: V2, from: V2) -> bool {
    !is_frontal(heading, at, from) && !is_rear(heading, at, from)
}

/// Damage multiplier for where the blow landed. A rear attack beats a frontal
/// one, and that is the whole reason position is worth manoeuvring for.
pub const FLANK_MULT: Fx = crate::fx!("1.25");
pub const REAR_MULT: Fx = crate::fx!("1.5");

pub fn facing_multiplier(heading: u8, at: V2, from: V2) -> Fx {
    if is_rear(heading, at, from) {
        REAR_MULT
    } else if is_frontal(heading, at, from) {
        Fx::ONE
    } else {
        FLANK_MULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::unit_def;

    fn at(x: &str, y: &str) -> V2 {
        V2::new(Fx::lit(x), Fx::lit(y))
    }

    #[test]
    fn the_compass_is_a_compass() {
        for (i, d) in HEADING_DIRS.iter().enumerate() {
            let len2 = d.x * d.x + d.y * d.y;
            assert!((len2 - Fx::ONE).abs() < crate::fx!("0.0001"), "heading {i} is not a unit vector");
            assert_eq!(heading_of(*d), i as u8, "heading {i} does not round-trip");
        }
        assert_eq!(heading_of(V2::new(crate::fx!("5"), crate::fx!("0"))), 0);
        assert_eq!(heading_of(V2::new(crate::fx!("0"), crate::fx!("-9"))), 12);
        // rotation preserves length, so a slot never drifts with the heading
        let local = V2::new(crate::fx!("3"), crate::fx!("-2"));
        for h in 0..HEADINGS as u8 {
            let w = rotate(local, h);
            let l2 = w.x * w.x + w.y * w.y;
            assert!((l2 - crate::fx!("13")).abs() < crate::fx!("0.001"), "heading {h} stretched a slot");
        }
    }

    /// The client's 0.85 grid is tighter than two Rams touching, so engines
    /// arrived interpenetrating and then fought the separation pass forever.
    #[test]
    fn a_siege_train_gets_enough_room_to_stand_in() {
        use crate::enums::UnitKind;
        let radii = [unit_def(UnitKind::Ram).radius, unit_def(UnitKind::Mangonel).radius];
        let pitch = formation_pitch(&radii);
        assert!(pitch >= radii[0] + radii[1], "a ram and a mangonel overlap at {pitch}");
        assert!(pitch > crate::fx!("0.85"), "the shipped client grid was {pitch} or tighter");
        // two rams need more room than two archers
        let rams = [unit_def(UnitKind::Ram).radius; 2];
        let archers = [unit_def(UnitKind::Archer).radius; 2];
        assert!(formation_pitch(&rams) > formation_pitch(&archers));
        // and a lone unit still gets a real pitch
        assert!(formation_pitch(&[unit_def(UnitKind::Archer).radius]) >= MIN_PITCH);
    }

    #[test]
    fn every_shape_lays_out_without_two_men_on_one_spot() {
        let pitch = crate::fx!("1");
        for shape in FormationShape::ALL {
            for count in [1, 2, 5, 12, 37] {
                let mut seen: Vec<V2> = Vec::new();
                for i in 0..count {
                    let o = slot_offset(shape, i, count, pitch);
                    for p in &seen {
                        assert!(
                            dist2(*p, o) > crate::fx!("0.0001"),
                            "{shape:?} put two men on one spot at {count}"
                        );
                    }
                    seen.push(o);
                }
            }
            assert_eq!(slot_offset(shape, 0, 1, pitch), V2::ZERO, "{shape:?} slot 0");
        }
        // a line is wide and shallow, a column is narrow and deep
        let line = slot_offset(FormationShape::Line, 9, 10, pitch);
        let col = slot_offset(FormationShape::Column, 9, 10, pitch);
        assert!(line.x.abs() > line.y.abs() && col.y.abs() > col.x.abs());
    }

    /// Slots used to go out in sorted-`GameId` order, so 402 of 435 measured
    /// pairs crossed on the way to the line. Ordering the men and the slots by
    /// the same march key costs one sort and removes the crossings.
    #[test]
    fn men_do_not_cross_each_other_reaching_their_slots() {
        let anchor = at("40", "40");
        let heading = heading_of(V2::new(crate::fx!("1"), crate::fx!("0")));
        // a column of ten standing in exactly the WRONG order
        let members: Vec<(u64, V2)> = (0..10)
            .map(|i| (i as u64, V2::new(crate::fx!("36"), Fx::from_num(49 - i))))
            .collect();
        let (mut s1, mut s2, mut out) = (Vec::new(), Vec::new(), Vec::new());
        assign_slots(&members, anchor, heading, FormationShape::Line, crate::fx!("1"), &mut s1, &mut s2, &mut out);
        assert_eq!(out.len(), members.len());

        let pos_of = |id: u64| members.iter().find(|(i, _)| *i == id).unwrap().1;
        let mut crossings = 0;
        for i in 0..out.len() {
            for j in i + 1..out.len() {
                let (a, ta) = (pos_of(out[i].0), out[i].1);
                let (b, tb) = (pos_of(out[j].0), out[j].1);
                // two paths cross when the men swap sides of the march axis
                let before = a.y - b.y;
                let after = ta.y - tb.y;
                if before != Fx::ZERO && after != Fx::ZERO && (before > Fx::ZERO) != (after > Fx::ZERO)
                {
                    crossings += 1;
                }
            }
        }
        assert_eq!(crossings, 0, "{crossings} of 45 pairs crossed");
    }

    #[test]
    fn slot_assignment_is_the_same_on_every_peer() {
        let anchor = at("20", "20");
        // two men on the SAME spot: only GameId can separate them
        let a = vec![(7u64, at("20", "20")), (3u64, at("20", "20"))];
        let b = vec![(3u64, at("20", "20")), (7u64, at("20", "20"))];
        let (mut s1, mut s2, mut oa, mut ob) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        assign_slots(&a, anchor, 4, FormationShape::Box, crate::fx!("1"), &mut s1, &mut s2, &mut oa);
        let mut s3 = Vec::new();
        let mut s4 = Vec::new();
        assign_slots(&b, anchor, 4, FormationShape::Box, crate::fx!("1"), &mut s3, &mut s4, &mut ob);
        oa.sort_by_key(|(id, _)| *id);
        ob.sort_by_key(|(id, _)| *id);
        assert_eq!(oa, ob);
    }

    /// Facing did not exist, so a rear attack and a frontal one were the same
    /// attack. All of this is squared compares — no sqrt, no atan2.
    #[test]
    fn a_blow_from_behind_lands_harder_than_one_to_the_face() {
        let man = at("10", "10");
        let east = 0u8; // looking along +X
        assert!(is_frontal(east, man, at("14", "10")));
        assert!(is_rear(east, man, at("6", "10")));
        assert!(is_flank(east, man, at("10", "14")));
        assert!(facing_multiplier(east, man, at("6", "10")) > facing_multiplier(east, man, at("10", "14")));
        assert!(facing_multiplier(east, man, at("10", "14")) > facing_multiplier(east, man, at("14", "10")));
        assert_eq!(facing_multiplier(east, man, at("14", "10")), Fx::ONE);
        // exactly three sectors, and every direction lands in one of them
        for h in 0..HEADINGS as u8 {
            for (k, d) in HEADING_DIRS.iter().enumerate() {
                let p = V2::new(man.x + d.x * crate::fx!("3"), man.y + d.y * crate::fx!("3"));
                let n = [is_frontal(h, man, p), is_rear(h, man, p), is_flank(h, man, p)]
                    .iter()
                    .filter(|b| **b)
                    .count();
                assert_eq!(n, 1, "heading {h} vs bearing {k} fell in {n} sectors");
            }
        }
    }
}
