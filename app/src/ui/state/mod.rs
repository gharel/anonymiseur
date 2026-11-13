//! État de l'interface, séparé du rendu : structures et transitions pures,
//! testables sans egui.

pub mod anonymize;
pub mod deanonymize;
pub mod files;
pub mod freetext;
pub mod generalize;
pub mod images;

/// Onglet principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Anonymize,
    Deanonymize,
    FreeText,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Anonymize, Tab::Deanonymize, Tab::FreeText];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Anonymize => "Anonymiser",
            Tab::Deanonymize => "Désanonymiser",
            Tab::FreeText => "Texte libre",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Tab::Anonymize => 0,
            Tab::Deanonymize => 1,
            Tab::FreeText => 2,
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_indices_round_trip() {
        for t in Tab::ALL {
            assert_eq!(Tab::from_index(t.index()), Some(t));
        }
        assert_eq!(Tab::from_index(3), None);
    }
}
