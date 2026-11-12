//! Plafonds de sécurité contre les fichiers piégés (bombes de décompression,
//! imbrications sans fin).

/// Taille décompressée maximale d'une entrée de paquet zip.
pub const MAX_ENTRY_UNCOMPRESSED: u64 = 256 * 1024 * 1024;

/// Taille décompressée maximale cumulée de toutes les entrées lues pour un
/// fichier, fichiers embarqués compris.
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 1024 * 1024 * 1024;

/// Nombre maximal d'entrées dans un paquet zip.
pub const MAX_ENTRIES: usize = 20_000;

/// Profondeur maximale de fichiers Office embarqués les uns dans les autres.
pub const MAX_EMBED_DEPTH: usize = 3;

/// Profondeur maximale d'imbrication d'éléments XML.
pub const MAX_XML_DEPTH: usize = 1024;

/// Taille visée d'un bloc renvoyé par `deep_scan` (octets).
pub const DEEP_SCAN_CHUNK: usize = 64 * 1024;
