use crate::buildings_defs::BuildingDef;
use crate::enums::BuildingKind;
use std::collections::HashSet;

/// True when there is no prerequisite, or `owned` contains it. Shared gate for
/// placing buildings and training units (the `requires` field).
pub fn has_prereq(owned: &HashSet<BuildingKind>, requires: Option<BuildingKind>) -> bool {
    match requires {
        None => true,
        Some(k) => owned.contains(&k),
    }
}

/// The first prerequisite `owned` is missing for `def` — the PRIMARY one first,
/// so the short lock label ("Requires Barracks") matches what `has_prereq`
/// alone would have said, then the additional set.
pub fn has_prereq_all(owned: &HashSet<BuildingKind>, def: &BuildingDef) -> Option<BuildingKind> {
    if let Some(k) = def.requires
        && !owned.contains(&k)
    {
        return Some(k);
    }
    def.prereqs.iter().copied().find(|k| !owned.contains(k))
}

/// Every prerequisite `def` needs, primary first — the lock note's full list.
pub fn all_prereqs(def: &BuildingDef) -> Vec<BuildingKind> {
    def.requires.into_iter().chain(def.prereqs.iter().copied()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate() {
        let mut owned = HashSet::new();
        assert!(has_prereq(&owned, None));
        assert!(!has_prereq(&owned, Some(BuildingKind::Barracks)));
        owned.insert(BuildingKind::Barracks);
        assert!(has_prereq(&owned, Some(BuildingKind::Barracks)));
    }
}
