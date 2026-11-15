//! Relecture d'une clé de restauration écrite par la V1.
//!
//! `fixtures/cle-v1.anonmap` a été produit par le code de la V1 (commit 976a0f0,
//! `FORMAT_VERSION = 1`, paramètres Argon2id de production), avec la session fictive
//! décrite ci-dessous. Ne jamais le régénérer : il représente les clés déjà
//! enregistrées chez les utilisateurs.

use anon_core::assess::{Goal, Recipient};
use anon_core::entities::{EntityKind, Source, Tag};
use anon_core::session::{Entity, FORMAT_VERSION, Session, Treatment};

/// Mot de passe fictif de la référence.
const PASSWORD: &str = "cle-v1-de-test-é";
const FIXTURE: &[u8] = include_bytes!("fixtures/cle-v1.anonmap");

/// La session V1 telle que la V2 doit la relire.
fn expected() -> Session {
    Session {
        format_version: 1,
        entities: vec![
            Entity {
                tag: Tag::new(EntityKind::Personne, 1),
                value: "Claire Fictive".to_string(),
                variants: vec!["FICTIVE Claire".to_string(), "C. Fictive".to_string()],
                case_sensitive: false,
                enabled: true,
                treatment: Treatment::Tag,
                source: Source::Ner,
            },
            Entity {
                tag: Tag::new(EntityKind::Email, 1),
                value: "claire.fictive@example.com".to_string(),
                variants: Vec::new(),
                case_sensitive: false,
                enabled: true,
                treatment: Treatment::Tag,
                source: Source::Rule,
            },
            Entity {
                tag: Tag::new(EntityKind::DateNaiss, 1),
                value: "05/06/1987".to_string(),
                variants: vec!["5 juin 1987".to_string()],
                case_sensitive: true,
                enabled: false,
                treatment: Treatment::Generalize("1987".to_string()),
                source: Source::Manual,
            },
        ],
        never: vec!["Bonjour".to_string()],
        media: Vec::new(),
        irreversible: false,
        goal: Goal::Mask,
        recipient: Recipient::PublicAi,
        created_on: None,
        keep_until: None,
    }
}

#[test]
fn une_cle_v1_se_relit_avec_la_v2() {
    const { assert!(FORMAT_VERSION >= 2) };
    let session = anon_vault::open(FIXTURE, PASSWORD).expect("clé V1 refusée");
    assert_eq!(session.format_version, 1);
    // Champs de la V2 absents du fichier : valeurs par défaut.
    assert_eq!(session.goal, Goal::Mask);
    assert_eq!(session.recipient, Recipient::default());
    assert_eq!(session.created_on, None);
    assert_eq!(session.keep_until, None);
    // Entités intactes, valeurs comprises (`Debug` ne les montre pas : on compare).
    assert_eq!(session.entities.len(), 3);
    assert_eq!(session, expected());
    let email = session.entity(Tag::new(EntityKind::Email, 1));
    assert_eq!(
        email.map(|e| e.value.as_str()),
        Some("claire.fictive@example.com")
    );
}

/// Reprise d'une table existante : la clé V1 rescellée passe au format courant,
/// sans rien perdre.
#[test]
fn une_cle_v1_rescellee_passe_au_format_courant() {
    let session = anon_vault::open(FIXTURE, PASSWORD).expect("clé V1 refusée");
    let resealed = anon_vault::seal(&session, PASSWORD).expect("scellement");
    let reopened = anon_vault::open(&resealed, PASSWORD).expect("relecture");
    assert_eq!(reopened.format_version, FORMAT_VERSION);
    assert_eq!(
        reopened,
        Session {
            format_version: FORMAT_VERSION,
            ..expected()
        }
    );
}

#[test]
fn une_cle_v1_refuse_un_mauvais_mot_de_passe() {
    assert_eq!(
        anon_vault::open(FIXTURE, "autre mot de passe"),
        Err(anon_vault::VaultError::WrongPassword)
    );
}
