//! Tests des données sensibles et des événements personnels (phrases fictives).

use super::*;
use crate::pipeline::merge;
use crate::{QuasiDetector, RuleDetector};
use anon_core::{BlockContext, BlockRole, DetectOptions, Detector, Profile, TextBlock};
use proptest::prelude::*;

/// Détections fusionnées (comme le fait le pipeline), tous types voulus.
fn found(text: &str) -> Vec<(EntityKind, String)> {
    let mut out = Vec::new();
    detect(text, |_| true, &mut out);
    for d in &out {
        assert!(d.start < d.end && d.end <= text.len(), "{d:?}");
        assert!(text.is_char_boundary(d.start) && text.is_char_boundary(d.end));
    }
    merge(out)
        .into_iter()
        .map(|d| (d.kind, text[d.start..d.end].to_string()))
        .collect()
}

#[track_caller]
fn assert_found(text: &str, kind: EntityKind, value: &str) {
    let got = found(text);
    assert!(
        got.contains(&(kind, value.to_string())),
        "{text:?} : attendu {kind:?} « {value} », obtenu {got:?}"
    );
}

#[track_caller]
fn assert_none(text: &str) {
    let got = found(text);
    assert!(got.is_empty(), "{text:?} : rien attendu, obtenu {got:?}");
}

#[test]
fn every_rule_compiles_and_every_kind_has_a_lexicon() {
    let e = engine();
    assert_eq!(e.rules.len(), RULES.len(), "une règle ne compile pas");
    assert!(e.set.is_some(), "l'ensemble des règles ne compile pas");
    for kind in EntityKind::SENSITIVE
        .iter()
        .copied()
        .chain([EntityKind::Evenement])
    {
        assert!(
            RULES.iter().any(|r| r.kind == kind),
            "aucune règle pour {kind:?}"
        );
        assert!(
            lexicon().keys().any(|(k, _)| *k == kind),
            "aucun lexique pour {kind:?}"
        );
    }
    // Les formes sont pliées et sans doublon.
    for forms in lexicon().values() {
        for f in forms {
            assert_eq!(&fold_text(f), f);
        }
        let set: std::collections::HashSet<_> = forms.iter().collect();
        assert_eq!(set.len(), forms.len());
    }
    let t = lexicon()
        .get(&(EntityKind::Sante, Class::Term))
        .map_or(0, Vec::len);
    assert!(t >= 150, "{t} termes de santé");
}

#[test]
fn folding_keeps_offsets() {
    let f = Folded::new("Cœur  ÉTÉ\u{a0}d’été\nX");
    assert_eq!(f.text, "coeur ete d'ete\nx");
    assert_eq!(f.src.len(), f.text.len());
    let text = "Cœur  ÉTÉ\u{a0}d’été\nX";
    // « o » seul (moitié de « oe ») renvoie au caractère « œ » entier.
    let (s, e) = f.source(1, 2).unwrap();
    assert_eq!(&text[s..e], "œ");
    let (s, e) = f.source(0, 5).unwrap();
    assert_eq!(&text[s..e], "Cœur");
    let (s, e) = f.source(6, 9).unwrap();
    assert_eq!(&text[s..e], "ÉTÉ");
    assert_eq!(f.source(3, 3), None);
}

#[test]
fn health_with_a_personal_cue() {
    use EntityKind::Sante;
    assert_found(
        "Mme Durand est atteinte d'un cancer du sein.",
        Sante,
        "atteinte d'un cancer du sein",
    );
    assert_found(
        "Il souffre de dépression depuis 2024.",
        Sante,
        "souffre de dépression",
    );
    assert_found(
        "M. Martin, suivi pour une sclérose en plaques, reprend le travail.",
        Sante,
        "suivi pour une sclérose en plaques",
    );
    assert_found(
        "Elle est en arrêt maladie du 3 au 14 mars.",
        Sante,
        "arrêt maladie",
    );
    assert_found(
        "Il prolonge son arrêt de travail ; son congé maladie finit lundi.",
        Sante,
        "congé maladie",
    );
    assert_found("Son diabète est équilibré.", Sante, "diabète");
    assert_found("Ses crises d'épilepsie sont rares.", Sante, "épilepsie");
    assert_found("Il est diabétique.", Sante, "diabétique");
    assert_found(
        "Elle est enceinte de six mois.",
        Sante,
        "enceinte de six mois",
    );
    assert_found(
        "Il a été hospitalisé la semaine dernière.",
        Sante,
        "hospitalisé",
    );
    assert_found("Elle a été opérée du genou.", Sante, "opérée du genou");
    assert_found(
        "Il est sous antidépresseurs.",
        Sante,
        "sous antidépresseurs",
    );
    assert_found("Absente pour raisons de santé.", Sante, "raisons de santé");
    assert_found(
        "Il a été déclaré inapte à son poste.",
        Sante,
        "déclaré inapte à son poste",
    );
    assert_found(
        "John was diagnosed with type 2 diabetes in May.",
        Sante,
        "diagnosed with type 2 diabetes",
    );
    assert_found("She is on sick leave until Friday.", Sante, "sick leave");
    assert_found("She is pregnant.", Sante, "pregnant");
}

#[test]
fn health_words_without_a_person_are_ignored() {
    for t in [
        "Le diabète touche 10 % de la population.",
        "Les patients atteints de diabète doivent consulter.",
        "En cas d'arrêt maladie, le salarié prévient son employeur.",
        "Si le salarié est en arrêt maladie, il transmet le certificat.",
        "Tout salarié en arrêt maladie reste joignable.",
        "La convention prévoit un arrêt maladie indemnisé.",
        "Le projet souffre de retards.",
        "Ce module calcule l'incidence du cancer du sein.",
        "La fracture numérique progresse.",
        "Le contrat est invalide.",
        "The dataset covers diabetes prevalence.",
        "Any employee on sick leave must inform the manager.",
        "Employees who are pregnant may ask for a new schedule.",
        "La dépression tropicale se renforce.",
    ] {
        assert_none(t);
    }
}

#[test]
fn convictions_and_offences() {
    use EntityKind::Judiciaire;
    assert_found(
        "Il a été condamné pour vol en 2019.",
        Judiciaire,
        "condamné pour vol",
    );
    assert_found(
        "Elle a été condamnée à six mois de prison avec sursis.",
        Judiciaire,
        "condamnée à six mois de prison avec sursis",
    );
    assert_found(
        "condamné par le tribunal correctionnel de Nouméa pour abus de confiance",
        Judiciaire,
        "condamné par le tribunal correctionnel de Nouméa pour abus de confiance",
    );
    assert_found(
        "Il a été placé en garde à vue mardi.",
        Judiciaire,
        "placé en garde à vue",
    );
    assert_found(
        "Son casier judiciaire mentionne une amende.",
        Judiciaire,
        "casier judiciaire",
    );
    assert_found(
        "Il a été reconnu coupable de harcèlement moral.",
        Judiciaire,
        "reconnu coupable de harcèlement moral",
    );
    assert_found(
        "Elle a porté plainte pour harcèlement sexuel.",
        Judiciaire,
        "a porté plainte pour harcèlement sexuel",
    );
    assert_found(
        "He was convicted of fraud in 2021.",
        Judiciaire,
        "convicted of fraud",
    );
    assert_found(
        "He was sentenced to two years in prison.",
        Judiciaire,
        "sentenced to two years in prison",
    );
}

#[test]
fn no_offence_without_a_person() {
    for t in [
        "La porte a été condamnée.",
        "L'auteur encourt une condamnation.",
        "Toute personne condamnée pour vol perd ses droits.",
        "Il peut être placé en garde à vue.",
        "La mise en examen est une mesure grave.",
        "Le capital détenu par la société augmente.",
        "Le vol Nouméa-Sydney part à 9 h.",
        "Son vol est retardé.",
    ] {
        assert_none(t);
    }
}

#[test]
fn political_opinions() {
    use EntityKind::Opinion;
    assert_found(
        "Pierre, militant du Parti socialiste, a pris la parole.",
        Opinion,
        "militant du Parti socialiste",
    );
    assert_found("Il est communiste.", Opinion, "communiste");
    assert_found("Elle est d'extrême droite.", Opinion, "d'extrême droite");
    assert_found(
        "Il a voté pour le Rassemblement national.",
        Opinion,
        "a voté pour le Rassemblement national",
    );
    assert_found(
        "Il est membre d'une loge maçonnique.",
        Opinion,
        "membre d'une loge maçonnique",
    );
    assert_found(
        "Son engagement politique est connu.",
        Opinion,
        "engagement politique",
    );
    assert_found(
        "élue de Calédonie ensemble",
        Opinion,
        "élue de Calédonie ensemble",
    );
    assert_found(
        "She is a Labour Party member.",
        Opinion,
        "Labour Party member",
    );
    for t in [
        "Le Parti socialiste a présenté son programme.",
        "Les militants du Parti socialiste manifestent.",
        "La loi encadre le financement des partis politiques.",
        "Le Labour a gagné les élections.",
    ] {
        assert_none(t);
    }
}

#[test]
fn trade_union_membership() {
    use EntityKind::Syndicat;
    assert_found(
        "M. Kaloï, délégué syndical CGT, conteste la décision.",
        Syndicat,
        "délégué syndical CGT",
    );
    assert_found(
        "Elle est adhérente de la CFDT.",
        Syndicat,
        "adhérente de la CFDT",
    );
    assert_found("Il est syndiqué.", Syndicat, "syndiqué");
    assert_found(
        "Il est membre du syndicat USTKE.",
        Syndicat,
        "membre du syndicat USTKE",
    );
    assert_found(
        "Elle est membre du syndicat depuis 2010.",
        Syndicat,
        "membre du syndicat",
    );
    assert_found(
        "He is a union representative.",
        Syndicat,
        "union representative",
    );
    for t in [
        "Le délégué syndical est désigné par le syndicat.",
        "La CGT appelle à la grève.",
        "Il est membre du syndicat de copropriété.",
        "European Union member states agreed.",
        "Les salariés syndiqués votent.",
    ] {
        assert_none(t);
    }
}

#[test]
fn religion() {
    use EntityKind::Religion;
    assert_found(
        "Elle est musulmane pratiquante.",
        Religion,
        "musulmane pratiquante",
    );
    assert_found(
        "Il est de confession juive.",
        Religion,
        "de confession juive",
    );
    assert_found(
        "Il s'est converti à l'islam.",
        Religion,
        "s'est converti à l'islam",
    );
    assert_found("Elle porte le voile.", Religion, "porte le voile");
    assert_found(
        "Il fréquente la mosquée du quartier.",
        Religion,
        "fréquente la mosquée",
    );
    assert_found("Sa religion lui interdit l'alcool.", Religion, "religion");
    assert_found(
        "He is a practising Catholic.",
        Religion,
        "practising Catholic",
    );
    for t in [
        "L'église Saint-Joseph ouvre à 9 h.",
        "Le règlement interdit de porter le voile.",
        "Le catholicisme est la religion majoritaire.",
        "Sa foi en l'avenir est intacte.",
    ] {
        assert_none(t);
    }
}

#[test]
fn sexual_orientation() {
    use EntityKind::Orientation;
    assert_found("Il est homosexuel.", Orientation, "homosexuel");
    assert_found(
        "Elle a fait son coming out en 2020.",
        Orientation,
        "a fait son coming out",
    );
    assert_found(
        "Son orientation sexuelle ne regarde que lui.",
        Orientation,
        "orientation sexuelle",
    );
    assert_found("He came out as gay.", Orientation, "came out as gay");
    for t in [
        "La loi protège l'orientation sexuelle.",
        "Gay est un nom de famille.",
        "Le mariage pour tous date de 2013.",
    ] {
        assert_none(t);
    }
}

#[test]
fn ethnic_origin() {
    use EntityKind::Origine;
    assert_found(
        "Un homme d'origine maghrébine a été vu.",
        Origine,
        "d'origine maghrébine",
    );
    assert_found("Il est kanak.", Origine, "kanak");
    assert_found("Un individu de type européen.", Origine, "de type européen");
    assert_found(
        "Elle est issue de la communauté wallisienne.",
        Origine,
        "issue de la communauté wallisienne",
    );
    assert_found(
        "She is of Vietnamese descent.",
        Origine,
        "of Vietnamese descent",
    );
    for t in [
        "Des produits d'origine asiatique sont vendus.",
        "Un vin rouge d'origine italienne.",
        "Une cuisine de type asiatique.",
        "La communauté kanak a voté.",
        "Le nom d'origine bretonne.",
    ] {
        assert_none(t);
    }
}

#[test]
fn genetic_and_biometric_data() {
    use EntityKind::{Biometrie, Genetique};
    assert_found(
        "Elle est porteuse d'une mutation du gène BRCA2.",
        Genetique,
        "porteuse d'une mutation du gène BRCA2",
    );
    assert_found(
        "Son test génétique est revenu positif.",
        Genetique,
        "test génétique",
    );
    assert_found("Il a passé un test ADN.", Genetique, "a passé un test ADN");
    assert_found(
        "Ses empreintes digitales ont été relevées.",
        Biometrie,
        "empreintes digitales",
    );
    assert_found(
        "Le gabarit biométrique de M. Dupont est conservé.",
        Biometrie,
        "gabarit biométrique",
    );
    for t in [
        "Le gène BRCA1 est impliqué dans la réparation de l'ADN.",
        "Le traitement des données biométriques est encadré.",
        "Cette valeur est dans son ADN.",
    ] {
        assert_none(t);
    }
}

#[test]
fn personal_events() {
    use EntityKind::Evenement;
    assert_found(
        "Il a été licencié pour faute grave.",
        Evenement,
        "licencié pour faute grave",
    );
    assert_found(
        "Sa mise à pied a duré trois jours.",
        Evenement,
        "mise à pied",
    );
    assert_found(
        "Elle est en instance de divorce.",
        Evenement,
        "en instance de divorce",
    );
    assert_found(
        "Absent après le décès de son père.",
        Evenement,
        "décès de son père",
    );
    assert_found(
        "Il a fait l'objet d'une procédure disciplinaire.",
        Evenement,
        "procédure disciplinaire",
    );
    assert_found(
        "Elle a été convoquée à un entretien préalable.",
        Evenement,
        "convoquée à un entretien préalable",
    );
    assert_found(
        "Il est en arrêt de travail depuis lundi.",
        Evenement,
        "arrêt de travail",
    );
    assert_found(
        "Il a reçu sa lettre de licenciement.",
        Evenement,
        "licenciement",
    );
    assert_found(
        "He was dismissed for gross misconduct.",
        Evenement,
        "dismissed for gross misconduct",
    );
    for t in [
        "La procédure de licenciement comprend trois étapes.",
        "Les licenciés du club se réunissent.",
        "Le licenciement économique est encadré par la loi.",
        "Les délégués ont rappelé que tout salarié en arrêt de travail doit prévenir son employeur.",
        "Il est licencié en droit.",
    ] {
        assert_none(t);
    }
}

fn block(text: &str) -> TextBlock {
    TextBlock {
        text: text.to_string(),
        ctx: BlockContext::new("test", BlockRole::Body),
    }
}

/// Les catégories particulières passent par les règles (deux profils), les
/// événements par les quasi-identifiants (profil « Renforcé »).
#[test]
fn detectors_and_profiles() {
    let text = "Il est atteint d'un cancer et a été licencié pour faute grave.";
    let standard = DetectOptions::default();
    let renforce = DetectOptions {
        profile: Profile::Renforce,
        ..Default::default()
    };
    let kinds = |d: &dyn Detector, o: &DetectOptions| -> Vec<EntityKind> {
        d.detect(&block(text), o).iter().map(|d| d.kind).collect()
    };
    assert!(kinds(&RuleDetector, &standard).contains(&EntityKind::Sante));
    assert!(!kinds(&RuleDetector, &standard).contains(&EntityKind::Evenement));
    assert!(kinds(&QuasiDetector, &standard).is_empty());
    assert!(kinds(&QuasiDetector, &renforce).contains(&EntityKind::Evenement));
    assert!(!kinds(&QuasiDetector, &renforce).contains(&EntityKind::Sante));
    let mut off = renforce.clone();
    off.disabled.insert(EntityKind::Sante);
    assert!(!kinds(&RuleDetector, &off).contains(&EntityKind::Sante));
    // Confiance suffisante pour être cochée d'office par le moteur (0,6).
    for d in RuleDetector.detect(&block(text), &standard) {
        assert!(d.confidence >= 0.6, "{d:?}");
    }
}

const FRAGMENTS: &[&str] = &[
    "atteint d'un ",
    "cancer",
    "en arrêt maladie",
    "condamné à ",
    "six mois de prison",
    "délégué syndical ",
    "CGT",
    "est ",
    "musulmane",
    "d'origine ",
    "maghrébine",
    "Œ",
    "œ",
    "ß",
    "É",
    "\u{a0}",
    "\u{2011}",
    "’",
    "\n",
    " ",
    ".",
    ";",
    "😀",
    "\u{0301}",
    "son ",
    "si ",
    "les salariés ",
];

proptest! {
    #[test]
    fn never_panics_on_any_string(s in "\\PC{0,300}") {
        let _ = found(&s);
    }

    #[test]
    fn never_panics_on_structured_text(parts in proptest::collection::vec(proptest::sample::select(FRAGMENTS.to_vec()), 0..25)) {
        let _ = found(&parts.concat());
        let _ = found(&parts.join(" "));
    }
}

/// Durée maximale de la détection sur environ 1 Mo de texte (compilation
/// optimisée), règles déjà compilées.
const MAX_DURATION_1MB: std::time::Duration = std::time::Duration::from_secs(1);

#[test]
fn a_megabyte_of_text_in_under_a_second() {
    let paragraph = "Réunion du comité : M. Martin, responsable technique, présente le budget ; \
        Mme Durand est en arrêt maladie depuis lundi et son dossier est suivi par le service. \
        Le diabète touche 10 % de la population, selon le rapport annuel de l'agence. \
        Les délégués ont rappelé que tout salarié en arrêt de travail doit prévenir son employeur.\n";
    let text = paragraph.repeat(1_000_000 / paragraph.len());
    let start = std::time::Instant::now();
    let _ = engine();
    let compile = start.elapsed();
    let start = std::time::Instant::now();
    let mut out = Vec::new();
    for line in text.lines() {
        detect(line, |_| true, &mut out);
    }
    let elapsed = start.elapsed();
    println!(
        "{} octets : {elapsed:?} ({} détections), compilation des règles {compile:?}",
        text.len(),
        out.len()
    );
    assert!(!out.is_empty());
    if !cfg!(debug_assertions) {
        assert!(elapsed < MAX_DURATION_1MB, "{elapsed:?}");
    }
}
