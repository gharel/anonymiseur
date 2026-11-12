use super::*;
use anon_core::detect::Profile;
use anon_core::text::{BlockContext, BlockRole};

fn block(text: &str) -> TextBlock {
    TextBlock {
        text: text.to_string(),
        ctx: BlockContext::new("test", BlockRole::Body),
    }
}

fn renforce() -> DetectOptions {
    DetectOptions {
        profile: Profile::Renforce,
        ..Default::default()
    }
}

fn found<'a>(text: &'a str, opts: &DetectOptions) -> Vec<(EntityKind, &'a str)> {
    DictionaryDetector::new()
        .detect(&block(text), opts)
        .iter()
        .map(|d| (d.kind, text.get(d.start..d.end).unwrap_or("<invalide>")))
        .collect()
}

fn std_found(text: &str) -> Vec<(EntityKind, &str)> {
    found(text, &DetectOptions::default())
}

#[test]
fn detector_identity() {
    let d = DictionaryDetector::new();
    assert_eq!(d.name(), "dictionary");
    let text = "Jean Dupont";
    let dets = d.detect(&block(text), &DetectOptions::default());
    assert_eq!(dets.len(), 1);
    assert_eq!(dets[0].source, Source::Dictionary);
    assert_eq!(dets[0].confidence, PERSON_CONFIDENCE);
}

#[test]
fn first_name_then_surname() {
    assert_eq!(
        std_found("Jean Dupont a signé le contrat."),
        [(EntityKind::Personne, "Jean Dupont")]
    );
    assert_eq!(
        std_found("Bonjour Marie Durand, merci."),
        [(EntityKind::Personne, "Marie Durand")]
    );
}

#[test]
fn compound_names_and_particles() {
    assert_eq!(
        std_found("Jean-Pierre Le Goff est venu."),
        [(EntityKind::Personne, "Jean-Pierre Le Goff")]
    );
    assert_eq!(
        std_found("Nous avons reçu Marie de la Fontaine hier."),
        [(EntityKind::Personne, "Marie de la Fontaine")]
    );
    assert_eq!(
        std_found("Il a écrit à Charles d'Artagnan hier."),
        [(EntityKind::Personne, "Charles d'Artagnan")]
    );
    assert_eq!(
        std_found("Contact : Pierre von Müller"),
        [(EntityKind::Personne, "Pierre von Müller")]
    );
    assert_eq!(
        std_found("Contact : Karim ben Salah"),
        [(EntityKind::Personne, "Karim ben Salah")]
    );
}

#[test]
fn capitalized_surname_after_first_name() {
    assert_eq!(
        std_found("Contact : Jean DUPONT"),
        [(EntityKind::Personne, "Jean DUPONT")]
    );
}

#[test]
fn surname_first_order() {
    assert_eq!(
        std_found("Contact : DUPONT Jean"),
        [(EntityKind::Personne, "DUPONT Jean")]
    );
    assert_eq!(
        std_found("Liste : Durand Marie, Lefebvre Paul"),
        [
            (EntityKind::Personne, "Durand Marie"),
            (EntityKind::Personne, "Lefebvre Paul")
        ]
    );
}

#[test]
fn two_people_in_a_row_are_split() {
    assert_eq!(
        std_found("Contact : Jean Dupont Marie Durand"),
        [
            (EntityKind::Personne, "Jean Dupont"),
            (EntityKind::Personne, "Marie Durand")
        ]
    );
}

#[test]
fn title_with_surname_only_gives_nom() {
    let t = "Mme Durand a appelé.";
    let dets = DictionaryDetector::new().detect(&block(t), &DetectOptions::default());
    assert_eq!(dets.len(), 1);
    assert_eq!(dets[0].kind, EntityKind::Nom);
    assert_eq!(dets[0].confidence, TITLE_NAME_CONFIDENCE);
    assert_eq!(t.get(dets[0].start..dets[0].end), Some("Durand"));
    assert_eq!(std_found("M. Dupont a dit"), [(EntityKind::Nom, "Dupont")]);
    assert_eq!(
        std_found("Voir Maître Dupont."),
        [(EntityKind::Nom, "Dupont")]
    );
}

#[test]
fn title_with_first_and_last_gives_personne() {
    assert_eq!(
        std_found("Le Dr Jean Martin est là."),
        [(EntityKind::Personne, "Jean Martin")]
    );
    assert_eq!(
        std_found("Voir Monsieur Pierre Dupont."),
        [(EntityKind::Personne, "Pierre Dupont")]
    );
    assert_eq!(
        std_found("Mme de Sévigné écrit."),
        [(EntityKind::Nom, "de Sévigné")]
    );
}

#[test]
fn title_works_for_names_absent_from_dictionaries() {
    assert_eq!(
        std_found("Voir Mlle Quenotte demain."),
        [(EntityKind::Nom, "Quenotte")]
    );
    assert_eq!(
        std_found("Voir Mlle Xylia Quenotte demain."),
        [(EntityKind::Nom, "Xylia Quenotte")]
    );
    assert_eq!(
        std_found("Voir Mr Zorbal Quenotte."),
        [(EntityKind::Nom, "Zorbal Quenotte")]
    );
}

#[test]
fn title_is_not_followed_by_common_words() {
    assert_eq!(std_found("Monsieur le maire est là."), []);
    assert_eq!(std_found("Voir Docteur en droit."), []);
}

#[test]
fn lone_surname_mid_sentence() {
    let t = "Le dossier de Dupont est clos.";
    let dets = DictionaryDetector::new().detect(&block(t), &DetectOptions::default());
    assert_eq!(dets.len(), 1);
    assert_eq!(dets[0].kind, EntityKind::Nom);
    assert_eq!(dets[0].confidence, SOLO_NAME_CONFIDENCE);
    assert_eq!(t.get(dets[0].start..dets[0].end), Some("Dupont"));
}

#[test]
fn lone_first_name_mid_sentence() {
    let t = "Elle a croisé Stéphanie hier.";
    let dets = DictionaryDetector::new().detect(&block(t), &DetectOptions::default());
    assert_eq!(dets.len(), 1);
    assert_eq!(dets[0].kind, EntityKind::Prenom);
    assert_eq!(dets[0].confidence, SOLO_NAME_CONFIDENCE);
    assert_eq!(t.get(dets[0].start..dets[0].end), Some("Stéphanie"));
}

#[test]
fn rare_lone_first_name_is_ignored() {
    // Palier 1 : trop rare pour être proposé seul.
    let rare = crate::first_name("Teiva");
    assert!(rare.is_some_and(|t| t < Tier::Common));
    assert_eq!(std_found("Elle a croisé Teiva hier."), []);
}

#[test]
fn initials_before_a_known_surname() {
    for text in ["J. Dupont", "J.-P. Martin", "J. P. Martin", "J.P. Martin"] {
        let line = format!("Le dossier sera repris par {text} demain.");
        let dets = DictionaryDetector::new().detect(&block(&line), &DetectOptions::default());
        assert_eq!(dets.len(), 1, "{text}");
        assert_eq!(dets[0].kind, EntityKind::Personne, "{text}");
        assert_eq!(line.get(dets[0].start..dets[0].end), Some(text), "{text}");
        assert_eq!(dets[0].confidence, PERSON_CONFIDENCE);
    }
    assert_eq!(
        std_found("Signé J. Dupont"),
        [(EntityKind::Personne, "J. Dupont")]
    );
}

#[test]
fn initials_before_unknown_surname_need_a_title() {
    assert_eq!(std_found("Repris par J. Quenotte demain."), []);
    assert_eq!(
        std_found("Voir Dr J. Quenotte demain."),
        [(EntityKind::Personne, "J. Quenotte")]
    );
    assert_eq!(
        std_found("Voir M. A.-L. Quenotte demain."),
        [(EntityKind::Personne, "A.-L. Quenotte")]
    );
    assert_eq!(std_found("Option A. Rose est fermée."), []);
}

#[test]
fn labels_are_never_lone_names() {
    // Mots connus comme noms, mais ici étiquettes ou débuts d'éléments.
    assert_eq!(std_found("Voir Dupont : le dossier."), []);
    assert_eq!(std_found("Voir Dupont: le dossier."), []);
    assert_eq!(std_found("Liste\n- Dupont\n* Durand\n• Lefebvre"), []);
    assert_eq!(std_found("Liste\n1. Dupont\n2) Durand"), []);
    assert_eq!(std_found("# Dupont\n## Durand"), []);
    assert_eq!(std_found("Voir : Dupont"), []);
    assert_eq!(std_found("Voir ; Dupont"), []);
    assert_eq!(std_found("| Rôle | Dupont | Contact |"), []);
}

#[test]
fn lone_names_need_a_frequency_tier_of_two() {
    // « Teiva » (rare) est ignoré seul ; un nom très courant est proposé.
    assert_eq!(std_found("Voir chez Teiva demain."), []);
    assert_eq!(
        std_found("Voir chez Dupont demain."),
        [(EntityKind::Nom, "Dupont")]
    );
}

#[test]
fn ordinary_lexicon_words_are_never_lone_names() {
    // Mots du lexique qui sont aussi des noms de famille ou des mots rares.
    for w in [
        "Carte",
        "Équipe",
        "Rôle",
        "Lancement",
        "Tél",
        "Brief",
        "España",
    ] {
        let text = format!("Voir la {w} demain.");
        assert_eq!(std_found(&text), [], "{w}");
    }
}

#[test]
fn administrative_prose_has_no_person_names() {
    let prose = "\
# Compte rendu de la réunion

Objet : Demande de renouvellement du dossier
Réf : 2026-0412
Date : le 3 décembre 2026
Dossier : Instruction en cours

Suite à votre courrier, le service instruit la demande. Pour toute question,
adressez-vous au bureau des affaires générales. Après examen, la commission
rendra son avis. Monsieur le Préfet signera l'arrêté. Cordialement.

- Pièces à fournir : justificatif de domicile, carte d'identité.
- Délai : deux mois.
* Contact : accueil du service.
1. Dépôt du dossier
2) Examen par la commission

## Prochaines étapes

Rappel : Le délai de recours est de deux mois. Ensuite, la décision devient
définitive. Sinon, une nouvelle demande peut être déposée. Enfin, le service
reste à votre disposition. Voir aussi la Carte nationale et la Préfecture.
Tableau récapitulatif : Rôle, Équipe, Lancement, Brief, Tél, Date, Montant.
";
    for opts in [DetectOptions::default(), renforce()] {
        let bad: Vec<_> = found(prose, &opts)
            .into_iter()
            .filter(|(k, _)| {
                matches!(
                    k,
                    EntityKind::Nom | EntityKind::Prenom | EntityKind::Personne
                )
            })
            .collect();
        assert_eq!(bad, [], "bruit sur de la prose");
    }
}

#[test]
fn capitalized_word_at_sentence_start_is_ignored() {
    assert_eq!(std_found("Dupont est absent."), []);
    assert_eq!(std_found("Il part. Dupont reste."), []);
    assert_eq!(std_found("Il part !\nDupont reste."), []);
    assert_eq!(std_found("« Dupont est absent »"), []);
    assert_eq!(std_found("Dupont"), []);
}

#[test]
fn common_words_are_traps_not_names() {
    assert_eq!(std_found("C'est la pierre angulaire du projet."), []);
    assert_eq!(std_found("La Rose des vents souffle."), []);
    assert_eq!(std_found("Il a vu la Rose des vents."), []);
    assert_eq!(std_found("Hier, Pierre est venu."), []);
    assert_eq!(std_found("Un joli Marin pêcheur."), []);
    assert_eq!(std_found("Un grand Blanc est apparu."), []);
}

#[test]
fn martinique_is_not_martin() {
    assert_eq!(std_found("Il est né en Martinique."), []);
    assert_eq!(found("Il est né en Martinique.", &renforce()), []);
    assert_eq!(std_found("Voir la Martinique et la Guadeloupe."), []);
}

#[test]
fn common_first_name_needs_known_surname() {
    assert_eq!(std_found("Voir Rose Quenotte."), []);
    assert_eq!(
        std_found("Contact : Rose Martin"),
        [(EntityKind::Personne, "Rose Martin")]
    );
}

#[test]
fn legal_forms() {
    assert_eq!(
        std_found("La SARL Dupont Frères a gagné."),
        [(EntityKind::Societe, "SARL Dupont Frères")]
    );
    assert_eq!(
        std_found("Contrat avec Acme Holding Ltd signé."),
        [(EntityKind::Societe, "Acme Holding Ltd")]
    );
    assert_eq!(
        std_found("Livraison à Garage Quenotte SAS demain."),
        [(EntityKind::Societe, "Garage Quenotte SAS")]
    );
    assert_eq!(
        std_found("Voir SCI Les Mimosas et le notaire."),
        [(EntityKind::Societe, "SCI Les Mimosas")]
    );
}

#[test]
fn organization_lead_words() {
    assert_eq!(
        std_found("Chez Société Générale Immobilière hier."),
        [(EntityKind::Societe, "Société Générale Immobilière")]
    );
    assert_eq!(
        std_found("Voir Cabinet Martin & Associés demain."),
        [(EntityKind::Societe, "Cabinet Martin & Associés")]
    );
    assert_eq!(
        std_found("Le Groupe Quenotte annonce."),
        [(EntityKind::Societe, "Groupe Quenotte")]
    );
    assert_eq!(
        std_found("Voir Ets. Quenotte et fils."),
        [(EntityKind::Societe, "Ets. Quenotte")]
    );
    assert_eq!(std_found("Le groupe de travail se réunit."), []);
    assert_eq!(std_found("Notre société est en croissance."), []);
}

#[test]
fn communes_need_lieu_to_be_wanted() {
    let t = "Il habite au Mont-Dore.";
    assert_eq!(std_found(t), []);
    assert_eq!(found(t, &renforce()), [(EntityKind::Lieu, "Mont-Dore")]);
    let mut opts = renforce();
    opts.disabled.insert(EntityKind::Lieu);
    assert_eq!(found(t, &opts), []);
}

#[test]
fn communes_multi_word_and_new_caledonia() {
    let r = renforce();
    assert_eq!(
        found("Elle vit à La Foa depuis longtemps.", &r),
        [(EntityKind::Lieu, "La Foa")]
    );
    assert_eq!(
        found("Courrier pour 98800 Nouméa.", &r),
        [(EntityKind::Lieu, "Nouméa")]
    );
    assert_eq!(
        found("Un séjour à Fort-de-France puis à Païta.", &r),
        [
            (EntityKind::Lieu, "Fort-de-France"),
            (EntityKind::Lieu, "Païta")
        ]
    );
    assert_eq!(
        found("Au Mont-Dore, puis L'Haÿ-les-Roses.", &r),
        [
            (EntityKind::Lieu, "Mont-Dore"),
            (EntityKind::Lieu, "L'Haÿ-les-Roses")
        ]
    );
    assert_eq!(
        found("Il est parti de Paris vers Lyon.", &r),
        [(EntityKind::Lieu, "Paris"), (EntityKind::Lieu, "Lyon")]
    );
}

#[test]
fn commune_confidence_and_person_together() {
    let t = "Marie Dupont habite à Paris.";
    let dets = DictionaryDetector::new().detect(&block(t), &renforce());
    assert_eq!(dets.len(), 2);
    assert_eq!(dets[0].kind, EntityKind::Personne);
    assert_eq!(dets[1].kind, EntityKind::Lieu);
    assert_eq!(dets[1].confidence, COMMUNE_CONFIDENCE);
}

#[test]
fn ambiguous_single_word_commune_needs_context() {
    let r = renforce();
    // `Marie` est une commune mais surtout un prénom : pas de lieu sans contexte.
    assert!(
        found("Il a vu Marie hier.", &r)
            .iter()
            .all(|(k, _)| *k != EntityKind::Lieu)
    );
    assert_eq!(found("Il vit à Marie.", &r), [(EntityKind::Lieu, "Marie")]);
}

#[test]
fn nothing_inside_emails_or_urls() {
    assert_eq!(std_found("Écrire à jean.dupont@example.com merci"), []);
    assert_eq!(std_found("Voir https://example.com/Jean-Dupont/Marie"), []);
    assert_eq!(std_found("Voir www.example.com/Marie-Durand"), []);
    assert_eq!(
        std_found("Jean Dupont (jean.dupont@example.com)"),
        [(EntityKind::Personne, "Jean Dupont")]
    );
    assert_eq!(
        found("Voir Jean.Dupont@example.com à Paris", &renforce()),
        [(EntityKind::Lieu, "Paris")]
    );
}

#[test]
fn all_caps_text_has_no_noise() {
    assert_eq!(std_found("BONJOUR JEAN DUPONT MARTIN DURAND"), []);
    assert_eq!(found("LE MONT-DORE ET NOUMEA", &renforce()), []);
    assert_eq!(
        std_found("MME DURAND ARRIVE DEMAIN"),
        [(EntityKind::Nom, "DURAND")]
    );
    assert_eq!(
        std_found("SARL DUPONT FRERES"),
        [(EntityKind::Societe, "SARL DUPONT FRERES")]
    );
    assert_eq!(std_found("SA FAMILLE EST LA"), []);
}

#[test]
fn empty_and_blank_text() {
    assert_eq!(std_found(""), []);
    assert_eq!(std_found("   \n\t "), []);
    assert_eq!(std_found("123 456 !?"), []);
}

#[test]
fn disabled_kinds_are_not_proposed() {
    let mut opts = DetectOptions::default();
    opts.disabled.insert(EntityKind::Personne);
    assert_eq!(found("Jean Dupont est là.", &opts), []);
    let mut opts = DetectOptions::default();
    opts.disabled.insert(EntityKind::Nom);
    // La civilité réserve l'intervalle : pas de détection de remplacement.
    assert_eq!(found("Voir Mme Durand.", &opts), []);
}

#[test]
fn exotic_unicode_never_panics_and_offsets_are_boundaries() {
    let samples = [
        "",
        "\u{0301}\u{0301}Jean\u{0301} Dupont\u{0301}",
        "😀 Jean 😀 Dupont 😀",
        "Jean\u{200b}Dupont Marie\u{a0}Durand",
        "Ｊｅａｎ Ｄｕｐｏｎｔ",
        "ǅemal Ǆurić à Nouméa",
        "İstanbul Éric Ünal",
        "日本語 東京 Jean 田中 太郎",
        "مرحبا Jean Dupont שלום",
        "Jean\0Dupont\0",
        "M.\u{a0}Dupont, Mme.\u{a0}Durand, Dr.Martin",
        "L'\u{2019}d'\u{2019}D\u{2019}Artagnan -Jean- 'Marie'",
        "SARL \u{a0}& SARL & & Ltd.",
        "Société Société Société Cabinet Groupe Ets",
        "a@b a@ @b http:// www. ://",
        "..!!?? « « « ( [ \"",
        "Fort-de-France-Fort-de-France La La La Le Le",
    ];
    for opts in [DetectOptions::default(), renforce()] {
        for text in samples {
            for d in DictionaryDetector::new().detect(&block(text), &opts) {
                assert!(d.start < d.end && d.end <= text.len(), "{text:?}");
                assert!(text.is_char_boundary(d.start), "{text:?}");
                assert!(text.is_char_boundary(d.end), "{text:?}");
            }
        }
    }
}

#[test]
fn detections_are_sorted_and_disjoint() {
    let t = "Jean Dupont, Mme Durand, la SARL Martin Frères, Marie Curie et Paul vivent à Paris près de Dupont.";
    let dets = DictionaryDetector::new().detect(&block(t), &renforce());
    assert!(dets.len() >= 4);
    for pair in dets.windows(2) {
        assert!(pair[0].end <= pair[1].start, "{pair:?}");
    }
}

#[test]
fn sentence_start_helper() {
    assert!(is_sentence_start("Dupont", 0));
    assert!(is_sentence_start("Oui. Dupont", 5));
    assert!(!is_sentence_start("Voir Dupont", 5));
    assert!(!is_sentence_start("M. Dupont", 3));
    assert!(!is_sentence_start("Nom : Dupont", 6));
    assert!(is_sentence_start("a\nDupont", 2));
    assert!(is_sentence_start("• Dupont", 4));
}
