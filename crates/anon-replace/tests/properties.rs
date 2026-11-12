//! Propriétés de `Replacer` et `Restorer`.

use anon_core::{BlockContext, BlockRole, EntityKind, Replacement, Session, Source, TextTransform};
use anon_replace::{Replacer, Restorer, SessionExt};
use proptest::prelude::*;

fn ctx() -> BlockContext {
    BlockContext::new("prop", BlockRole::Body)
}

/// Valeurs canoniques de la session de test (fictives).
const VALUES: &[(EntityKind, &str)] = &[
    (EntityKind::Personne, "Jean Dupont"),
    (EntityKind::Nom, "Dupont"),
    (EntityKind::Prenom, "Jean"),
    (EntityKind::Prenom, "Zoé"),
    (EntityKind::Email, "jean@example.com"),
    (EntityKind::Tel, "06 12 34 56 78"),
    (EntityKind::Societe, "ACME"),
    (EntityKind::Societe, "BTP"),
    (EntityKind::Lieu, "東京"),
    (EntityKind::Nom, "Martin"),
];

fn session() -> Session {
    let mut s = Session::new();
    for (kind, v) in VALUES {
        if *v == "Martin" {
            s.intern_with_case(*kind, v, Source::Manual, true);
        } else {
            s.intern(*kind, v, Source::Manual);
        }
    }
    s
}

/// Morceaux de texte qui ne forment aucune valeur sous une forme non canonique.
const FILLERS: &[&str] = &[
    "le",
    "la",
    "Martinique",
    "pierre",
    "btp",
    "martin",
    "été",
    "🙂",
    "👍🏽",
    ",",
    ".",
    "(",
    ")",
    "d'",
    "—",
    "Москва",
    "x",
    "42",
    "«",
    "»",
    "**",
    "_",
    "#",
    "Æsir",
    "straße",
];

/// Morceaux plus agressifs pour les textes quelconques.
const PIECES: &[&str] = &[
    "Jean",
    "Dupont",
    "Jean Dupont",
    "JEAN",
    "Zoé",
    "Zoe\u{301}",
    "jean@example.com",
    "06 12 34 56 78",
    "0612345678",
    "06.12.34",
    "ACME",
    "acme",
    "BTP",
    "東京",
    "Martin",
    "Martinique",
    "[PERSONNE_1]",
    "[personne 1]",
    "[PRÉNOM_3]",
    "\\[NOM_2\\]",
    "【",
    "】",
    "[",
    "]",
    "\\",
    "_",
    "-",
    "'",
    "’",
    "\u{00A0}",
    "\u{00AD}",
    "\u{200B}",
    "\u{301}",
    "\n",
    " ",
    "🙂",
    "ß",
    "Œ",
    "EMAIL_1@anon.invalid",
    "%40",
    "İ",
    "ﬁ",
    "\u{FEFF}",
];

fn check_well_formed(text: &str, reps: &[Replacement]) {
    let mut cursor = 0usize;
    for r in reps {
        assert!(r.start >= cursor, "chevauchement ou désordre");
        assert!(r.start <= r.end && r.end <= text.len(), "hors limites");
        assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
        cursor = r.end;
    }
}

fn pieces_text() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES), 0..20).prop_map(|v| v.concat())
}

fn any_text() -> impl Strategy<Value = String> {
    prop_oneof![any::<String>(), pieces_text(), "\\PC{0,60}"]
}

fn canonical_text() -> impl Strategy<Value = String> {
    let values: Vec<&'static str> = VALUES.iter().map(|(_, v)| *v).collect();
    let segment = prop_oneof![prop::sample::select(values), prop::sample::select(FILLERS),];
    prop::collection::vec(segment, 0..16).prop_map(|v| v.join(" "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn never_panics_and_well_formed(text in any_text()) {
        let s = session();
        let replacer = Replacer::new(&s);
        let restorer = Restorer::new(&s);
        let link = BlockContext::new("lien", BlockRole::Hyperlink);
        for c in [ctx(), link] {
            check_well_formed(&text, &replacer.replacements(&text, &c));
            check_well_formed(&text, &restorer.replacements(&text, &c));
        }
        let m = replacer.matches(&text);
        let mut cursor = 0;
        for x in m {
            prop_assert!(x.start >= cursor && x.start < x.end && x.end <= text.len());
            prop_assert!(text.is_char_boundary(x.start) && text.is_char_boundary(x.end));
            cursor = x.end;
        }
        let _ = replacer.apply_uri(&text);
        let _ = restorer.apply_uri(&text);
    }

    #[test]
    fn anonymize_then_restore_round_trips(text in canonical_text()) {
        let s = session();
        let anonymized = Replacer::new(&s).apply(&text, &ctx()).into_owned();
        for (_, v) in VALUES {
            if *v != "Martin" {
                prop_assert!(!anonymized.contains(v), "valeur restante");
            }
        }
        let restored = Restorer::new(&s).apply(&anonymized, &ctx()).into_owned();
        prop_assert_eq!(restored, text);
    }

    #[test]
    fn reanonymizing_changes_nothing(text in any_text()) {
        let s = session();
        let r = Replacer::new(&s);
        let once = r.apply(&text, &ctx()).into_owned();
        let twice = r.apply(&once, &ctx()).into_owned();
        prop_assert_eq!(&twice, &once);
        let uri_once = r.apply_uri(&text).into_owned();
        prop_assert_eq!(r.apply_uri(&uri_once).into_owned(), uri_once);
    }

    #[test]
    fn canonical_text_reanonymizing_changes_nothing(text in canonical_text()) {
        let s = session();
        let r = Replacer::new(&s);
        let once = r.apply(&text, &ctx()).into_owned();
        prop_assert_eq!(r.apply(&once, &ctx()).into_owned(), once);
    }
}
