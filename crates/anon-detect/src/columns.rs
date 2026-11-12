//! Détection par en-tête de colonne : l'en-tête décide du type, la cellule
//! entière est la détection.

use anon_core::{BlockRole, DetectOptions, Detection, Detector, EntityKind, Source, TextBlock};

use crate::fold::tokens;

/// Confiance d'une cellule typée par son en-tête.
const CONF_COLUMN: f32 = 0.9;
/// Longueur minimale (caractères) d'une clé pour qu'elle soit reconnue dans un
/// en-tête plus long (« nom du client ») ; les clés courtes (« nom », « age »,
/// « tel ») exigent un en-tête identique.
const MIN_CONTAINED_KEY_CHARS: usize = 4;

/// En-têtes connus, pliés (minuscules, sans accents).
const HEADERS: &[(&str, EntityKind)] = &[
    // Nom de famille
    ("nom", EntityKind::Nom),
    ("nom de famille", EntityKind::Nom),
    ("last name", EntityKind::Nom),
    ("lastname", EntityKind::Nom),
    ("family name", EntityKind::Nom),
    ("surname", EntityKind::Nom),
    ("apellido", EntityKind::Nom),
    ("apellidos", EntityKind::Nom),
    // Prénom
    ("prenom", EntityKind::Prenom),
    ("prenoms", EntityKind::Prenom),
    ("first name", EntityKind::Prenom),
    ("firstname", EntityKind::Prenom),
    ("given name", EntityKind::Prenom),
    ("nombre", EntityKind::Prenom),
    // Personne
    ("nom complet", EntityKind::Personne),
    ("nom et prenom", EntityKind::Personne),
    ("nom prenom", EntityKind::Personne),
    ("client", EntityKind::Personne),
    ("contact", EntityKind::Personne),
    ("salarie", EntityKind::Personne),
    ("employe", EntityKind::Personne),
    ("beneficiaire", EntityKind::Personne),
    ("full name", EntityKind::Personne),
    ("name", EntityKind::Personne),
    ("nombre completo", EntityKind::Personne),
    // Société
    ("societe", EntityKind::Societe),
    ("entreprise", EntityKind::Societe),
    ("raison sociale", EntityKind::Societe),
    ("company", EntityKind::Societe),
    ("employeur", EntityKind::Societe),
    ("fournisseur", EntityKind::Societe),
    ("empresa", EntityKind::Societe),
    // Email (avant « adresse » : la clé la plus longue gagne)
    ("email", EntityKind::Email),
    ("e-mail", EntityKind::Email),
    ("courriel", EntityKind::Email),
    ("mail", EntityKind::Email),
    ("correo", EntityKind::Email),
    ("adresse email", EntityKind::Email),
    ("adresse mail", EntityKind::Email),
    ("adresse e-mail", EntityKind::Email),
    ("adresse courriel", EntityKind::Email),
    ("email address", EntityKind::Email),
    // Téléphone
    ("telephone", EntityKind::Tel),
    ("tel", EntityKind::Tel),
    ("mobile", EntityKind::Tel),
    ("portable", EntityKind::Tel),
    ("phone", EntityKind::Tel),
    ("fax", EntityKind::Tel),
    ("telefono", EntityKind::Tel),
    // Adresse, lieu
    ("adresse", EntityKind::Adresse),
    ("address", EntityKind::Adresse),
    ("direccion", EntityKind::Adresse),
    ("ville", EntityKind::Lieu),
    ("commune", EntityKind::Lieu),
    ("city", EntityKind::Lieu),
    ("ciudad", EntityKind::Lieu),
    // Dates, âge
    ("date de naissance", EntityKind::DateNaiss),
    ("ne le", EntityKind::DateNaiss),
    ("nee le", EntityKind::DateNaiss),
    ("dob", EntityKind::DateNaiss),
    ("date of birth", EntityKind::DateNaiss),
    ("fecha de nacimiento", EntityKind::DateNaiss),
    ("date", EntityKind::Date),
    ("fecha", EntityKind::Date),
    ("age", EntityKind::Age),
    ("edad", EntityKind::Age),
    // Identifiants
    ("iban", EntityKind::Iban),
    ("rib", EntityKind::Iban),
    ("siret", EntityKind::Siret),
    ("siren", EntityKind::Siret),
    ("ridet", EntityKind::Ridet),
    ("nir", EntityKind::Nir),
    ("secu", EntityKind::Nir),
    ("securite sociale", EntityKind::Nir),
    ("matricule", EntityKind::Matricule),
    ("immatriculation", EntityKind::Plaque),
    ("plaque", EntityKind::Plaque),
    // Montants, fonction
    ("montant", EntityKind::Montant),
    ("salaire", EntityKind::Montant),
    ("total", EntityKind::Montant),
    ("prix", EntityKind::Montant),
    ("amount", EntityKind::Montant),
    ("price", EntityKind::Montant),
    ("salary", EntityKind::Montant),
    ("importe", EntityKind::Montant),
    ("precio", EntityKind::Montant),
    ("fonction", EntityKind::Fonction),
    ("poste", EntityKind::Fonction),
    ("job title", EntityKind::Fonction),
    ("cargo", EntityKind::Fonction),
    ("puesto", EntityKind::Fonction),
];

/// Type de donnée d'une colonne d'après son en-tête (correspondance exacte,
/// sinon la clé la plus longue contenue comme suite de mots).
pub fn kind_for_header(header: &str) -> Option<EntityKind> {
    let h = tokens(header);
    if h.is_empty() {
        return None;
    }
    let mut best: Option<(usize, EntityKind)> = None;
    for (key, kind) in HEADERS {
        let k = tokens(key);
        let chars: usize = k.iter().map(|t| t.chars().count()).sum();
        let hit = if k == h {
            true
        } else {
            chars >= MIN_CONTAINED_KEY_CHARS && !k.is_empty() && h.windows(k.len()).any(|w| w == k)
        };
        if hit && best.is_none_or(|(n, _)| chars > n) {
            best = Some((chars, *kind));
        }
    }
    best.map(|(_, kind)| kind)
}

/// Mots maximum d'une cellule de nom (au-delà, c'est une phrase ou un bandeau).
const MAX_NAME_WORDS: usize = 5;

/// Une cellule de colonne de nom ne contient ni tiret long, ni deux-points, ni
/// point final, et reste courte.
fn plausible_name(cell: &str) -> bool {
    cell.split_whitespace().count() <= MAX_NAME_WORDS
        && !cell.contains(['—', '–', ':'])
        && !cell.ends_with('.')
}

/// Détecteur de colonnes (`name() == "columns"`).
#[derive(Debug, Default, Clone, Copy)]
pub struct ColumnDetector;

impl ColumnDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Detector for ColumnDetector {
    fn name(&self) -> &'static str {
        "columns"
    }

    fn detect(&self, block: &TextBlock, opts: &DetectOptions) -> Vec<Detection> {
        if block.ctx.role != BlockRole::Cell {
            return Vec::new();
        }
        let Some(header) = block.ctx.column_header.as_deref() else {
            return Vec::new();
        };
        let Some(kind) = kind_for_header(header) else {
            return Vec::new();
        };
        if !opts.wants(kind) {
            return Vec::new();
        }
        let text = block.text.as_str();
        let trimmed = text.trim();
        if trimmed.is_empty() || tokens(trimmed) == tokens(header) {
            return Vec::new();
        }
        let is_name_kind = matches!(
            kind,
            EntityKind::Nom | EntityKind::Prenom | EntityKind::Personne
        );
        if is_name_kind && !plausible_name(trimmed) {
            return Vec::new();
        }
        let start = text.len() - text.trim_start().len();
        vec![Detection {
            kind,
            start,
            end: start + trimmed.len(),
            source: Source::Column,
            confidence: CONF_COLUMN,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anon_core::{BlockContext, Profile};
    use proptest::prelude::*;

    fn cell(text: &str, header: Option<&str>, role: BlockRole) -> TextBlock {
        TextBlock {
            text: text.to_string(),
            ctx: BlockContext {
                part: "Feuil1!B".into(),
                role,
                column_header: header.map(str::to_string),
            },
        }
    }

    fn renforce() -> DetectOptions {
        DetectOptions {
            profile: Profile::Renforce,
            ..Default::default()
        }
    }

    fn one(text: &str, header: &str) -> Option<(EntityKind, String)> {
        let b = cell(text, Some(header), BlockRole::Cell);
        let d = ColumnDetector.detect(&b, &renforce());
        assert!(d.len() <= 1);
        d.first().map(|d| {
            assert_eq!(d.source, Source::Column);
            assert!((d.confidence - 0.9).abs() < f32::EPSILON);
            (d.kind, text[d.start..d.end].to_string())
        })
    }

    #[test]
    fn header_table() {
        use EntityKind::*;
        let cases = [
            ("Nom", Nom),
            ("NOM DE FAMILLE", Nom),
            ("Last name", Nom),
            ("Surname", Nom),
            ("Apellido", Nom),
            ("Prénom", Prenom),
            ("First Name", Prenom),
            ("Nombre", Prenom),
            ("Nom complet", Personne),
            ("Nom et prénom", Personne),
            ("Client", Personne),
            ("Contact", Personne),
            ("Salarié", Personne),
            ("Employé", Personne),
            ("Bénéficiaire", Personne),
            ("Full name", Personne),
            ("Name", Personne),
            ("Société", Societe),
            ("Entreprise", Societe),
            ("Raison sociale", Societe),
            ("Company", Societe),
            ("Employeur", Societe),
            ("Fournisseur", Societe),
            ("Email", Email),
            ("E-mail", Email),
            ("Courriel", Email),
            ("Mail", Email),
            ("Adresse email", Email),
            ("Téléphone", Tel),
            ("Tél", Tel),
            ("Mobile", Tel),
            ("Portable", Tel),
            ("Phone", Tel),
            ("Fax", Tel),
            ("Adresse", Adresse),
            ("Address", Adresse),
            ("Ville", Lieu),
            ("Commune", Lieu),
            ("City", Lieu),
            ("Date de naissance", DateNaiss),
            ("Né le", DateNaiss),
            ("DOB", DateNaiss),
            ("IBAN", Iban),
            ("RIB", Iban),
            ("SIRET", Siret),
            ("Siren", Siret),
            ("RIDET", Ridet),
            ("N° sécu", Nir),
            ("NIR", Nir),
            ("Sécurité sociale", Nir),
            ("Matricule", Matricule),
            ("Montant", Montant),
            ("Salaire", Montant),
            ("Total", Montant),
            ("Prix", Montant),
            ("Fonction", Fonction),
            ("Poste", Fonction),
            ("Job title", Fonction),
            ("Âge", Age),
            ("Date", Date),
            ("Immatriculation", Plaque),
            ("Plaque", Plaque),
            // en-têtes composés : la clé la plus longue gagne
            ("Nom du client", Personne),
            ("Téléphone mobile", Tel),
            ("Date de naissance du salarié", DateNaiss),
            ("Adresse e-mail", Email),
            ("Montant TTC (€)", Montant),
            ("  nom  ", Nom),
        ];
        for (header, kind) in cases {
            assert_eq!(kind_for_header(header), Some(kind), "{header}");
        }
        for header in ["", "Remarques", "Code postal", "Référence", "Statut", "N°"] {
            assert_eq!(kind_for_header(header), None, "{header}");
        }
    }

    #[test]
    fn whole_trimmed_cell_is_the_detection() {
        assert_eq!(
            one("  Jean Dupont \u{a0}", "Nom complet"),
            Some((EntityKind::Personne, "Jean Dupont".to_string()))
        );
        assert_eq!(
            one("é 12 rue de la Paix, 75002 Paris", "Adresse"),
            Some((
                EntityKind::Adresse,
                "é 12 rue de la Paix, 75002 Paris".to_string()
            ))
        );
        assert_eq!(
            one("1250000", "Montant"),
            Some((EntityKind::Montant, "1250000".to_string()))
        );
        let b = cell("  Éric ", Some("Prénom"), BlockRole::Cell);
        let d = ColumnDetector.detect(&b, &renforce());
        assert_eq!((d[0].start, d[0].end), (2, 7));
    }

    #[test]
    fn nothing_emitted_when_not_applicable() {
        assert_eq!(one("", "Nom"), None);
        assert_eq!(one("   \u{a0}", "Nom"), None);
        assert_eq!(one("Nom", "Nom"), None); // cellule d'en-tête
        assert_eq!(one("  nom : ", "Nom"), None);
        assert_eq!(one("Jean", "Remarques"), None);
        for role in [
            BlockRole::Body,
            BlockRole::Header,
            BlockRole::Other,
            BlockRole::Comment,
        ] {
            let b = cell("Jean", Some("Nom"), role);
            assert!(ColumnDetector.detect(&b, &renforce()).is_empty());
        }
        let b = cell("Jean", None, BlockRole::Cell);
        assert!(ColumnDetector.detect(&b, &renforce()).is_empty());
    }

    #[test]
    fn banner_cells_are_not_names() {
        for header in ["Nom", "Prénom", "Personne", "Nom complet", "Client"] {
            assert_eq!(
                one("DONNÉES FICTIVES — exemple de test", header),
                None,
                "{header}"
            );
            assert_eq!(one("Note : à vérifier", header), None);
            assert_eq!(one("Jean Dupont est parti.", header), None);
            assert_eq!(one("un deux trois quatre cinq six", header), None);
            assert_eq!(one("a – b", header), None);
        }
        assert_eq!(one("Jean Marie Pierre de la Fontaine", "Nom"), None);
        assert_eq!(
            one("Jean-Marie Dupont", "Nom complet"),
            Some((EntityKind::Personne, "Jean-Marie Dupont".to_string()))
        );
        assert_eq!(
            one("Marie de la Fontaine Bois", "Nom"),
            Some((EntityKind::Nom, "Marie de la Fontaine Bois".to_string()))
        );
        // les autres types ne sont pas concernés
        assert!(one("Note : voir annexe — 2 pages.", "Adresse").is_some());
    }

    #[test]
    fn respects_wants() {
        let b = cell("12/03/2024", Some("Date"), BlockRole::Cell);
        assert!(
            ColumnDetector
                .detect(&b, &DetectOptions::default())
                .is_empty()
        );
        assert_eq!(ColumnDetector.detect(&b, &renforce()).len(), 1);
        let mut opts = renforce();
        opts.disabled.insert(EntityKind::Date);
        assert!(ColumnDetector.detect(&b, &opts).is_empty());
        let b = cell("Jean", Some("Nom"), BlockRole::Cell);
        let mut opts = DetectOptions::default();
        opts.disabled.insert(EntityKind::Nom);
        assert!(ColumnDetector.detect(&b, &opts).is_empty());
        assert_eq!(ColumnDetector.name(), "columns");
    }

    proptest! {
        #[test]
        fn never_panics_and_offsets_valid(text in "\\PC{0,80}", header in "\\PC{0,30}") {
            let b = cell(&text, Some(&header), BlockRole::Cell);
            for d in ColumnDetector.detect(&b, &renforce()) {
                prop_assert!(d.start < d.end && d.end <= text.len());
                prop_assert!(text.is_char_boundary(d.start) && text.is_char_boundary(d.end));
            }
        }
    }
}
