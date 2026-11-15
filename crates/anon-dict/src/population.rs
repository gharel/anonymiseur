//! Données de population embarquées : communes (populations légales de l'Insee,
//! recensement de la Nouvelle-Calédonie), répartition par âge, effectifs par
//! métier. Sources et millésimes : `data/SOURCES.md` ; préparation :
//! `tools/prepare_population.py`. Aucune lecture de fichier ni réseau à
//! l'exécution.
//!
//! Ces chiffres servent à estimer la taille du groupe auquel une personne
//! appartient (critère d'isolement) : un ordre de grandeur, pas un décompte.
//! La population d'un département, d'une province, d'une région ou d'un
//! territoire ([`area`], [`postal_area`]) est la somme de ses communes,
//! calculée par `build.rs`.

use crate::fold::fold;
use fst::{IntoStreamer, Map, Streamer};
use std::sync::OnceLock;

/// Territoire couvert par les données.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Territory {
    /// France (métropole, DROM, Saint-Pierre-et-Miquelon, Saint-Barthélemy, Saint-Martin).
    France,
    /// Nouvelle-Calédonie.
    NouvelleCaledonie,
}

impl Territory {
    pub const ALL: [Territory; 2] = [Territory::France, Territory::NouvelleCaledonie];

    /// Libellé du territoire (« France », « Nouvelle-Calédonie »).
    pub fn label(self) -> &'static str {
        match self {
            Territory::France => "France",
            Territory::NouvelleCaledonie => "Nouvelle-Calédonie",
        }
    }

    /// Code utilisé dans les fichiers de données.
    fn code(self) -> &'static str {
        match self {
            Territory::France => "FR",
            Territory::NouvelleCaledonie => "NC",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.code() == code)
    }
}

/// Une commune.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Place {
    /// Population municipale.
    pub population: u32,
    /// Département (France), province (Nouvelle-Calédonie) ou collectivité.
    pub area: &'static str,
    /// Région (France), « Nouvelle-Calédonie », ou collectivité.
    pub region: &'static str,
    pub territory: Territory,
}

/// Effectif d'un métier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Occupation {
    /// Effectif France entière (voir `data/SOURCES.md` : salariés en équivalent
    /// temps plein, ou professionnels de santé inscrits).
    pub count: u32,
    /// Catégorie plus large, accordée au genre de l'intitulé (« professionnelle de santé »).
    pub broader: &'static str,
}

static ZONES_TEXT: &str = include_str!("../data/population_zones.txt");
static TERRITORIES_TEXT: &str = include_str!("../data/population_territoires.txt");
static AGES_TEXT: &str = include_str!("../data/population_ages.txt");
static OCCUPATIONS_TEXT: &str = include_str!("../data/metiers.txt");
static COMMUNES_FST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/population.fst"));
/// Population de chaque zone, calculée par `build.rs` (code<TAB>population).
static ZONE_TOTALS_TEXT: &str =
    include_str!(concat!(env!("OUT_DIR"), "/population_zone_totaux.txt"));

/// Taille des données de population embarquées, en octets (contrôlée par un test).
pub const EMBEDDED_BYTES: usize = COMMUNES_FST.len()
    + ZONE_TOTALS_TEXT.len()
    + ZONES_TEXT.len()
    + TERRITORIES_TEXT.len()
    + AGES_TEXT.len()
    + OCCUPATIONS_TEXT.len();

/// Bits de la valeur du FST réservés au rang de la zone (voir `build.rs`).
const ZONE_BITS: u32 = 16;
/// Masque du rang de la zone.
const ZONE_MASK: u64 = (1 << ZONE_BITS) - 1;
/// Séparateur entre le nom et le rang de l'homonyme dans les clés du FST.
const KEY_SEPARATOR: u8 = 0;

/// Mots de tête d'une catégorie au féminin.
const FEMININE_HEADS: &[&str] = &["professionnelle", "dirigeante", "artisane"];

/// Lignes utiles d'un fichier de données, découpées sur les tabulations.
fn records(text: &'static str) -> impl Iterator<Item = Vec<&'static str>> {
    text.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').collect())
}

#[derive(Debug)]
struct Zone {
    /// Code Insee du département ou de la collectivité (« 54 », « 2A », « 974 »),
    /// ou code de province calédonienne (« NC-SUD »).
    code: &'static str,
    territory: Territory,
    area: &'static str,
    region: &'static str,
}

/// Zones, dans l'ordre du fichier (le rang est celui enregistré dans le FST).
fn zones() -> &'static [Option<Zone>] {
    static CELL: OnceLock<Vec<Option<Zone>>> = OnceLock::new();
    CELL.get_or_init(|| {
        records(ZONES_TEXT)
            .map(|f| match f.as_slice() {
                [code, territory, area, region, ..] => Some(Zone {
                    code,
                    territory: Territory::from_code(territory)?,
                    area,
                    region,
                }),
                _ => None,
            })
            .collect()
    })
}

fn communes() -> Option<&'static Map<&'static [u8]>> {
    static CELL: OnceLock<Option<Map<&'static [u8]>>> = OnceLock::new();
    CELL.get_or_init(|| Map::new(COMMUNES_FST).ok()).as_ref()
}

/// Communes de ce nom (comparaison pliée comme les dictionnaires) ; plusieurs
/// si homonymes, aucune si le nom est inconnu.
pub fn places(name: &str) -> Vec<Place> {
    let key = fold(name);
    let Some(map) = communes().filter(|_| !key.is_empty()) else {
        return Vec::new();
    };
    let mut lo = key.into_bytes();
    lo.push(KEY_SEPARATOR);
    let mut hi = lo.clone();
    hi.push(u8::MAX);
    let mut out = Vec::new();
    let mut stream = map.range().ge(&lo).le(&hi).into_stream();
    while let Some((_, value)) = stream.next() {
        let zone = usize::try_from(value & ZONE_MASK)
            .ok()
            .and_then(|i| zones().get(i))
            .and_then(Option::as_ref);
        let population = u32::try_from(value >> ZONE_BITS).ok();
        if let (Some(zone), Some(population)) = (zone, population) {
            out.push(Place {
                population,
                area: zone.area,
                region: zone.region,
                territory: zone.territory,
            });
        }
    }
    out
}

/// Population de chaque zone, dans l'ordre de `zones()`.
fn zone_totals() -> &'static [u64] {
    static CELL: OnceLock<Vec<u64>> = OnceLock::new();
    CELL.get_or_init(|| {
        let by_code: std::collections::HashMap<&str, u64> = records(ZONE_TOTALS_TEXT)
            .filter_map(|f| match f.as_slice() {
                [code, total, ..] => Some((*code, total.parse().ok()?)),
                _ => None,
            })
            .collect();
        zones()
            .iter()
            .map(|z| {
                z.as_ref()
                    .and_then(|z| by_code.get(z.code).copied())
                    .unwrap_or(0)
            })
            .collect()
    })
}

/// Population d'un ensemble de zones et son territoire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Area {
    /// Population municipale cumulée.
    pub population: u64,
    pub territory: Territory,
}

/// Zones retenues par `keep` : population cumulée et territoire de la
/// première ; `None` si aucune.
fn sum_zones(keep: impl Fn(&Zone) -> bool) -> Option<Area> {
    let mut out: Option<Area> = None;
    for (zone, pop) in zones().iter().zip(zone_totals()) {
        if let Some(z) = zone.as_ref().filter(|z| keep(z)) {
            let a = out.get_or_insert(Area {
                population: 0,
                territory: z.territory,
            });
            a.population = a.population.saturating_add(*pop);
        }
    }
    out
}

/// Département, province ou collectivité, région ou territoire, d'après son
/// libellé (celui que renvoie [`hierarchy::generalize`](crate::hierarchy::generalize)
/// pour un lieu : « Meurthe-et-Moselle », « province Sud », « Grand Est »,
/// « France »). Comparaison pliée. `None` si le libellé est inconnu.
pub fn area(label: &str) -> Option<Area> {
    let key = fold(label);
    if key.is_empty() {
        return None;
    }
    sum_zones(|z| fold(z.area) == key)
        .or_else(|| sum_zones(|z| fold(z.region) == key))
        .or_else(|| {
            Territory::ALL
                .into_iter()
                .find(|t| fold(t.label()) == key)
                .map(|t| Area {
                    population: territory_population(t),
                    territory: t,
                })
        })
}

/// Zone désignée par les deux premiers chiffres d'un code postal (« 54 » :
/// Meurthe-et-Moselle ; « 20 » : Corse ; « 97 » : départements et
/// collectivités d'outre-mer en 97x ; « 98 » : Nouvelle-Calédonie, seule
/// collectivité en 98x des données). `None` si inconnue.
pub fn postal_area(prefix: &str) -> Option<Area> {
    if prefix.len() != 2 || !prefix.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    sum_zones(|z| match prefix {
        "20" => z.code == "2A" || z.code == "2B",
        "98" => z.territory == Territory::NouvelleCaledonie,
        p => z.code.starts_with(p),
    })
}

/// Population municipale totale du territoire (somme des communes embarquées).
pub fn territory_population(t: Territory) -> u64 {
    records(TERRITORIES_TEXT)
        .find(|f| f.first() == Some(&t.code()))
        .and_then(|f| f.get(2).and_then(|v| v.parse().ok()))
        .unwrap_or(0)
}

/// Effectifs par âge en années révolues (indice = âge) ; le dernier âge de la
/// France est ouvert (« 105 ou plus »).
fn ages(t: Territory) -> &'static [u64] {
    static CELL: OnceLock<[Vec<u64>; 2]> = OnceLock::new();
    let all = CELL.get_or_init(|| {
        let (mut fr, mut nc) = (Vec::new(), Vec::new());
        for f in records(AGES_TEXT) {
            let [code, age, count, ..] = f.as_slice() else {
                continue;
            };
            let (Some(t), Ok(age), Ok(count)) = (
                Territory::from_code(code),
                age.parse::<usize>(),
                count.parse::<u64>(),
            ) else {
                continue;
            };
            let slot: &mut Vec<u64> = match t {
                Territory::France => &mut fr,
                Territory::NouvelleCaledonie => &mut nc,
            };
            if slot.len() <= age {
                slot.resize(age + 1, 0);
            }
            if let Some(s) = slot.get_mut(age) {
                *s = count;
            }
        }
        [fr, nc]
    });
    let [fr, nc] = all;
    match t {
        Territory::France => fr,
        Territory::NouvelleCaledonie => nc,
    }
}

/// Part de la population du territoire dont l'âge est dans `[from, to]` (années
/// révolues). Au-delà du dernier âge publié, la part est nulle, sauf pour la
/// classe ouverte de la France (« 105 ou plus ») comptée entière si `[from, to]`
/// contient 105 : une estimation prudente plutôt qu'une répartition inventée.
pub fn age_share(t: Territory, from: u32, to: u32) -> f64 {
    let counts = ages(t);
    let total: u64 = counts.iter().sum();
    if total == 0 || from > to {
        return 0.0;
    }
    let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else {
        return 0.0;
    };
    let last = counts.len().saturating_sub(1);
    let end = to.min(last);
    let part: u64 = counts.get(from..=end).map_or(0, |s| s.iter().sum());
    part as f64 / total as f64
}

/// Métiers triés par intitulé plié.
fn occupations() -> &'static [(&'static str, Occupation)] {
    static CELL: OnceLock<Vec<(&'static str, Occupation)>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut v: Vec<(&'static str, Occupation)> = records(OCCUPATIONS_TEXT)
            .filter_map(|f| match f.as_slice() {
                [title, count, broader, ..] => Some((
                    *title,
                    Occupation {
                        count: count.parse().ok()?,
                        broader,
                    },
                )),
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v
    })
}

fn occupation_folded(folded: &str) -> Option<Occupation> {
    let all = occupations();
    all.binary_search_by(|(k, _)| (*k).cmp(folded))
        .ok()
        .and_then(|i| all.get(i))
        .map(|(_, o)| *o)
}

/// Métier d'un intitulé (« infirmière », « Directrice financière »). À défaut
/// d'intitulé exact, le plus long intitulé connu qui commence l'intitulé donné
/// (« infirmière de bloc » → « infirmière »).
pub fn occupation(title: &str) -> Option<Occupation> {
    let folded = fold(title);
    if folded.is_empty() {
        return None;
    }
    if let Some(o) = occupation_folded(&folded) {
        return Some(o);
    }
    folded
        .char_indices()
        .rev()
        .filter(|(_, c)| *c == ' ')
        .find_map(|(i, _)| folded.get(..i).and_then(occupation_folded))
}

/// Vrai si une catégorie plus large est au féminin (« professionnelle de santé »).
pub(crate) fn is_feminine(broader: &str) -> bool {
    broader
        .split(' ')
        .next()
        .is_some_and(|w| FEMININE_HEADS.contains(&w))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Taille maximale des données de population embarquées.
    const MAX_EMBEDDED_BYTES: usize = 1024 * 1024;

    #[test]
    fn embedded_size_is_bounded() {
        let total = std::hint::black_box(EMBEDDED_BYTES);
        assert!(total <= MAX_EMBEDDED_BYTES, "{total} octets");
        assert!(total > 0);
    }

    #[test]
    fn communes_of_both_territories() {
        let farino = places("Farino");
        assert_eq!(farino.len(), 1);
        let f = farino[0];
        assert_eq!(f.area, "province Sud");
        assert_eq!(f.region, "Nouvelle-Calédonie");
        assert_eq!(f.territory, Territory::NouvelleCaledonie);
        assert!(f.population > 0 && f.population < 1_000, "{}", f.population);

        let nancy = places("NANCY");
        assert_eq!(nancy.len(), 1);
        assert_eq!(nancy[0].area, "Meurthe-et-Moselle");
        assert_eq!(nancy[0].region, "Grand Est");
        assert_eq!(nancy[0].territory, Territory::France);
        assert!(nancy[0].population > 50_000);

        // Article facultatif, accents et casse indifférents.
        for n in [
            "Le Mont-Dore",
            "Nouméa",
            "noumea",
            "L'Île-des-Pins",
            "Île des Pins",
            "Païta",
        ] {
            assert!(
                places(n)
                    .iter()
                    .any(|p| p.territory == Territory::NouvelleCaledonie),
                "{n}"
            );
        }
        // Paris, Lyon, Marseille : somme des arrondissements municipaux.
        let paris = places("Paris");
        assert_eq!(paris.len(), 1);
        assert!(paris[0].population > 1_000_000);
        assert!(places("Atlantis-sur-Mer").is_empty());
        assert!(places("").is_empty());
    }

    #[test]
    fn homonyms_are_all_returned() {
        let sd = places("Saint-Denis");
        assert!(sd.len() >= 2, "{sd:?}");
        assert!(sd.iter().any(|p| p.area == "Seine-Saint-Denis"));
        assert!(sd.iter().any(|p| p.area == "La Réunion"));
        // « Mont-Dore » : commune du Puy-de-Dôme et commune calédonienne.
        let md = places("Mont-Dore");
        assert!(md.iter().any(|p| p.territory == Territory::France));
        assert!(
            md.iter()
                .any(|p| p.territory == Territory::NouvelleCaledonie)
        );
    }

    #[test]
    fn all_new_caledonia_communes_are_present() {
        let n = zones()
            .iter()
            .flatten()
            .filter(|z| z.territory == Territory::NouvelleCaledonie)
            .count();
        assert_eq!(n, 3, "trois provinces");
        let total: u64 = [
            "Belep",
            "Bouloupari",
            "Bourail",
            "Canala",
            "Dumbéa",
            "Farino",
            "Hienghène",
            "Houaïlou",
            "L'Île-des-Pins",
            "Kaala-Gomen",
            "Koné",
            "Koumac",
            "La Foa",
            "Lifou",
            "Maré",
            "Moindou",
            "Le Mont-Dore",
            "Nouméa",
            "Ouégoa",
            "Ouvéa",
            "Païta",
            "Poindimié",
            "Ponérihouen",
            "Pouébo",
            "Pouembout",
            "Poum",
            "Poya",
            "Sarraméa",
            "Thio",
            "Touho",
            "Voh",
            "Yaté",
            "Kouaoua",
        ]
        .iter()
        .map(|n| {
            let nc: Vec<Place> = places(n)
                .into_iter()
                .filter(|p| p.territory == Territory::NouvelleCaledonie)
                .collect();
            assert_eq!(nc.len(), 1, "{n}");
            u64::from(nc[0].population)
        })
        .sum();
        assert_eq!(total, territory_population(Territory::NouvelleCaledonie));
    }

    #[test]
    fn area_populations() {
        let pop = |label: &str| area(label).map(|a| a.population);
        // Province Sud : somme de ses communes, chacune une seule fois (« Le
        // Mont-Dore » et « Mont-Dore », « Bouloupari » et « Boulouparis »).
        let sud = pop("province Sud").unwrap();
        let nord = pop("Province Nord").unwrap();
        let iles = pop("province des îles Loyauté").unwrap();
        let nc = territory_population(Territory::NouvelleCaledonie);
        assert_eq!(sud + nord + iles, nc);
        assert!(sud > nord && nord > iles, "{sud} {nord} {iles}");
        assert_eq!(
            area("province Sud").map(|a| a.territory),
            Some(Territory::NouvelleCaledonie)
        );
        assert_eq!(pop("Nouvelle-Calédonie"), Some(nc));
        // France : la somme des zones redonne la population totale, à quelques
        // homonymes près (deux communes d'une même zone de même population).
        let fr = territory_population(Territory::France);
        let sum: u64 = zones()
            .iter()
            .zip(zone_totals())
            .filter(|(z, _)| z.as_ref().is_some_and(|z| z.territory == Territory::France))
            .map(|(_, p)| *p)
            .sum();
        assert!(sum <= fr && fr - sum < fr / 10_000, "{sum} / {fr}");
        assert_eq!(pop("France"), Some(fr));
        let mm = pop("Meurthe-et-Moselle").unwrap();
        let ge = pop("Grand Est").unwrap();
        assert!(mm > 500_000 && mm < 1_000_000, "{mm}");
        assert!(ge > mm && ge < fr, "{ge}");
        assert_eq!(
            area("Grand Est").map(|a| a.territory),
            Some(Territory::France)
        );
        // Paris compté une fois (pas ses arrondissements en plus).
        let paris = pop("Paris").unwrap();
        assert!(paris > 2_000_000 && paris < 2_300_000, "{paris}");
        // Chaque niveau englobe la commune.
        for name in ["Farino", "Nancy", "Bourail", "Metz"] {
            let p = u64::from(places(name)[0].population);
            for level in 1..=3 {
                let label =
                    crate::hierarchy::generalize(anon_core::EntityKind::Lieu, name, level).unwrap();
                assert!(pop(&label).unwrap() >= p, "{name} {label}");
            }
        }
        assert_eq!(area("Atlantide"), None);
        assert_eq!(area(""), None);
    }

    #[test]
    fn postal_areas() {
        assert_eq!(postal_area("54"), area("Meurthe-et-Moselle"));
        assert_eq!(
            postal_area("98"),
            Some(Area {
                population: territory_population(Territory::NouvelleCaledonie),
                territory: Territory::NouvelleCaledonie,
            })
        );
        let corse = postal_area("20").unwrap().population;
        assert!(corse > 300_000 && corse < 400_000, "{corse}");
        assert!(postal_area("97").unwrap().population > 1_000_000);
        assert_eq!(postal_area("5"), None);
        assert_eq!(postal_area("5a"), None);
        assert_eq!(postal_area("00"), None);
    }

    #[test]
    fn territory_totals() {
        let fr = territory_population(Territory::France);
        let nc = territory_population(Territory::NouvelleCaledonie);
        assert!(fr > 60_000_000 && fr < 80_000_000, "{fr}");
        assert!(nc > 200_000 && nc < 400_000, "{nc}");
    }

    #[test]
    fn age_shares() {
        for t in Territory::ALL {
            let all = age_share(t, 0, 200);
            assert!((all - 1.0).abs() < 1e-9, "{t:?} {all}");
            let one = age_share(t, 62, 62);
            assert!(one > 0.0 && one < 0.05, "{t:?} {one}");
            let band = age_share(t, 60, 69);
            assert!(band > one && band < 0.3, "{t:?} {band}");
            assert_eq!(age_share(t, 70, 60), 0.0);
            assert_eq!(age_share(t, 150, 200), 0.0);
        }
        // Somme des tranches = ensemble.
        let parts: f64 = (0..=20u32)
            .map(|i| age_share(Territory::France, i * 10, i * 10 + 9))
            .sum();
        assert!((parts - 1.0).abs() < 1e-9);
    }

    #[test]
    fn occupations_lookup() {
        let inf = occupation("Infirmière").unwrap();
        assert_eq!(inf.broader, "professionnelle de santé");
        assert!(inf.count > 100_000);
        assert_eq!(
            occupation("infirmier").unwrap().broader,
            "professionnel de santé"
        );
        assert_eq!(occupation("infirmier").unwrap().count, inf.count);
        let df = occupation("directrice financière").unwrap();
        assert!(is_feminine(df.broader));
        assert!(!is_feminine(occupation("gérant").unwrap().broader));
        // Préfixe connu le plus long.
        assert_eq!(occupation("infirmière de bloc opératoire"), Some(inf));
        assert!(occupation("astronaute").is_none());
        assert!(occupation("").is_none());
    }

    #[test]
    fn at_least_a_hundred_occupations() {
        // Un effectif par métier (les intitulés d'un même métier le partagent).
        let distinct: std::collections::HashSet<u32> =
            occupations().iter().map(|(_, o)| o.count).collect();
        assert!(occupations().len() >= 200, "{}", occupations().len());
        assert!(distinct.len() >= 100, "{}", distinct.len());
    }

    #[test]
    fn data_files_are_well_formed() {
        let titles: Vec<&str> = records(OCCUPATIONS_TEXT)
            .map(|f| f.first().copied().unwrap_or(""))
            .collect();
        for w in titles.windows(2) {
            assert!(w[0] < w[1], "{} / {}", w[0], w[1]);
        }
        for t in &titles {
            assert_eq!(&fold(t), t, "intitulé non plié");
        }
        let communes = include_str!("../data/population_communes.txt");
        let mut previous: Option<&str> = None;
        for f in records(communes) {
            let name = f[0];
            assert_eq!(fold(name), name, "commune non pliée");
            assert!(f[1].parse::<u32>().is_ok());
            if let Some(p) = previous {
                assert!(p <= name, "{p} / {name}");
            }
            previous = Some(name);
        }
        assert!(zones().iter().all(Option::is_some));
    }
}
