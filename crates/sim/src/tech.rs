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
