//! Évaluation des textes : un sujet par personne mentionnée.
//!
//! **Personnes.** Une mention `Personne`, `Nom` ou `Prenom` désigne une
//! personne. Les mentions d'une même personne sont regroupées par l'indice de
//! leur balise quand la valeur est masquée (`[PERSONNE_1]`, `[NOM_1]` et
//! `[PRENOM_1]` désignent la même personne), sinon par `value_id`. Le sujet est
//! désigné par le `value_id` de sa première mention `Personne`, à défaut de sa
//! première mention.
//!
//! **Rattachement des indices.** Les indices d'une personne sont les autres
//! mentions des paragraphes (`TextUnit`) où elle apparaît. Quand un paragraphe
//! mentionne plusieurs personnes, chaque indice va à la mention de personne la
//! plus proche dans l'ordre des mentions ; à égale distance, à la précédente
//! (« Jean Dupont, 62 ans, et Marie Durand » : l'âge suit le nom qu'il décrit).
//! Un paragraphe sans personne ne rattache rien : les pronoms ne sont pas suivis
//! d'un paragraphe à l'autre.
//!
//! **Isolement.** Échec `DirectIdentifier` pour chaque nom, prénom ou autre
//! identifiant direct rattaché (email, téléphone, adresse, NIR, IBAN, carte,
//! plaque) encore visible ou seulement rendu moins précis ; correction : le
//! masquer. Ensuite, estimation de la taille du groupe à partir des indices
//! visibles ou rendus moins précis :
//!
//! ```text
//! groupe = population du lieu × part de la tranche d'âge × part du métier
//! ```
//!
//! - lieu : commune (`population::places`, la plus petite des homonymes, par
//!   prudence), département, province, région ou territoire
//!   (`population::area`), code postal ([`POSTAL_CODE_POPULATION`], ou la zone
//!   de « 54*** »), lieu inconnu des données ([`UNKNOWN_PLACE_POPULATION`]) ;
//!   sans lieu, la population de la France. Plusieurs lieux : le plus petit.
//! - âge : `population::age_share` du territoire du lieu (France par défaut)
//!   sur l'âge exact (« 62 ans ») ou la tranche affichée (« 60-69 ans ») ; une
//!   date de naissance donne l'âge au 1er janvier [`REFERENCE_YEAR`], divisé
//!   par [`DAYS_PER_YEAR`] (date complète) ou [`MONTHS_PER_YEAR`] (mois et
//!   année). Plusieurs indices d'âge : le plus précis.
//! - métier : effectif de `population::occupation` rapporté à la population de
//!   la France (effectifs nationaux appliqués à tout territoire) ; intitulé
//!   inconnu : [`UNKNOWN_OCCUPATION_SHARE`] ; catégorie plus large
//!   (« professionnelle de santé ») : [`BROAD_OCCUPATION_SHARE`] ;
//!   « professionnel » : aucune réduction. Plusieurs métiers : le plus précis.
//! - une fonction visible et une organisation nommée visible rattachées à la
//!   même personne (« responsable technique de Nautilus Pacifique SARL »)
//!   désignent [`NAMED_ROLE_GROUP`] personne.
//!
//! Le résultat, arrondi et au moins 1, est `SubjectReport::group_estimate`.
//! Le sexe n'est pas pris en compte. Échec `SmallGroup` sous `k_min`.
//!
//! **Corrections de l'isolement**, classées (la première est conseillée) :
//! d'abord celles qui atteignent `k_min`, de la moindre perte à la plus grande,
//! puis les autres, de la plus grande estimation à la plus petite. Perte : nombre
//! de crans de hiérarchie (`anon_dict::hierarchy`) ; masquer un indice compte
//! un cran de plus que son dernier niveau ; masquer l'organisation compte
//! [`ORG_MASK_LOSS`]. À perte égale : la généralisation, puis le masquage de
//! l'organisation, puis le masquage d'un indice. Candidats :
//! - la généralisation de moindre perte qui atteint `k_min` (recherche
//!   exhaustive sur au plus [`MAX_FIX_DIMENSIONS`] indices), en
//!   `Fix::Generalize` pour un seul indice, `Fix::GeneralizeMany` sinon ;
//! - le masquage de l'organisation (règle fonction et organisation) ;
//! - le masquage de chaque indice qui agrandit le groupe.
//!
//! Aucune correction proposée ne réduit l'estimation.
//!
//! **Recoupement.** Échec `StableIdentifier` pour chaque identifiant stable
//! rattaché encore exposé (matricule, identifiant, lien, adresse IP, SIRET,
//! RIDET) ; correction : le masquer.
//!
//! **Déduction.** Si la personne échoue à l'isolement, échec `SensitiveLinked`
//! pour chaque donnée sensible (art. 9 et 10) ou événement personnel rattaché
//! encore exposé ; correction : le masquer.
//!
//! **Approche.** En V2.0, le verdict des textes suit toujours l'approche
//! simplifiée, la plus prudente : toute voie de réidentification compte, quelles
//! que soient les réponses de `AssessSettings::answers` (le moteur consigne ces
//! réponses dans la fiche d'évaluation).
//!
//! Un dossier sans personne mentionnée n'a aucun sujet : il reste anonyme. Une
//! personne dont un nom reste visible échoue toujours.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use anon_core::{
    Criterion, EntityKind, Failure, FailureReason, Fix, Mention, Shown, SubjectRef, SubjectReport,
    TextUnit,
};
use anon_dict::fold;
use anon_dict::hierarchy::{generalize, max_level};
use anon_dict::population::{
    Territory, age_share, area, occupation, places, postal_area, territory_population,
};

// ---------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------

/// Taille du groupe désigné par une fonction précise rattachée à une
/// organisation nommée et visible : une seule personne (exemple du plan :
/// « responsable technique de Nautilus Pacifique SARL »).
pub const NAMED_ROLE_GROUP: u64 = 1;
/// Part de la population pour un intitulé de fonction absent des données : un
/// métier rare (0,1 %, environ 68 000 personnes en France), par prudence.
pub const UNKNOWN_OCCUPATION_SHARE: f64 = 0.001;
/// Part de la population pour une catégorie de métiers (« professionnelle de
/// santé ») : 5 %. Au moins la part du métier qu'elle englobe.
pub const BROAD_OCCUPATION_SHARE: f64 = 0.05;
/// Population d'un lieu inconnu des données (quartier, lieu-dit, ville
/// étrangère) : ordre de grandeur d'une ville moyenne.
pub const UNKNOWN_PLACE_POPULATION: u64 = 50_000;
/// Population d'un code postal complet : environ 6 300 codes pour 68 millions
/// d'habitants en France, soit 11 000 en moyenne, arrondi à 10 000 par prudence.
pub const POSTAL_CODE_POPULATION: u64 = 10_000;
/// Année des données d'âge (population au 1er janvier 2026) : une personne née
/// l'année A a `REFERENCE_YEAR - 1 - A` ans révolus.
pub const REFERENCE_YEAR: u32 = 2026;
/// Jours par an : une date de naissance complète divise la part d'un âge.
pub const DAYS_PER_YEAR: f64 = 365.25;
/// Mois par an : une date de naissance au mois près divise la part d'un âge.
pub const MONTHS_PER_YEAR: f64 = 12.0;
/// Âge maximal lu dans une valeur.
const AGE_MAX: u32 = 120;
/// Nombre maximal d'indices combinés dans la recherche de la généralisation de
/// moindre perte (borne le temps de calcul ; les autres restent tels quels).
pub const MAX_FIX_DIMENSIONS: usize = 6;
/// Perte comptée pour le masquage d'une organisation : un cran.
pub const ORG_MASK_LOSS: u32 = 1;
/// Nombre de chiffres d'un code postal.
const POSTAL_CODE_DIGITS: usize = 5;
/// Chiffres de zone d'un code postal.
const POSTAL_PREFIX_DIGITS: usize = 2;
/// Préfixe postal de la Nouvelle-Calédonie (988xx).
const NC_POSTAL_PREFIX: &str = "988";
/// Années d'une décennie.
const DECADE: u32 = 10;

/// Débuts de noms de mois pliés (français, anglais), pour lire une date de
/// naissance au mois près.
const MONTH_STEMS: &[&str] = &[
    "janv", "jan", "fev", "feb", "mars", "mar", "avr", "apr", "mai", "may", "juin", "jun", "juil",
    "jul", "aout", "aug", "sep", "oct", "nov", "dec",
];

// ---------------------------------------------------------------------------
// Rôles des mentions
// ---------------------------------------------------------------------------

/// Rôle d'une mention dans l'évaluation d'une personne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Nom, prénom : la personne elle-même.
    Name,
    /// Identifiant direct (isolement).
    Direct,
    /// Identifiant stable (recoupement).
    Stable,
    /// Donnée sensible ou événement personnel (déduction).
    Sensitive,
    /// Organisation (règle fonction et organisation).
    Organization,
    Place,
    Age,
    Birth,
    Occupation,
    /// Indice sans effet sur l'estimation (date, montant, autre).
    Other,
}

fn role(kind: EntityKind) -> Role {
    use EntityKind::*;
    match kind {
        Personne | Nom | Prenom => Role::Name,
        Email | Tel | Adresse | Nir | Iban | Cb | Plaque => Role::Direct,
        Matricule | Identifiant | Url | Ip | Siret | Ridet => Role::Stable,
        Sante | Opinion | Religion | Syndicat | Orientation | Origine | Genetique | Biometrie
        | Judiciaire | Evenement => Role::Sensitive,
        Societe => Role::Organization,
        Lieu => Role::Place,
        Age => Role::Age,
        DateNaiss => Role::Birth,
        Fonction => Role::Occupation,
        Date | Montant | Autre => Role::Other,
    }
}

/// Vrai si le document produit montre quelque chose de la valeur.
fn exposed(m: &Mention) -> bool {
    match &m.shown {
        Shown::Masked(_) => false,
        Shown::Generalized(s) => !s.trim().is_empty(),
        Shown::Visible => !m.value.trim().is_empty(),
    }
}

// ---------------------------------------------------------------------------
// Personnes et rattachement des indices
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PersonKey {
    /// Indice de balise partagé par `[PERSONNE_n]`, `[NOM_n]`, `[PRENOM_n]`.
    Tag(u32),
    Value(usize),
}

fn person_key(m: &Mention) -> PersonKey {
    match &m.shown {
        Shown::Masked(tag) if tag.kind.is_person_part() => PersonKey::Tag(tag.index),
        _ => PersonKey::Value(m.value_id),
    }
}

/// Une personne : ses mentions et ses indices, sans doublon de `value_id`.
struct Person<'a> {
    id: usize,
    has_personne: bool,
    names: Vec<&'a Mention>,
    clues: Vec<&'a Mention>,
    seen_names: HashSet<usize>,
    seen_clues: HashSet<usize>,
}

impl<'a> Person<'a> {
    fn new(m: &'a Mention) -> Self {
        Self {
            id: m.value_id,
            has_personne: m.kind == EntityKind::Personne,
            names: Vec::new(),
            clues: Vec::new(),
            seen_names: HashSet::new(),
            seen_clues: HashSet::new(),
        }
    }

    fn add_name(&mut self, m: &'a Mention) {
        if m.kind == EntityKind::Personne && !self.has_personne {
            self.id = m.value_id;
            self.has_personne = true;
        }
        if self.seen_names.insert(m.value_id) {
            self.names.push(m);
        }
    }

    fn add_clue(&mut self, m: &'a Mention) {
        if self.seen_clues.insert(m.value_id) {
            self.clues.push(m);
        }
    }
}

/// Sujet de la mention de personne la plus proche de `pos` (positions dans le
/// paragraphe) ; à égale distance, la précédente.
fn nearest(anchors: &[(usize, usize)], pos: usize) -> Option<usize> {
    anchors
        .iter()
        .min_by_key(|(p, _)| (p.abs_diff(pos), *p > pos))
        .map(|(_, s)| *s)
}

/// Personnes des textes, dans l'ordre de leur première mention.
fn people(units: &[TextUnit]) -> Vec<Person<'_>> {
    let mut out: Vec<Person<'_>> = Vec::new();
    let mut index: HashMap<PersonKey, usize> = HashMap::new();
    for unit in units {
        let mut anchors: Vec<(usize, usize)> = Vec::new();
        for (pos, m) in unit.mentions.iter().enumerate() {
            if !m.kind.is_person_part() {
                continue;
            }
            let next = out.len();
            let s = *index.entry(person_key(m)).or_insert(next);
            if s == next {
                out.push(Person::new(m));
            }
            if let Some(p) = out.get_mut(s) {
                p.add_name(m);
            }
            anchors.push((pos, s));
        }
        if anchors.is_empty() {
            continue;
        }
        for (pos, m) in unit.mentions.iter().enumerate() {
            if m.kind.is_person_part() {
                continue;
            }
            if let Some(p) = nearest(&anchors, pos).and_then(|s| out.get_mut(s)) {
                p.add_clue(m);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Mesures et niveaux des indices
// ---------------------------------------------------------------------------

/// Ce qu'une forme affichée d'un indice apprend sur la personne.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Measure {
    /// Lieu : population et territoire.
    Place {
        population: u64,
        territory: Territory,
    },
    /// Âges possibles (années révolues) ; la part d'un âge est divisée par
    /// `per_year` (date de naissance au jour ou au mois près).
    Ages { from: u32, to: u32, per_year: f64 },
    /// Part de la population (métier).
    Share(f64),
    /// Rien d'exploitable (forme générale, valeur illisible, masquée).
    Nothing,
}

/// Une forme possible d'un indice : niveau de hiérarchie, texte affiché
/// (`None` : masqué), mesure.
#[derive(Debug, Clone)]
struct Level {
    level: u8,
    form: Option<String>,
    measure: Measure,
}

/// Mesure englobante : jamais plus précise que `prev` (une forme moins précise
/// ne peut pas réduire le groupe).
fn widen(prev: Measure, next: Measure) -> Measure {
    match (prev, next) {
        (_, Measure::Nothing) | (Measure::Nothing, _) => Measure::Nothing,
        (
            Measure::Place { population: a, .. },
            Measure::Place {
                population: b,
                territory,
            },
        ) => Measure::Place {
            population: a.max(b),
            territory,
        },
        (
            Measure::Ages {
                from: f1,
                to: t1,
                per_year: p1,
            },
            Measure::Ages {
                from: f2,
                to: t2,
                per_year: p2,
            },
        ) => Measure::Ages {
            from: f1.min(f2),
            to: t1.max(t2),
            per_year: p1.min(p2),
        },
        (Measure::Share(a), Measure::Share(b)) => Measure::Share(a.max(b)),
        (_, other) => other,
    }
}

/// Suites de chiffres d'un texte.
fn numbers(text: &str) -> Vec<u32> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|r| !r.is_empty())
        .filter_map(|r| r.parse().ok())
        .collect()
}

fn place_measure(text: &str, hierarchy_level: u8) -> Measure {
    let t = text.trim();
    let commune = || {
        places(t)
            .into_iter()
            .min_by_key(|p| p.population)
            .map(|p| Measure::Place {
                population: u64::from(p.population),
                territory: p.territory,
            })
    };
    let zone = || {
        area(t).map(|a| Measure::Place {
            population: a.population,
            territory: a.territory,
        })
    };
    let postal = || {
        let digits = t.bytes().filter(u8::is_ascii_digit).count();
        let prefix = t.get(..POSTAL_PREFIX_DIGITS)?;
        let stars = t.get(POSTAL_PREFIX_DIGITS..)?;
        if t.len() == POSTAL_CODE_DIGITS && digits == POSTAL_CODE_DIGITS {
            let territory = if t.starts_with(NC_POSTAL_PREFIX) {
                Territory::NouvelleCaledonie
            } else {
                Territory::France
            };
            return Some(Measure::Place {
                population: POSTAL_CODE_POPULATION,
                territory,
            });
        }
        if t.len() == POSTAL_CODE_DIGITS && stars.bytes().all(|b| b == b'*') {
            return postal_area(prefix).map(|a| Measure::Place {
                population: a.population,
                territory: a.territory,
            });
        }
        None
    };
    let found = if hierarchy_level == 0 {
        commune().or_else(postal).or_else(zone)
    } else {
        zone().or_else(postal).or_else(commune)
    };
    found.unwrap_or(Measure::Place {
        population: UNKNOWN_PLACE_POPULATION,
        territory: Territory::France,
    })
}

fn age_measure(text: &str) -> Measure {
    let n = numbers(text);
    let f = fold(text);
    let open_above = ["plus de", "au-dela", "over", "+"]
        .iter()
        .any(|w| f.contains(w));
    let open_below = ["moins de", "under"].iter().any(|w| f.contains(w));
    let (from, to) = match n.as_slice() {
        // « plus de 60 ans », « moins de 18 ans » : tranche ouverte.
        [a] if open_above => (*a, AGE_MAX),
        [a] if open_below => (0, a.saturating_sub(1)),
        [a] => (*a, *a),
        [a, b, ..] if a <= b => (*a, *b),
        _ => return Measure::Nothing,
    };
    if to > AGE_MAX {
        return Measure::Nothing;
    }
    Measure::Ages {
        from,
        to,
        per_year: 1.0,
    }
}

/// Âge révolu au 1er janvier [`REFERENCE_YEAR`] d'une personne née en `year`.
fn age_of_birth_year(year: u32) -> u32 {
    REFERENCE_YEAR.saturating_sub(1).saturating_sub(year)
}

fn birth_measure(text: &str) -> Measure {
    let folded = fold(text);
    let n = numbers(&folded);
    let Some(year) = n
        .iter()
        .copied()
        .find(|y| (1000..=REFERENCE_YEAR).contains(y))
    else {
        return Measure::Nothing;
    };
    if folded.contains("annee") || folded.contains("decennie") || folded.ends_with('s') {
        // « années 1940 » : une décennie.
        let start = year / DECADE * DECADE;
        return Measure::Ages {
            from: age_of_birth_year(start + DECADE - 1),
            to: age_of_birth_year(start),
            per_year: 1.0,
        };
    }
    let others = n.iter().filter(|x| **x != year).count();
    let named_month = folded
        .split(|c: char| !c.is_alphabetic())
        .any(|w| w.len() >= 3 && MONTH_STEMS.iter().any(|m| w.starts_with(m)));
    let components = others + usize::from(named_month);
    let per_year = match components {
        0 => 1.0,
        1 => MONTHS_PER_YEAR,
        _ => DAYS_PER_YEAR,
    };
    let age = age_of_birth_year(year);
    Measure::Ages {
        from: age,
        to: age,
        per_year,
    }
}

/// Part de la population d'un intitulé, selon le niveau de hiérarchie de sa
/// forme (0 : intitulé ; 1 : catégorie plus large ; 2 : « professionnel »).
fn occupation_measure(text: &str, hierarchy_level: Option<u8>) -> Measure {
    let france = territory_population(Territory::France).max(1) as f64;
    let known = || occupation(text).map(|o| f64::from(o.count) / france);
    match hierarchy_level {
        Some(0) => Measure::Share(known().unwrap_or(UNKNOWN_OCCUPATION_SHARE)),
        Some(1) => Measure::Share(BROAD_OCCUPATION_SHARE),
        Some(_) => Measure::Nothing,
        // Forme choisie hors de la hiérarchie (« salarié ») : un métier connu,
        // sinon une catégorie, sauf un mot sans information.
        None => {
            let f = fold(text);
            if let Some(s) = known() {
                Measure::Share(s)
            } else if [
                "professionnel",
                "professionnelle",
                "salarie",
                "salariee",
                "employe",
                "employee",
                "agent",
                "agente",
            ]
            .contains(&f.trim())
            {
                Measure::Nothing
            } else {
                Measure::Share(BROAD_OCCUPATION_SHARE)
            }
        }
    }
}

fn measure(role: Role, text: &str, hierarchy_level: Option<u8>) -> Measure {
    match role {
        Role::Place => place_measure(text, hierarchy_level.unwrap_or(0)),
        Role::Age => age_measure(text),
        Role::Birth => birth_measure(text),
        Role::Occupation => occupation_measure(text, hierarchy_level),
        _ => Measure::Nothing,
    }
}

/// Niveaux possibles d'un indice exposé : la forme actuelle, les formes moins
/// précises de la hiérarchie, puis le masquage. Vide si l'indice est masqué.
fn levels(m: &Mention) -> Vec<Level> {
    let role = role(m.kind);
    let (current, text) = match &m.shown {
        Shown::Masked(_) => return Vec::new(),
        Shown::Visible => (Some(0u8), m.value.as_str()),
        Shown::Generalized(s) => (
            (1..=max_level(m.kind))
                .find(|l| generalize(m.kind, &m.value, *l).as_deref() == Some(s.as_str())),
            s.as_str(),
        ),
    };
    let mut out = vec![Level {
        level: current.unwrap_or(0),
        form: Some(text.to_string()),
        measure: measure(role, text, current),
    }];
    if let Some(cur) = current {
        for l in cur.saturating_add(1)..=max_level(m.kind) {
            if let Some(form) = generalize(m.kind, &m.value, l) {
                let prev = out.last().map_or(Measure::Nothing, |p| p.measure);
                let next = measure(role, &form, Some(l));
                out.push(Level {
                    level: l,
                    measure: widen(prev, next),
                    form: Some(form),
                });
            }
        }
    }
    out.push(Level {
        level: max_level(m.kind).saturating_add(1),
        form: None,
        measure: Measure::Nothing,
    });
    out
}

/// Niveaux par (type, valeur, forme rendue moins précise), calculés une fois.
type LevelCache<'a> = HashMap<(EntityKind, &'a str, Option<&'a str>), Rc<Vec<Level>>>;

/// Un indice qui entre dans l'estimation.
struct Dimension<'a> {
    mention: &'a Mention,
    role: Role,
    /// Fonction affichée telle quelle (règle fonction et organisation).
    visible: bool,
    levels: Rc<Vec<Level>>,
}

fn dimension<'a>(m: &'a Mention, cache: &mut LevelCache<'a>) -> Option<Dimension<'a>> {
    let role = role(m.kind);
    if !matches!(
        role,
        Role::Place | Role::Age | Role::Birth | Role::Occupation
    ) || !exposed(m)
    {
        return None;
    }
    let generalized = match &m.shown {
        Shown::Generalized(s) => Some(s.as_str()),
        _ => None,
    };
    let levels = cache
        .entry((m.kind, m.value.as_str(), generalized))
        .or_insert_with(|| Rc::new(levels(m)))
        .clone();
    Some(Dimension {
        mention: m,
        role,
        visible: matches!(m.shown, Shown::Visible),
        levels,
    })
}

// ---------------------------------------------------------------------------
// Estimation
// ---------------------------------------------------------------------------

/// Taille estimée du groupe quand chaque indice `dims[i]` est au niveau
/// `state[i]` (indice dans ses niveaux). `org_visible` : une organisation
/// nommée reste exposée.
fn group_size(dims: &[Dimension<'_>], state: &[usize], org_visible: bool) -> u64 {
    let at = |i: usize| -> Option<&Level> {
        dims.get(i)
            .and_then(|d| d.levels.get(state.get(i).copied().unwrap_or(0)))
    };
    let named_role = org_visible
        && dims
            .iter()
            .enumerate()
            .any(|(i, d)| d.role == Role::Occupation && d.visible && state.get(i) == Some(&0));
    if named_role {
        return NAMED_ROLE_GROUP;
    }
    let france = territory_population(Territory::France);
    let mut place: Option<(u64, Territory)> = None;
    for i in 0..dims.len() {
        if let Some(Measure::Place {
            population,
            territory,
        }) = at(i).map(|l| l.measure)
            && place.is_none_or(|(p, _)| population < p)
        {
            place = Some((population, territory));
        }
    }
    let (population, territory) = place.unwrap_or((france, Territory::France));
    let mut age = 1.0f64;
    let mut job = 1.0f64;
    for i in 0..dims.len() {
        match at(i).map(|l| l.measure) {
            Some(Measure::Ages { from, to, per_year }) => {
                let s = age_share(territory, from, to) / per_year.max(1.0);
                age = age.min(s);
            }
            Some(Measure::Share(s)) => job = job.min(s.clamp(0.0, 1.0)),
            _ => {}
        }
    }
    let x = population as f64 * age * job;
    if !x.is_finite() {
        return population.max(1);
    }
    (x.round() as u64).clamp(1, population.max(1))
}

/// Perte d'un niveau : nombre de crans depuis la forme actuelle.
fn loss(d: &Dimension<'_>, index: usize) -> u32 {
    let base = d.levels.first().map_or(0, |l| l.level);
    let to = d.levels.get(index).map_or(base, |l| l.level);
    u32::from(to.saturating_sub(base))
}

/// Une correction candidate de l'isolement.
struct Candidate {
    fix: Fix,
    estimate: u64,
    loss: u32,
    /// 0 : généralisation ; 1 : organisation masquée ; 2 : indice masqué.
    rank: u8,
}

/// Généralisation de moindre perte qui atteint `k_min` (recherche exhaustive
/// sur les premiers [`MAX_FIX_DIMENSIONS`] indices qui ont une hiérarchie).
fn best_generalization(dims: &[Dimension<'_>], org_visible: bool, k_min: u32) -> Option<Candidate> {
    // Indices et nombre de formes non masquées de chaque dimension retenue.
    let movable: Vec<(usize, usize)> = dims
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let forms = d.levels.iter().filter(|l| l.form.is_some()).count();
            (forms > 1).then_some((i, forms))
        })
        .take(MAX_FIX_DIMENSIONS)
        .collect();
    if movable.is_empty() {
        return None;
    }
    let mut state = vec![0usize; dims.len()];
    let mut counter = vec![0usize; movable.len()];
    let mut best: Option<(u32, u64, Vec<usize>)> = None;
    loop {
        // Compteur à base mixte sur les formes de chaque dimension.
        let mut carry = true;
        for (c, (_, forms)) in counter.iter_mut().zip(&movable) {
            if !carry {
                break;
            }
            *c += 1;
            carry = *c >= *forms;
            if carry {
                *c = 0;
            }
        }
        if carry {
            break;
        }
        for (c, (i, _)) in counter.iter().zip(&movable) {
            if let Some(s) = state.get_mut(*i) {
                *s = *c;
            }
        }
        let estimate = group_size(dims, &state, org_visible);
        if estimate < u64::from(k_min) {
            continue;
        }
        let total: u32 = dims.iter().zip(&state).map(|(d, s)| loss(d, *s)).sum();
        let better = best
            .as_ref()
            .is_none_or(|(l, e, _)| total < *l || (total == *l && estimate > *e));
        if better {
            best = Some((total, estimate, state.clone()));
        }
    }
    let (total, estimate, state) = best?;
    let mut items: Vec<(usize, String)> = Vec::new();
    for (d, s) in dims.iter().zip(&state) {
        if *s == 0 {
            continue;
        }
        if let Some(form) = d.levels.get(*s).and_then(|l| l.form.clone()) {
            items.push((d.mention.value_id, form));
        }
    }
    let fix = if items.len() > 1 {
        Fix::GeneralizeMany { items }
    } else {
        let (value_id, to) = items.pop()?;
        Fix::Generalize { value_id, to }
    };
    Some(Candidate {
        fix,
        estimate,
        loss: total,
        rank: 0,
    })
}

/// Corrections de l'isolement, classées (voir l'en-tête du module).
fn isolation_fixes(dims: &[Dimension<'_>], orgs: &[&Mention], now: u64, k_min: u32) -> Vec<Fix> {
    let org_visible = !orgs.is_empty();
    let base = vec![0usize; dims.len()];
    let mut candidates: Vec<Candidate> = Vec::new();
    if let Some(c) = best_generalization(dims, org_visible, k_min) {
        candidates.push(c);
    }
    if group_size(dims, &base, org_visible) == NAMED_ROLE_GROUP
        && group_size(dims, &base, false) > NAMED_ROLE_GROUP
    {
        // Masquer une organisation : la règle tient tant qu'une autre reste.
        let others_remain = orgs.len() > 1;
        for o in orgs {
            candidates.push(Candidate {
                fix: Fix::Mask {
                    value_id: o.value_id,
                },
                estimate: group_size(dims, &base, others_remain),
                loss: ORG_MASK_LOSS,
                rank: 1,
            });
        }
    }
    for (i, d) in dims.iter().enumerate() {
        let mut state = base.clone();
        if let Some(s) = state.get_mut(i) {
            *s = d.levels.len().saturating_sub(1);
        }
        candidates.push(Candidate {
            fix: Fix::Mask {
                value_id: d.mention.value_id,
            },
            estimate: group_size(dims, &state, org_visible),
            loss: loss(d, d.levels.len().saturating_sub(1)),
            rank: 2,
        });
    }
    // Aucune correction qui laisse le groupe aussi petit.
    candidates.retain(|c| c.estimate > now);
    let k = u64::from(k_min);
    candidates.sort_by(|a, b| {
        let (ra, rb) = (a.estimate >= k, b.estimate >= k);
        rb.cmp(&ra).then_with(|| {
            if ra {
                a.loss
                    .cmp(&b.loss)
                    .then(a.rank.cmp(&b.rank))
                    .then(b.estimate.cmp(&a.estimate))
            } else {
                b.estimate.cmp(&a.estimate).then(a.rank.cmp(&b.rank))
            }
        })
    });
    let mut seen: HashSet<Fix> = HashSet::new();
    candidates
        .into_iter()
        .map(|c| c.fix)
        .filter(|f| seen.insert(f.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Évaluation
// ---------------------------------------------------------------------------

/// Évalue les textes : (bilans, échecs), un sujet par personne mentionnée.
pub(crate) fn assess_texts(units: &[TextUnit], k_min: u32) -> (Vec<SubjectReport>, Vec<Failure>) {
    let mut cache: LevelCache<'_> = HashMap::new();
    let mut subjects = Vec::new();
    let mut failures = Vec::new();
    for person in people(units) {
        let subject = SubjectRef::Person {
            value_id: person.id,
        };
        let mut passed = [true; 3];
        let mut fail = |criterion: Criterion, reason: FailureReason, fixes: Vec<Fix>| {
            if let Some(p) = passed.get_mut(criterion.index()) {
                *p = false;
            }
            failures.push(Failure {
                criterion,
                subject: subject.clone(),
                reason,
                fixes,
            });
        };

        // Isolement : identifiants directs encore exposés.
        let direct = person
            .names
            .iter()
            .chain(person.clues.iter().filter(|m| role(m.kind) == Role::Direct))
            .filter(|m| exposed(m));
        let mut isolable = false;
        for m in direct {
            isolable = true;
            fail(
                Criterion::Isolation,
                FailureReason::DirectIdentifier { kind: m.kind },
                vec![Fix::Mask {
                    value_id: m.value_id,
                }],
            );
        }

        // Isolement : taille estimée du groupe.
        let dims: Vec<Dimension<'_>> = person
            .clues
            .iter()
            .filter_map(|m| dimension(m, &mut cache))
            .collect();
        let orgs: Vec<&Mention> = person
            .clues
            .iter()
            .copied()
            .filter(|m| role(m.kind) == Role::Organization && exposed(m))
            .collect();
        let state = vec![0usize; dims.len()];
        let estimate = group_size(&dims, &state, !orgs.is_empty());
        if estimate < u64::from(k_min) {
            isolable = true;
            fail(
                Criterion::Isolation,
                FailureReason::SmallGroup { estimate },
                isolation_fixes(&dims, &orgs, estimate, k_min),
            );
        }

        // Recoupement : identifiants stables encore exposés.
        for m in person
            .clues
            .iter()
            .filter(|m| role(m.kind) == Role::Stable && exposed(m))
        {
            fail(
                Criterion::Linkage,
                FailureReason::StableIdentifier { kind: m.kind },
                vec![Fix::Mask {
                    value_id: m.value_id,
                }],
            );
        }

        // Déduction : donnée sensible ou événement lié à une personne isolable.
        if isolable {
            for m in person
                .clues
                .iter()
                .filter(|m| role(m.kind) == Role::Sensitive && exposed(m))
            {
                fail(
                    Criterion::Inference,
                    FailureReason::SensitiveLinked { kind: m.kind },
                    vec![Fix::Mask {
                        value_id: m.value_id,
                    }],
                );
            }
        }

        let clues = person
            .clues
            .iter()
            .filter(|m| exposed(m))
            .map(|m| (m.kind, m.value_id))
            .collect();
        subjects.push(SubjectReport {
            subject,
            clues,
            group_estimate: Some(estimate),
            passed,
        });
    }
    (subjects, failures)
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
