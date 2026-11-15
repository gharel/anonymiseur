//! Trousseau du système : mémoriser le mot de passe d'une table de correspondance.
//!
//! - Windows : Gestionnaire d'identification (`windows-native-keyring-store`).
//! - macOS : Trousseau d'accès (`apple-native-keyring-store`).
//! - ailleurs : [`SystemStore::new`] renvoie [`VaultError::KeychainUnavailable`].
//!
//! Le secret est rangé sous le service [`SERVICE`] et l'identifiant du fichier
//! (voir [`key_id`]). Aucune erreur ne reprend le contenu renvoyé par le système.

use std::collections::HashMap;
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::{VaultError, parse_header};

/// Nom de service sous lequel les secrets sont rangés.
pub const SERVICE: &str = "Anonymiseur";

/// Magasin de mots de passe, indexé par l'identifiant d'un fichier `.anonmap`.
pub trait SecretStore {
    /// Enregistre (ou remplace) le mot de passe.
    fn remember(&self, key_id: &str, password: &str) -> Result<(), VaultError>;
    /// Relit le mot de passe ; `Ok(None)` s'il n'y en a pas.
    fn recall(&self, key_id: &str) -> Result<Option<Zeroizing<String>>, VaultError>;
    /// Oublie le mot de passe ; sans effet s'il n'y en a pas.
    fn forget(&self, key_id: &str) -> Result<(), VaultError>;
}

/// Identifiant stable d'un fichier `.anonmap` : le sel de l'en-tête, en hexadécimal
/// minuscule (32 caractères). Il ne révèle aucun contenu. Un fichier re-scellé reçoit
/// un nouveau sel, donc un nouvel identifiant. `None` si ce n'est pas un `.anonmap` valide.
pub fn key_id(sealed: &[u8]) -> Option<String> {
    let (_, salt, _) = parse_header(sealed).ok()?;
    Some(salt.iter().map(|b| format!("{b:02x}")).collect())
}

/// Magasin en mémoire (tests, ou secret à durée de vie du processus).
#[derive(Default)]
pub struct MemoryStore {
    secrets: Mutex<HashMap<String, Zeroizing<String>>>,
}

impl std::fmt::Debug for MemoryStore {
    /// Jamais de secret dans `Debug` (invariant 3).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MemoryStore")
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemoryStore {
    fn remember(&self, key_id: &str, password: &str) -> Result<(), VaultError> {
        let mut map = self
            .secrets
            .lock()
            .map_err(|_| VaultError::KeychainFailure)?;
        map.insert(key_id.to_string(), Zeroizing::new(password.to_string()));
        Ok(())
    }

    fn recall(&self, key_id: &str) -> Result<Option<Zeroizing<String>>, VaultError> {
        let map = self
            .secrets
            .lock()
            .map_err(|_| VaultError::KeychainFailure)?;
        Ok(map.get(key_id).map(|s| Zeroizing::new((**s).clone())))
    }

    fn forget(&self, key_id: &str) -> Result<(), VaultError> {
        let mut map = self
            .secrets
            .lock()
            .map_err(|_| VaultError::KeychainFailure)?;
        map.remove(key_id);
        Ok(())
    }
}

/// Trousseau du système d'exploitation.
pub struct SystemStore {
    #[cfg(any(windows, target_os = "macos"))]
    store: std::sync::Arc<dyn keyring_core::api::CredentialStoreApi + Send + Sync>,
}

impl std::fmt::Debug for SystemStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SystemStore")
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl SystemStore {
    /// Ouvre le trousseau du système.
    pub fn new() -> Result<Self, VaultError> {
        #[cfg(windows)]
        let store = windows_native_keyring_store::Store::new().map_err(map_error)?;
        #[cfg(target_os = "macos")]
        let store = apple_native_keyring_store::keychain::Store::new().map_err(map_error)?;
        Ok(Self { store })
    }

    fn entry(&self, key_id: &str) -> Result<keyring_core::Entry, VaultError> {
        // Windows : identifiant propre à cette machine (pas d'itinérance du domaine).
        #[cfg(windows)]
        let mods = HashMap::from([("persistence", "Local")]);
        #[cfg(not(windows))]
        let mods: HashMap<&str, &str> = HashMap::new();
        self.store
            .build(SERVICE, key_id, Some(&mods))
            .map_err(map_error)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
impl SystemStore {
    /// Pas de trousseau sur cette plateforme.
    pub fn new() -> Result<Self, VaultError> {
        Err(VaultError::KeychainUnavailable)
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn map_error(e: keyring_core::Error) -> VaultError {
    use keyring_core::Error;
    match e {
        Error::NoStorageAccess(_) => VaultError::KeychainDenied,
        Error::NoDefaultStore | Error::NotSupportedByStore(_) => VaultError::KeychainUnavailable,
        _ => VaultError::KeychainFailure,
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl SecretStore for SystemStore {
    fn remember(&self, key_id: &str, password: &str) -> Result<(), VaultError> {
        self.entry(key_id)?
            .set_password(password)
            .map_err(map_error)
    }

    fn recall(&self, key_id: &str) -> Result<Option<Zeroizing<String>>, VaultError> {
        match self.entry(key_id)?.get_password() {
            Ok(p) => Ok(Some(Zeroizing::new(p))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_error(e)),
        }
    }

    fn forget(&self, key_id: &str) -> Result<(), VaultError> {
        match self.entry(key_id)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_error(e)),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
impl SecretStore for SystemStore {
    fn remember(&self, _: &str, _: &str) -> Result<(), VaultError> {
        Err(VaultError::KeychainUnavailable)
    }
    fn recall(&self, _: &str) -> Result<Option<Zeroizing<String>>, VaultError> {
        Err(VaultError::KeychainUnavailable)
    }
    fn forget(&self, _: &str) -> Result<(), VaultError> {
        Err(VaultError::KeychainUnavailable)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::seal;
    use anon_core::session::Session;

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::new();
        assert_eq!(s.recall("a").unwrap(), None);
        s.remember("a", "secret é").unwrap();
        assert_eq!(s.recall("a").unwrap().unwrap().as_str(), "secret é");
        s.remember("a", "autre").unwrap();
        assert_eq!(s.recall("a").unwrap().unwrap().as_str(), "autre");
        assert_eq!(s.recall("b").unwrap(), None);
        s.forget("a").unwrap();
        s.forget("a").unwrap();
        assert_eq!(s.recall("a").unwrap(), None);
    }

    #[test]
    fn key_id_is_salt_hex_and_stable() {
        let bytes = seal(&Session::new(), "mot de passe d'essai").unwrap();
        let id = key_id(&bytes).unwrap();
        assert_eq!(id.len(), 32);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert_eq!(id, key_id(&bytes).unwrap());
        let salt: String = bytes[18..34].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(id, salt);
        let other = seal(&Session::new(), "mot de passe d'essai").unwrap();
        assert_ne!(id, key_id(&other).unwrap());
    }

    #[test]
    fn key_id_rejects_non_vaults() {
        assert_eq!(key_id(b""), None);
        assert_eq!(key_id(b"%PDF-1.7 quelconque"), None);
        assert_eq!(key_id(&[0u8; 200]), None);
        let bytes = seal(&Session::new(), "x").unwrap();
        assert_eq!(key_id(&bytes[..20]), None);
    }

    #[test]
    fn errors_are_french_and_generic() {
        for e in [
            VaultError::KeychainUnavailable,
            VaultError::KeychainDenied,
            VaultError::KeychainFailure,
        ] {
            assert!(e.to_string().contains("rousseau"));
        }
    }

    /// Aller-retour dans le vrai trousseau, uniquement si `ANON_VAULT_REAL_KEYCHAIN`
    /// est définie (jamais par défaut : `cargo test` n'écrit pas dans le trousseau de
    /// l'utilisateur). Utilise un identifiant jetable, nettoyé à la fin.
    #[test]
    fn system_store_real_round_trip_when_enabled() {
        if std::env::var_os("ANON_VAULT_REAL_KEYCHAIN").is_none() {
            return;
        }
        let store = match SystemStore::new() {
            Ok(s) => s,
            Err(VaultError::KeychainUnavailable) => return,
            Err(e) => panic!("{e}"),
        };
        let id = "test-aller-retour-jetable";
        store.forget(id).unwrap();
        assert_eq!(store.recall(id).unwrap(), None);
        store.remember(id, "phrase-d-essai-é-42").unwrap();
        assert_eq!(
            store.recall(id).unwrap().unwrap().as_str(),
            "phrase-d-essai-é-42"
        );
        store.forget(id).unwrap();
        assert_eq!(store.recall(id).unwrap(), None);
    }
}
