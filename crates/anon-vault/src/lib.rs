//! Fichier `.anonmap` : la table de correspondance d'une session, chiffrée par mot de passe.
//!
//! Disposition (entiers en petit-boutiste) :
//!
//! | Octets | Contenu |
//! |---|---|
//! | 0..8 | magie `ANONMAP\0` |
//! | 8 | version du conteneur (`u8`, 1) |
//! | 9..13 | mémoire Argon2id en Kio (`u32`) |
//! | 13..17 | itérations Argon2id (`u32`) |
//! | 17 | parallélisme Argon2id (`u8`) |
//! | 18..34 | sel (16 octets) |
//! | 34..46 | nonce AES-GCM (12 octets) |
//! | 46.. | AES-256-GCM du CBOR de la `Session` (étiquette de 16 octets en fin) |
//!
//! L'en-tête entier (46 octets) sert de données associées : toute altération
//! fait échouer le déchiffrement. Aucun contenu utilisateur n'apparaît dans les erreurs.

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use anon_core::session::{FORMAT_VERSION, Session};
use argon2::{Algorithm, Argon2, Params, Version};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

/// Magie en tête de fichier.
const MAGIC: &[u8; 8] = b"ANONMAP\0";
/// Version du conteneur écrite par ce code.
const CONTAINER_VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
/// Taille de l'en-tête en clair.
const HEADER_LEN: usize = MAGIC.len() + 1 + 4 + 4 + 1 + SALT_LEN + NONCE_LEN;

/// Paramètres Argon2id de production.
const ARGON_MEMORY_KIB: u32 = 64 * 1024;
const ARGON_ITERATIONS: u32 = 3;
const ARGON_PARALLELISM: u8 = 1;
/// Bornes acceptées à la lecture (protège contre un fichier piégé).
const ARGON_MAX_MEMORY_KIB: u32 = 1024 * 1024;
const ARGON_MAX_ITERATIONS: u32 = 20;
const ARGON_MAX_PARALLELISM: u8 = 16;

/// Seuils de l'heuristique de robustesse (en caractères).
const STRENGTH_MIN_LEN: usize = 8;
const STRENGTH_CORRECT_LEN: usize = 10;
const STRENGTH_STRONG_LEN: usize = 14;
const STRENGTH_PASSPHRASE_LEN: usize = 24;
const STRENGTH_CORRECT_CLASSES: usize = 2;
const STRENGTH_STRONG_CLASSES: usize = 3;
const STRENGTH_MIN_DISTINCT: usize = 5;

/// Erreurs du coffre. Aucun message ne contient de contenu utilisateur.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum VaultError {
    #[error(
        "Mot de passe incorrect, ou fichier altéré : les deux cas sont indiscernables par construction."
    )]
    WrongPassword,
    #[error("Ce fichier n'est pas une table de correspondance Anonymiseur.")]
    NotAVault,
    #[error("Version de fichier non prise en charge ({0}).")]
    UnsupportedVersion(u32),
    #[error("Le fichier est corrompu ou tronqué.")]
    Corrupt,
    #[error("Une session en mode irréversible ne peut pas être enregistrée.")]
    Irreversible,
    #[error("Erreur cryptographique interne.")]
    Crypto,
}

/// Robustesse estimée d'un mot de passe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PasswordStrength {
    TropCourt,
    Faible,
    Correct,
    Fort,
}

/// Paramètres Argon2id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KdfParams {
    memory_kib: u32,
    iterations: u32,
    parallelism: u8,
}

impl KdfParams {
    const PRODUCTION: Self = Self {
        memory_kib: ARGON_MEMORY_KIB,
        iterations: ARGON_ITERATIONS,
        parallelism: ARGON_PARALLELISM,
    };

    /// Refuse les valeurs absurdes ou invalides pour Argon2.
    fn validate(self) -> Result<Params, VaultError> {
        if self.memory_kib > ARGON_MAX_MEMORY_KIB
            || self.iterations > ARGON_MAX_ITERATIONS
            || self.parallelism > ARGON_MAX_PARALLELISM
        {
            return Err(VaultError::Corrupt);
        }
        Params::new(
            self.memory_kib,
            self.iterations,
            u32::from(self.parallelism),
            Some(KEY_LEN),
        )
        .map_err(|_| VaultError::Corrupt)
    }
}

/// Estime la robustesse d'un mot de passe (heuristique simple : longueur, variété).
pub fn password_strength(password: &str) -> PasswordStrength {
    let len = password.chars().count();
    if len < STRENGTH_MIN_LEN {
        return PasswordStrength::TropCourt;
    }
    let mut distinct: Vec<char> = password.chars().collect();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() < STRENGTH_MIN_DISTINCT {
        return PasswordStrength::Faible;
    }
    let classes = [
        password.chars().any(char::is_lowercase),
        password.chars().any(char::is_uppercase),
        password.chars().any(char::is_numeric),
        password.chars().any(|c| !c.is_alphanumeric()),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    if len >= STRENGTH_PASSPHRASE_LEN
        || (len >= STRENGTH_STRONG_LEN && classes >= STRENGTH_STRONG_CLASSES)
    {
        PasswordStrength::Fort
    } else if len >= STRENGTH_CORRECT_LEN && classes >= STRENGTH_CORRECT_CLASSES {
        PasswordStrength::Correct
    } else {
        PasswordStrength::Faible
    }
}

/// Scelle la session dans un fichier `.anonmap` (sel et nonce aléatoires).
pub fn seal(session: &Session, password: &str) -> Result<Vec<u8>, VaultError> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut salt).map_err(|_| VaultError::Crypto)?;
    getrandom::fill(&mut nonce).map_err(|_| VaultError::Crypto)?;
    seal_params(session, password, KdfParams::PRODUCTION, salt, nonce)
}

/// Ouvre un fichier `.anonmap`.
pub fn open(bytes: &[u8], password: &str) -> Result<Session, VaultError> {
    let (params, salt, nonce) = parse_header(bytes)?;
    let argon_params = params.validate()?;
    let header = bytes.get(..HEADER_LEN).ok_or(VaultError::Corrupt)?;
    let body = bytes.get(HEADER_LEN..).ok_or(VaultError::Corrupt)?;

    let key = derive_key(password, &salt, argon_params)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_slice()).map_err(|_| VaultError::Crypto)?;
    drop(key);
    let plain = cipher
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: body,
                aad: header,
            },
        )
        .map_err(|_| VaultError::WrongPassword)?;
    let plain = Zeroizing::new(plain);
    let session: Session =
        ciborium::from_reader(plain.as_slice()).map_err(|_| VaultError::Corrupt)?;
    drop(plain);
    if session.format_version > FORMAT_VERSION {
        return Err(VaultError::UnsupportedVersion(session.format_version));
    }
    Ok(session)
}

/// Variante déterministe (sel et nonce fournis), paramètres de production.
#[cfg(test)]
fn seal_with(
    session: &Session,
    password: &str,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
) -> Result<Vec<u8>, VaultError> {
    seal_params(session, password, KdfParams::PRODUCTION, salt, nonce)
}

fn seal_params(
    session: &Session,
    password: &str,
    params: KdfParams,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
) -> Result<Vec<u8>, VaultError> {
    if session.irreversible {
        return Err(VaultError::Irreversible);
    }
    let argon_params = params.validate()?;

    let mut out = Vec::with_capacity(HEADER_LEN + 1024);
    out.extend_from_slice(MAGIC);
    out.push(CONTAINER_VERSION);
    out.extend_from_slice(&params.memory_kib.to_le_bytes());
    out.extend_from_slice(&params.iterations.to_le_bytes());
    out.push(params.parallelism);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);

    let mut plain = Zeroizing::new(Vec::new());
    ciborium::into_writer(session, &mut *plain).map_err(|_| VaultError::Crypto)?;

    let key = derive_key(password, &salt, argon_params)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_slice()).map_err(|_| VaultError::Crypto)?;
    drop(key);
    let body = cipher
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: plain.as_slice(),
                aad: &out,
            },
        )
        .map_err(|_| VaultError::Crypto)?;
    drop(plain);
    out.extend_from_slice(&body);
    Ok(out)
}

fn derive_key(
    password: &str,
    salt: &[u8; SALT_LEN],
    params: Params,
) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError> {
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    if argon
        .hash_password_into(password.as_bytes(), salt, key.as_mut_slice())
        .is_err()
    {
        key.zeroize();
        return Err(VaultError::Crypto);
    }
    Ok(key)
}

fn parse_header(bytes: &[u8]) -> Result<(KdfParams, [u8; SALT_LEN], [u8; NONCE_LEN]), VaultError> {
    if bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
        return Err(VaultError::NotAVault);
    }
    let version = *bytes.get(MAGIC.len()).ok_or(VaultError::Corrupt)?;
    if version != CONTAINER_VERSION {
        return Err(VaultError::UnsupportedVersion(u32::from(version)));
    }
    if bytes.len() < HEADER_LEN + TAG_LEN {
        return Err(VaultError::Corrupt);
    }
    let mut pos = MAGIC.len() + 1;
    let memory_kib = u32::from_le_bytes(take(bytes, &mut pos)?);
    let iterations = u32::from_le_bytes(take(bytes, &mut pos)?);
    let [parallelism] = take::<1>(bytes, &mut pos)?;
    let salt = take::<SALT_LEN>(bytes, &mut pos)?;
    let nonce = take::<NONCE_LEN>(bytes, &mut pos)?;
    Ok((
        KdfParams {
            memory_kib,
            iterations,
            parallelism,
        },
        salt,
        nonce,
    ))
}

fn take<const N: usize>(bytes: &[u8], pos: &mut usize) -> Result<[u8; N], VaultError> {
    let end = pos.checked_add(N).ok_or(VaultError::Corrupt)?;
    let arr = bytes
        .get(*pos..end)
        .and_then(|s| <[u8; N]>::try_from(s).ok())
        .ok_or(VaultError::Corrupt)?;
    *pos = end;
    Ok(arr)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use anon_core::entities::{EntityKind, Source, Tag};
    use anon_core::session::{Entity, MediaOriginal, Treatment};

    /// Paramètres réduits pour les tests rapides.
    const FAST: KdfParams = KdfParams {
        memory_kib: 16,
        iterations: 1,
        parallelism: 1,
    };
    const PASSWORD: &str = "mot de passe d'essai é";

    fn entity(kind: EntityKind, index: u32, value: &str, variants: &[&str]) -> Entity {
        Entity {
            tag: Tag::new(kind, index),
            value: value.to_string(),
            variants: variants.iter().map(|s| s.to_string()).collect(),
            case_sensitive: false,
            enabled: true,
            treatment: Treatment::Tag,
            source: Source::Rule,
        }
    }

    fn rich_session() -> Session {
        let mut disabled = entity(EntityKind::Lieu, 1, "Nouméa", &[]);
        disabled.enabled = false;
        disabled.source = Source::Manual;
        let mut generalized = entity(EntityKind::DateNaiss, 1, "12/03/1984", &["12 mars 1984"]);
        generalized.treatment = Treatment::Generalize("1984".to_string());
        generalized.source = Source::Column;
        let mut cs = entity(EntityKind::Prenom, 1, "Pierre", &["PIERRE"]);
        cs.case_sensitive = true;
        cs.source = Source::Dictionary;
        let original: Vec<u8> = (0..4096u32).map(|i| (i * 31 % 251) as u8).collect();
        Session {
            format_version: FORMAT_VERSION,
            entities: vec![
                entity(
                    EntityKind::Personne,
                    1,
                    "Jean Dupont",
                    &["DUPONT Jean", "J. Dupont"],
                ),
                entity(EntityKind::Email, 1, "jean.dupont@example.com", &[]),
                entity(
                    EntityKind::Societe,
                    1,
                    "Société Générale d'Électricité",
                    &[],
                ),
                disabled,
                generalized,
                cs,
            ],
            never: vec!["Bonjour".to_string(), "Hélène".to_string()],
            media: vec![MediaOriginal {
                masked_sha256: "ab".repeat(32),
                original,
            }],
            irreversible: false,
        }
    }

    fn seal_fast(session: &Session) -> Vec<u8> {
        seal_params(session, PASSWORD, FAST, [7; SALT_LEN], [9; NONCE_LEN]).unwrap()
    }

    #[test]
    fn round_trip_rich_session() {
        let s = rich_session();
        let bytes = seal_fast(&s);
        assert_eq!(open(&bytes, PASSWORD).unwrap(), s);
    }

    #[test]
    fn round_trip_production_params() {
        let s = rich_session();
        let bytes = seal(&s, PASSWORD).unwrap();
        assert_eq!(&bytes[9..13], &ARGON_MEMORY_KIB.to_le_bytes());
        assert_eq!(open(&bytes, PASSWORD).unwrap(), s);
        assert_eq!(open(&bytes, "autre"), Err(VaultError::WrongPassword));
    }

    #[test]
    fn wrong_password() {
        let bytes = seal_fast(&rich_session());
        assert_eq!(open(&bytes, "faux"), Err(VaultError::WrongPassword));
        assert_eq!(open(&bytes, ""), Err(VaultError::WrongPassword));
    }

    #[test]
    fn irreversible_refused() {
        let s = Session {
            irreversible: true,
            ..Session::new()
        };
        assert_eq!(seal(&s, PASSWORD), Err(VaultError::Irreversible));
    }

    #[test]
    fn two_seals_differ() {
        let s = Session::new();
        let a = seal(&s, PASSWORD).unwrap();
        let b = seal(&s, PASSWORD).unwrap();
        assert_ne!(a, b);
        assert_ne!(a[18..46], b[18..46]);
    }

    #[test]
    fn bit_flips_everywhere_are_rejected() {
        let bytes = seal_fast(&rich_session());
        for byte in 0..bytes.len() {
            for bit in 0..8 {
                let mut m = bytes.clone();
                m[byte] ^= 1 << bit;
                let err = open(&m, PASSWORD).expect_err("altération acceptée");
                let ok = match byte {
                    0..=7 => err == VaultError::NotAVault,
                    8 => matches!(err, VaultError::UnsupportedVersion(_)),
                    // Paramètres : hors bornes (Corrupt) ou clé différente.
                    9..=17 => matches!(err, VaultError::Corrupt | VaultError::WrongPassword),
                    // Sel, nonce, corps, étiquette.
                    _ => err == VaultError::WrongPassword,
                };
                assert!(ok, "octet {byte} bit {bit} : {err:?}");
            }
        }
    }

    #[test]
    fn truncation_never_panics() {
        let bytes = seal_fast(&rich_session());
        for len in 0..=HEADER_LEN + 1 {
            let err = open(&bytes[..len], PASSWORD).expect_err("tronqué accepté");
            if len < MAGIC.len() {
                assert_eq!(err, VaultError::NotAVault, "longueur {len}");
            } else {
                assert_eq!(err, VaultError::Corrupt, "longueur {len}");
            }
        }
        // Corps tronqué plus loin : l'authentification échoue.
        let err = open(&bytes[..bytes.len() - 1], PASSWORD).unwrap_err();
        assert_eq!(err, VaultError::WrongPassword);
    }

    #[test]
    fn arbitrary_files_rejected() {
        assert_eq!(open(b"", PASSWORD), Err(VaultError::NotAVault));
        assert_eq!(
            open(b"%PDF-1.7 contenu quelconque", PASSWORD),
            Err(VaultError::NotAVault)
        );
        assert_eq!(open(&[0u8; 200], PASSWORD), Err(VaultError::NotAVault));
        let mut fake = MAGIC.to_vec();
        fake.extend_from_slice(&[0xFF; 100]);
        assert_eq!(
            open(&fake, PASSWORD),
            Err(VaultError::UnsupportedVersion(255))
        );
    }

    #[test]
    fn out_of_range_params_rejected_before_derivation() {
        let bytes = seal_fast(&Session::new());
        let patch = |range: std::ops::Range<usize>, value: &[u8]| {
            let mut m = bytes.clone();
            m[range].copy_from_slice(value);
            m
        };
        let cases = [
            patch(9..13, &(ARGON_MAX_MEMORY_KIB + 1).to_le_bytes()),
            patch(13..17, &(ARGON_MAX_ITERATIONS + 1).to_le_bytes()),
            patch(17..18, &[ARGON_MAX_PARALLELISM + 1]),
            patch(13..17, &0u32.to_le_bytes()),
            patch(9..13, &1u32.to_le_bytes()),
            patch(17..18, &[0]),
        ];
        for m in cases {
            assert_eq!(open(&m, PASSWORD), Err(VaultError::Corrupt));
        }
        let max_ok = KdfParams {
            memory_kib: ARGON_MAX_MEMORY_KIB,
            iterations: ARGON_MAX_ITERATIONS,
            parallelism: 1,
        };
        assert!(max_ok.validate().is_ok());
    }

    #[test]
    fn newer_session_format_refused() {
        let s = Session {
            format_version: FORMAT_VERSION + 1,
            ..Session::new()
        };
        let bytes = seal_fast(&s);
        assert_eq!(
            open(&bytes, PASSWORD),
            Err(VaultError::UnsupportedVersion(FORMAT_VERSION + 1))
        );
    }

    #[test]
    fn valid_envelope_with_garbage_payload_is_corrupt() {
        // Enveloppe authentique contenant un CBOR qui n'est pas une Session.
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.push(CONTAINER_VERSION);
        out.extend_from_slice(&FAST.memory_kib.to_le_bytes());
        out.extend_from_slice(&FAST.iterations.to_le_bytes());
        out.push(FAST.parallelism);
        out.extend_from_slice(&[1; SALT_LEN]);
        out.extend_from_slice(&[2; NONCE_LEN]);
        let key = derive_key(PASSWORD, &[1; SALT_LEN], FAST.validate().unwrap()).unwrap();
        let cipher = Aes256Gcm::new_from_slice(key.as_slice()).unwrap();
        let body = cipher
            .encrypt(
                &Nonce::from([2u8; NONCE_LEN]),
                Payload {
                    msg: b"pas du cbor de session",
                    aad: &out,
                },
            )
            .unwrap();
        out.extend_from_slice(&body);
        assert_eq!(open(&out, PASSWORD), Err(VaultError::Corrupt));
    }

    #[test]
    fn wrong_password_message_mentions_tampering() {
        assert!(VaultError::WrongPassword.to_string().contains("altéré"));
    }

    #[test]
    fn strength_levels() {
        use PasswordStrength::*;
        assert_eq!(password_strength(""), TropCourt);
        assert_eq!(password_strength("abc123X"), TropCourt);
        assert_eq!(password_strength("aaaaaaaaaaaa"), Faible);
        assert_eq!(password_strength("motdepasse"), Faible);
        assert_eq!(password_strength("motdepasse1"), Correct);
        assert_eq!(password_strength("Motdepasse-2024!"), Fort);
        assert_eq!(password_strength("un long mot de passe en phrase"), Fort);
        assert!(Faible < Correct && Correct < Fort);
    }

    // --- Stabilité du format --------------------------------------------------

    /// Session fictive de référence pour `tests/fixtures/v1.anonmap`.
    fn fixture_session() -> Session {
        let mut generalized = entity(EntityKind::DateNaiss, 1, "03/04/1990", &[]);
        generalized.treatment = Treatment::Generalize("1990".to_string());
        let mut off = entity(EntityKind::Lieu, 1, "Dumbéa", &[]);
        off.enabled = false;
        Session {
            format_version: 1,
            entities: vec![
                entity(
                    EntityKind::Personne,
                    1,
                    "Marie Exemple",
                    &["EXEMPLE Marie", "M. Exemple"],
                ),
                entity(EntityKind::Email, 1, "marie.exemple@example.com", &[]),
                generalized,
                off,
            ],
            never: vec!["Bonjour".to_string()],
            media: vec![MediaOriginal {
                masked_sha256: "0f".repeat(32),
                original: vec![1, 2, 3, 4, 5, 250, 251, 252],
            }],
            irreversible: false,
        }
    }

    /// Mot de passe, sel et nonce fictifs de la référence.
    const FIXTURE_PASSWORD: &str = "correct-horse-é-battery-staple";
    const FIXTURE_SALT: [u8; SALT_LEN] = *b"fixture-salt-v1!";
    const FIXTURE_NONCE: [u8; NONCE_LEN] = *b"fixture-n-v1";
    const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/v1.anonmap");

    /// Protège la relecture des anciens fichiers : si ce test échoue, le format a
    /// changé. Ne pas régénérer la référence ; incrémenter la version et en ajouter une.
    /// Création initiale : `ANON_VAULT_WRITE_FIXTURE=1 cargo test -p anon-vault v1_fixture`.
    #[test]
    fn v1_fixture_is_stable_and_readable() {
        let produced = seal_with(
            &fixture_session(),
            FIXTURE_PASSWORD,
            FIXTURE_SALT,
            FIXTURE_NONCE,
        )
        .unwrap();
        if std::env::var_os("ANON_VAULT_WRITE_FIXTURE").is_some() {
            std::fs::write(FIXTURE_PATH, &produced).unwrap();
        }
        let reference = std::fs::read(FIXTURE_PATH).expect("référence v1.anonmap manquante");
        assert_eq!(produced, reference, "le format v1 a changé");
        assert_eq!(
            open(&reference, FIXTURE_PASSWORD).unwrap(),
            fixture_session()
        );
    }
}
