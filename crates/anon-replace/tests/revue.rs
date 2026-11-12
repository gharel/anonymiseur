//! Cas pièges écrits lors de la revue du moteur, indépendamment de son auteur.
//! Ils servent de garde-fou contre les régressions.

use anon_core::{
    BlockContext, BlockRole, EntityKind, Session, Source, Tag, TextTransform, Treatment,
};
use anon_replace::{Replacer, Restorer, SessionExt};

fn ctx() -> BlockContext {
    BlockContext::new("test", BlockRole::Body)
}

fn anonymise(s: &Session, text: &str) -> String {
    Replacer::new(s).apply(text, &ctx()).into_owned()
}

fn restaure(s: &Session, text: &str) -> String {
    Restorer::new(s).apply(text, &ctx()).into_owned()
}

fn session_type() -> Session {
    let mut s = Session::new();
    s.intern(EntityKind::Personne, "Jean Dupont", Source::Manual);
    s.intern(EntityKind::Nom, "Dupont", Source::Manual);
    s.intern(EntityKind::Prenom, "Jean", Source::Manual);
    s.intern(EntityKind::Personne, "Hélène Kaloï", Source::Manual);
    s.intern(EntityKind::Nom, "Martin", Source::Manual);
    s.intern(EntityKind::Email, "jean.dupont@example.com", Source::Rule);
    s.intern(EntityKind::Tel, "06 12 34 56 78", Source::Rule);
    s.intern(
        EntityKind::Societe,
        "Atelier Bougainville SAS",
        Source::Manual,
    );
    s
}

#[test]
fn le_plus_long_gagne_et_les_parties_partagent_l_indice() {
    let s = session_type();
    assert_eq!(
        anonymise(&s, "Jean Dupont a signé. M. Dupont confirme, Jean aussi."),
        "[PERSONNE_1] a signé. M. [NOM_1] confirme, [PRENOM_1] aussi."
    );
}

#[test]
fn pas_de_remplacement_a_l_interieur_d_un_mot() {
    let s = session_type();
    for texte in [
        "La Martinique",
        "Martinez",
        "Dupontel",
        "xMartin",
        "Martin2",
        "2Martin",
    ] {
        assert_eq!(anonymise(&s, texte), texte, "{texte}");
    }
}

#[test]
fn apostrophes_tirets_et_ponctuation_sont_des_frontieres() {
    let s = session_type();
    // Personnes, noms et prénoms partagent un compteur : Martin est la 3ᵉ personne.
    assert_eq!(anonymise(&s, "l'avis de Martin."), "l'avis de [NOM_3].");
    assert_eq!(anonymise(&s, "d’Hélène Kaloï,"), "d’[PERSONNE_2],");
    assert_eq!(anonymise(&s, "(Martin)"), "([NOM_3])");
    assert_eq!(anonymise(&s, "Martin-Dupont"), "[NOM_3]-[NOM_1]");
    assert_eq!(
        anonymise(&s, "«\u{a0}Martin\u{a0}»"),
        "«\u{a0}[NOM_3]\u{a0}»"
    );
}

#[test]
fn casse_accents_et_espaces_insecables_sont_ignores() {
    let s = session_type();
    assert_eq!(anonymise(&s, "HELENE KALOI"), "[PERSONNE_2]");
    assert_eq!(anonymise(&s, "hélène\u{a0}kaloï"), "[PERSONNE_2]");
    // Forme décomposée (NFD) : e + accent combinant.
    assert_eq!(
        anonymise(&s, "He\u{301}le\u{300}ne Kaloi\u{308}"),
        "[PERSONNE_2]"
    );
    assert_eq!(anonymise(&s, "Jean\n  Dupont"), "[PERSONNE_1]");
}

#[test]
fn telephone_quels_que_soient_les_separateurs() {
    let s = session_type();
    for forme in [
        "0612345678",
        "06.12.34.56.78",
        "06-12-34-56-78",
        "06\u{a0}12\u{a0}34\u{a0}56\u{a0}78",
        "06 12 34 56 78",
    ] {
        assert_eq!(
            anonymise(&s, &format!("Tél : {forme}.")),
            "Tél : [TEL_1].",
            "{forme}"
        );
    }
    // Un numéro plus long qui contient le même préfixe n'est pas touché.
    assert_eq!(anonymise(&s, "06123456789"), "06123456789");
    assert_eq!(anonymise(&s, "106 12 34 56 78"), "106 12 34 56 78");
}

#[test]
fn email_entre_chevrons_et_dans_un_lien() {
    let s = session_type();
    assert_eq!(anonymise(&s, "<jean.dupont@example.com>"), "<[EMAIL_1]>");
    assert_eq!(anonymise(&s, "JEAN.DUPONT@EXAMPLE.COM"), "[EMAIL_1]");
    let r = Replacer::new(&s);
    assert_eq!(
        r.apply_uri("mailto:jean.dupont@example.com?subject=Bonjour"),
        "mailto:EMAIL_1@anon.invalid?subject=Bonjour"
    );
    assert_eq!(
        r.apply_uri("https://example.org/aide"),
        "https://example.org/aide"
    );
}

#[test]
fn une_balise_existante_n_est_jamais_touchee() {
    let mut s = session_type();
    // Valeurs volontairement tordues : elles ressemblent à un morceau de balise.
    s.intern(EntityKind::Autre, "PERSONNE", Source::Manual);
    s.intern(EntityKind::Autre, "NOM_1", Source::Manual);
    let texte = "Voir [PERSONNE_1] et [NOM_1].";
    assert_eq!(anonymise(&s, texte), texte);
    // Idempotence : anonymiser deux fois ne change plus rien.
    let une = anonymise(&s, "Jean Dupont écrit à Martin (06 12 34 56 78).");
    assert_eq!(anonymise(&s, &une), une);
}

#[test]
fn valeur_avec_caracteres_speciaux() {
    let mut s = Session::new();
    s.intern(EntityKind::Societe, "C++ & Co. (SARL)", Source::Manual);
    s.intern(EntityKind::Autre, "a.b*c", Source::Manual);
    assert_eq!(
        anonymise(&s, "Client : C++ & Co. (SARL)."),
        "Client : [SOCIETE_1]."
    );
    assert_eq!(anonymise(&s, "code a.b*c ici"), "code [AUTRE_1] ici");
    assert_eq!(anonymise(&s, "code axb*c ici"), "code axb*c ici");
}

#[test]
fn valeur_courte_sensible_a_la_casse() {
    let mut s = Session::new();
    let tag = s.intern(EntityKind::Prenom, "Léa", Source::Manual);
    assert!(tag.is_some());
    assert_eq!(anonymise(&s, "Léa arrive"), "[PRENOM_1] arrive");
    assert_eq!(anonymise(&s, "la lea et LEA"), "la lea et LEA");

    let mut s = Session::new();
    let Some(tag) = s.intern(EntityKind::Prenom, "Pierre", Source::Manual) else {
        panic!("interne refusé");
    };
    s.set_case_sensitive(tag, true);
    assert_eq!(
        anonymise(&s, "Pierre pose la pierre angulaire"),
        "[PRENOM_1] pose la pierre angulaire"
    );
}

#[test]
fn entite_desactivee_liste_jamais_et_generalisation() {
    let mut s = session_type();
    let tag = Tag::new(EntityKind::Nom, 3);
    assert!(s.set_enabled(tag, false));
    assert_eq!(anonymise(&s, "Martin"), "Martin");

    let mut s = Session::new();
    s.add_never("Nouméa");
    assert_eq!(
        s.intern(EntityKind::Lieu, "nouméa", Source::Dictionary),
        None
    );

    let mut s = Session::new();
    let Some(tag) = s.intern(EntityKind::Date, "12 mars 1984", Source::Rule) else {
        panic!("interne refusé");
    };
    s.set_treatment(tag, Treatment::Generalize("1984".into()));
    assert_eq!(anonymise(&s, "né le 12 mars 1984"), "né le 1984");
    // Une valeur généralisée est irréversible : la restauration n'invente rien.
    assert_eq!(restaure(&s, "né le 1984"), "né le 1984");
}

#[test]
fn aller_retour_sur_un_texte_realiste() {
    let s = session_type();
    let texte = "Objet : contrat\nJean Dupont (jean.dupont@example.com, 06 12 34 56 78) représente \
                 Atelier Bougainville SAS. Hélène Kaloï et M. Martin valident ; Jean signe.";
    let anon = anonymise(&s, texte);
    for valeur in [
        "Dupont",
        "Jean",
        "Kaloï",
        "Martin",
        "example.com",
        "06 12",
        "Bougainville",
    ] {
        assert!(!anon.contains(valeur), "fuite de {valeur} : {anon}");
    }
    assert_eq!(restaure(&s, &anon), texte);
}

#[test]
fn restauration_des_formes_alterees_par_une_ia() {
    let s = session_type();
    assert_eq!(
        restaure(
            &s,
            "**[PERSONNE_1]**, [Personne 2], [personne_1], [PERSONNE\\_2], \\[NOM\\_3\\], [SOCIÉTÉ_1], 【TEL_1】"
        ),
        "**Jean Dupont**, Hélène Kaloï, Jean Dupont, Hélène Kaloï, Martin, Atelier Bougainville SAS, 06 12 34 56 78"
    );
    let r = Restorer::new(&s);
    let sortie = r.apply("[PERSONNE_99] et [EMAIL_1]", &ctx()).into_owned();
    assert_eq!(sortie, "[PERSONNE_99] et jean.dupont@example.com");
    let rapport = r.report();
    assert_eq!(rapport.unknown, vec!["[PERSONNE_99]".to_string()]);
    assert!(rapport.unused.contains(&Tag::new(EntityKind::Tel, 1)));
    assert!(!rapport.unused.contains(&Tag::new(EntityKind::Email, 1)));
}

#[test]
fn texte_ordinaire_entre_crochets_n_est_pas_une_balise() {
    let s = session_type();
    for texte in [
        "[1]",
        "[voir annexe 2]",
        "[TODO]",
        "tableau[3]",
        "[PERSONNE]",
        "[PERSONNE_]",
        "[INCONNU_1]",
    ] {
        assert_eq!(restaure(&s, texte), texte, "{texte}");
    }
}

#[test]
fn unicode_exotique_sans_panique() {
    let s = session_type();
    let texte =
        "👩\u{200d}💻 Jean Dupont 日本語 Мартин \u{202e}Martin\u{202c} \u{200b}Dupont\u{feff} 🇳🇨";
    let anon = anonymise(&s, texte);
    assert!(anon.contains("[PERSONNE_1]"));
    assert!(
        anon.contains("Мартин"),
        "le cyrillique ne doit pas être confondu avec Martin"
    );
    for r in Replacer::new(&s).replacements(texte, &ctx()) {
        assert!(texte.is_char_boundary(r.start) && texte.is_char_boundary(r.end));
    }
    for r in Restorer::new(&s).replacements(&anon, &ctx()) {
        assert!(anon.is_char_boundary(r.start) && anon.is_char_boundary(r.end));
    }
    let _ = restaure(&s, &anon);
}
