//! Données sensibles (RGPD, art. 9 et 10 : santé, opinions, religion, syndicat,
//! orientation sexuelle, origine, données génétiques et biométriques,
//! condamnations) et événements personnels (licenciement, divorce, décès d'un
//! proche, procédure…), en français et en anglais.
//!
//! Lexiques : `data/sensibles.txt` (sources dans `data/SOURCES.md`). Règles de
//! contexte : [`RULES`], ci-dessous.
//!
//! **La précision d'abord.** Un mot du lexique seul n'est pas une donnée
//! sensible d'une personne (« le diabète touche 10 % de la population ») ; une
//! règle exige un indice qui le rattache à quelqu'un :
//! - un participe ou un verbe (« atteinte d'un cancer », « suivi pour une
//!   dépression », « condamné pour vol », « adhérente de la CFDT ») : la valeur
//!   détectée comprend l'indice, pour que la balise cache aussi le fait ;
//! - un possessif (« son diabète », « sa démission ») ou un verbe d'état devant
//!   un qualificatif (« est diabétique ») : la valeur est le terme seul ;
//! - une locution qui désigne une situation personnelle (« en arrêt maladie »,
//!   « délégué syndical », « de confession musulmane »).
//!
//! Les formes au pluriel (« les patients diabétiques ») ne sont pas reconnues.
//! Une proposition générale écarte la détection : début en « si », « lorsque »,
//! « tout », « chaque », « if », « when »…, ou tournure « en cas de », « pour
//! tout », « tout salarié », « les salariés », « employees »… avant la valeur (voir
//! [`generic_clause`]).
//!
//! Classement : un arrêt maladie, un congé maladie ou un accident du travail
//! révèle un état de santé (`Sante`, détecté dans les deux profils) ; un arrêt
//! de travail sans motif est un événement (`Evenement`, profil « Renforcé »).
//!
//! La recherche se fait sur le texte plié (minuscules, sans accents, espaces
//! réduites, « œ » écrit « oe »), avec une table de correspondance vers les
//! octets d'origine : les motifs s'écrivent sans accents ni majuscules.

use std::collections::HashMap;
use std::sync::LazyLock;

use anon_core::EntityKind;
use regex::{Regex, RegexBuilder, RegexSet, RegexSetBuilder};

use crate::fold::fold_char;
use crate::rules::{Out, push};

// ---------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------

/// Indice de contexte suivi d'un terme ou d'une précision (« atteinte d'un
/// cancer », « condamné à six mois de prison »).
const CONF_CUE: f32 = 0.85;
/// Locution qui désigne à elle seule une situation personnelle (« en arrêt
/// maladie », « délégué syndical »).
const CONF_PHRASE: f32 = 0.8;
/// Possessif ou verbe d'état devant un terme du lexique (« son diabète », « est
/// diabétique »).
const CONF_ANCHORED: f32 = 0.75;
/// Caractères (texte plié) examinés avant une correspondance pour le contexte
/// exigé ou refusé par une règle.
const BEFORE_WINDOW: usize = 40;
/// Caractères (texte plié) examinés après une correspondance.
const AFTER_WINDOW: usize = 40;

/// Lexiques embarqués.
static LEXICON_TEXT: &str = include_str!("../data/sensibles.txt");

// ---------------------------------------------------------------------------
// Lexiques
// ---------------------------------------------------------------------------

/// Classe d'une forme du lexique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Class {
    /// Nom ou locution, reconnu après un indice (`{T}` dans les motifs).
    Term,
    /// Qualificatif d'une personne (`{A}`).
    Adjective,
    /// Partie du corps, après « opéré de » (`{O}`).
    Organ,
    /// Membre de la famille (`{R}`).
    Relative,
}

impl Class {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "terme" => Some(Class::Term),
            "adjectif" => Some(Class::Adjective),
            "organe" => Some(Class::Organ),
            "proche" => Some(Class::Relative),
            _ => None,
        }
    }
}

/// Formes pliées par (type, classe), sans doublon.
fn lexicon() -> &'static HashMap<(EntityKind, Class), Vec<String>> {
    static CELL: LazyLock<HashMap<(EntityKind, Class), Vec<String>>> =
        LazyLock::new(|| parse_lexicon(LEXICON_TEXT));
    &CELL
}

fn parse_lexicon(text: &str) -> HashMap<(EntityKind, Class), Vec<String>> {
    let mut out: HashMap<(EntityKind, Class), Vec<String>> = HashMap::new();
    let mut section: Option<(EntityKind, Class)> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let mut parts = header.split_whitespace();
            section = match (parts.next(), parts.next()) {
                (Some(kind), Some(class)) => {
                    EntityKind::from_prefix(kind).zip(Class::from_name(class))
                }
                _ => None,
            };
            continue;
        }
        let Some(key) = section else {
            continue;
        };
        let form = fold_text(line);
        if form.is_empty() {
            continue;
        }
        let forms = out.entry(key).or_default();
        if !forms.contains(&form) {
            forms.push(form);
        }
    }
    out
}

/// Alternance d'expression régulière des formes d'un lexique, les plus longues
/// d'abord ; un motif qui ne correspond à rien si le lexique est vide.
fn alternation(kind: EntityKind, class: Class) -> String {
    let mut forms: Vec<&String> = lexicon()
        .get(&(kind, class))
        .map(|v| v.iter().collect())
        .unwrap_or_default();
    if forms.is_empty() {
        return r"[^\s\S]".to_string();
    }
    forms.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    let escaped: Vec<String> = forms.iter().map(|f| regex::escape(f)).collect();
    format!("(?:{})", escaped.join("|"))
}

// ---------------------------------------------------------------------------
// Pliage avec correspondance des positions
// ---------------------------------------------------------------------------

/// Texte plié et, pour chaque octet plié, l'intervalle d'octets du caractère
/// d'origine.
struct Folded {
    text: String,
    src: Vec<(usize, usize)>,
}

impl Folded {
    fn new(text: &str) -> Self {
        let mut out = Folded {
            text: String::with_capacity(text.len()),
            src: Vec::with_capacity(text.len()),
        };
        for (i, c) in text.char_indices() {
            let span = (i, i + c.len_utf8());
            if c == '\n' || c == '\r' {
                out.push('\n', span);
            } else if c.is_whitespace() {
                if !out.text.ends_with([' ', '\n']) {
                    out.push(' ', span);
                }
            } else if c.is_ascii() {
                out.push(c.to_ascii_lowercase(), span);
            } else {
                for f in fold_char(c) {
                    match f {
                        'œ' => out.push_str("oe", span),
                        'æ' => out.push_str("ae", span),
                        'ß' => out.push_str("ss", span),
                        '\u{2011}' | '\u{2010}' => out.push('-', span),
                        other => out.push(other, span),
                    }
                }
            }
        }
        out
    }

    fn push(&mut self, c: char, span: (usize, usize)) {
        self.text.push(c);
        for _ in 0..c.len_utf8() {
            self.src.push(span);
        }
    }

    fn push_str(&mut self, s: &str, span: (usize, usize)) {
        for c in s.chars() {
            self.push(c, span);
        }
    }

    /// Intervalle d'origine d'un intervalle du texte plié.
    fn source(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        let s = self.src.get(start)?.0;
        let e = self.src.get(end.checked_sub(1)?)?.1;
        (s < e).then_some((s, e))
    }
}

/// Pliage d'une forme du lexique, comme le texte analysé.
fn fold_text(s: &str) -> String {
    Folded::new(s).text.trim().to_string()
}

// ---------------------------------------------------------------------------
// Règles
// ---------------------------------------------------------------------------

/// Contexte exigé ou refusé juste avant ou juste après une correspondance (texte
/// plié ; motif ancré en fin de fenêtre avant, en début de fenêtre après).
#[derive(Debug, Clone, Copy)]
enum Ctx {
    Any,
    Deny(&'static str),
    Require(&'static str),
}

/// Une règle : type, motif (texte plié ; groupe `v` = la valeur, sinon toute
/// la correspondance), confiance, contexte avant et après.
#[derive(Debug, Clone, Copy)]
struct Rule {
    kind: EntityKind,
    pattern: &'static str,
    confidence: f32,
    before: Ctx,
    after: Ctx,
}

const fn rule(kind: EntityKind, pattern: &'static str, confidence: f32) -> Rule {
    Rule {
        kind,
        pattern,
        confidence,
        before: Ctx::Any,
        after: Ctx::Any,
    }
}

const fn rule_before(
    kind: EntityKind,
    pattern: &'static str,
    confidence: f32,
    before: Ctx,
) -> Rule {
    Rule {
        kind,
        pattern,
        confidence,
        before,
        after: Ctx::Any,
    }
}

const fn rule_after(kind: EntityKind, pattern: &'static str, confidence: f32, after: Ctx) -> Rule {
    Rule {
        kind,
        pattern,
        confidence,
        before: Ctx::Any,
        after,
    }
}

// Morceaux de motifs, substitués avant compilation.
const PIECES: &[(&str, &str)] = &[
    (
        "{POSS}",
        r"(?:son|sa|ses|leur|leurs|mon|ma|mes|notre|nos|votre|vos|his|her|their|my|our|your)",
    ),
    (
        "{COP}",
        r"(?:est|etait|etant|sera|serait|fut|a ete|avait ete|devenue?|reste|restait|demeure|is|was|being|became|has become|had become|has been|had been)(?: (?:aussi|egalement|toujours|desormais|depuis peu|ouvertement|also|still|now|openly))?",
    ),
    ("{DE}", r"(?:de la |de l'|du |des |d'une |d'un |de |d')"),
    (
        "{DET}",
        r"(?:une |un |le |la |les |l'|des |du |de la |de l'|d'une |d'un |de |d'|a |an |the )?",
    ),
    (
        "{NUM}",
        r"(?:\d+(?: \d{3})*|un|une|deux|trois|quatre|cinq|six|sept|huit|neuf|dix|onze|douze|quinze|dix-huit|vingt|trente|quarante|cinquante|one|two|three|four|five|six|seven|eight|nine|ten|twelve|eighteen|twenty)",
    ),
    (
        "{MONSIEUR}",
        r"(?:mme|mlle|monsieur|madame|mademoiselle|mrs|mr|ms|dr|m)",
    ),
    (
        "{PEINE}",
        r"(?:(?:une peine de |une peine d'|un emprisonnement de )?{NUM} (?:ans?|mois|jours|semaines) (?:de prison|d'emprisonnement|de reclusion(?: criminelle)?|de travaux d'interet general)(?: avec sursis| ferme| dont {NUM} (?:ans?|mois) avec sursis)?|une peine (?:de prison |d'emprisonnement )?(?:avec sursis|ferme)|une (?:forte )?amende(?: de {NUM} (?:euros?|eur|xpf|f cfp|francs?(?: cfp)?))?|{NUM} (?:euros?|eur|xpf|f cfp|francs?(?: cfp)?) d'amende|(?:la )?(?:prison|reclusion) a perpetuite|des travaux d'interet general|un travail d'interet general|un stage de citoyennete|un suivi socio-judiciaire)",
    ),
];

/// Verbe modal devant un participe : une possibilité, pas un fait (« peut être
/// condamné », « encourt »).
const MODAL: &str = r"(?:peut|peuvent|pourra|pourront|pourrait|pourraient|doit|doivent|devra|devront|devrait|risque|risquent|encourt|encourent|can|may|might|could|must|will|would|etre|be)(?: etre| be)? $";
/// Article ou déterminant général devant un nom de fonction (« le délégué
/// syndical est désigné… »).
const GENERAL_ARTICLE: &str =
    r"(?:le|la|les|un|une|chaque|tout|toute|du|au|des|aux|the|a|an|any|each|every) $";
/// Objets et notions qui peuvent être « d'origine X » sans désigner une personne.
const ORIGIN_OBJECTS: &str = r"(?:produits?|appellation|denrees?|viandes?|vins?|marchandises?|fruits?|legumes?|bois|materiaux|materiau|matieres?|fonds|capitaux|cles?|pieces?|biens|articles?|eaux?|aliments?|ingredients?|especes?|plantes?|animaux|animal|langues?|mots?|termes?|noms?|marques?|voitures?|vehicules?|recettes?|plats?|cuisine|musique|danses?|traditions?|coutumes?|races?|chiens?|chats?|chevaux|cheval|graines?|semences?|the|cafe|riz|epices?|tissus?|motifs?|objets?|oeuvres?|style|art|products?|goods|food|wine|words?|names?|dishes|cuisine)(?: [^ ]+){0,2} $";
/// Personne décrite « de type X ».
const PERSON_NOUNS: &str = r"(?:homme|femme|individu|personne|jeune|garcon|fille|enfant|suspecte?|conducteur|conductrice|cliente?|candidate?|patiente?|passagere?|passager|agresseur|victime|adolescente?|monsieur|madame)(?: [^ ]+){0,2} $";
/// Un « syndicat » qui n'est pas un syndicat de salariés.
const NOT_LABOR_UNION: &str = r"^ (?:(?:de |des |du |d')(?:coproprietaires?|copropriete|eaux|communes|initiative|agglomeration|transports?|energie|electricite|gestion|producteurs?|vins?|eleveurs?)|intercommunal|mixte|d'initiative)";
/// Une « union » qui n'est pas un syndicat.
const NOT_TRADE_UNION: &str = r"(?:european|african|soviet|credit|customs|postal|civil|monetary|economic|student|students') $";

use EntityKind::{
    Biometrie, Evenement, Genetique, Judiciaire, Opinion, Orientation, Origine, Religion, Sante,
    Syndicat,
};

/// Règles de contexte, par type.
const RULES: &[Rule] = &[
    // --- Santé ------------------------------------------------------------
    rule(
        Sante,
        r"(?P<v>(?:atteinte?|souffrante?|souffre|souffrait|a souffert|diagnostiquee?|victime|porteuse|porteur|soignee?|traitee?|suivie?|hospitalisee?|operee?|decedee? des suites|morte? des suites) (?:de la |de l'|du |des |d'une |d'un |de |d'|pour une |pour un |pour des |pour la |pour le |pour l'|pour |avec une |avec un |avec |contre une |contre un |contre |une |un )?{T})",
        CONF_CUE,
    ),
    rule(
        Sante,
        r"(?P<v>(?:diagnosed with|suffers from|suffering from|suffered from|treated for|being treated for|hospitali[sz]ed (?:for|with)|died of|died from|living with|battling|recovering from|undergoing) (?:a |an |the )?{T})",
        CONF_CUE,
    ),
    rule(
        Sante,
        r"(?P<v>(?:en|sous) traitement (?:pour|contre) {DET}{T})",
        CONF_CUE,
    ),
    rule(
        Sante,
        r"(?P<v>sous (?:insuline|insulinotherapie|antidepresseurs|anxiolytiques|neuroleptiques|methadone|tritherapie|chimiotherapie|radiotherapie|dialyse|hemodialyse|morphine|lithium))",
        CONF_CUE,
    ),
    rule(
        Sante,
        r"(?P<v>en (?:cure de |seance de |seances de )?(?:chimiotherapie|radiotherapie|dialyse|hemodialyse|reeducation|desintoxication|soins palliatifs|hopital psychiatrique|maison de repos|convalescence))",
        CONF_CUE,
    ),
    rule(
        Sante,
        r"(?P<v>(?:on (?:insulin|antidepressants|dialysis|chemotherapy|methadone))|in (?:rehab|chemotherapy|palliative care))",
        CONF_CUE,
    ),
    rule(Sante, r"(?P<v>operee? {DE}{O})", CONF_CUE),
    rule(Sante, r"(?P<v>hospitalisee?|hospitali[sz]ed)", CONF_PHRASE),
    rule(
        Sante,
        r"(?P<v>(?:admise?|admitted) (?:aux urgences|a l'hopital|en reanimation|en soins intensifs|en psychiatrie|to hospital|to the emergency room|to intensive care))",
        CONF_CUE,
    ),
    rule(
        Sante,
        r"(?:en|{POSS}|cet|ce|nouvel|nouveau|prolongation d'|prolongation de l') ?(?P<v>arrets? maladie|conges? (?:de )?maladie|conge de longue (?:maladie|duree)|mi-temps therapeutique)",
        CONF_PHRASE,
    ),
    rule(
        Sante,
        r"(?P<v>arret maladie|conge maladie) (?:de |du |d')?{MONSIEUR}",
        CONF_PHRASE,
    ),
    rule(
        Sante,
        r"(?P<v>(?:declaree?|reconnue?) inapte(?: (?:a son poste|au travail|a tout poste))?|reconnue? travailleu(?:r|se) handicapee?)",
        CONF_CUE,
    ),
    rule(
        Sante,
        r"(?:{POSS}|beneficiaire d'une|beneficiaire de la|titulaire d'une|titulaire de la) (?P<v>rqth|reconnaissance de la qualite de travailleur handicape)",
        CONF_ANCHORED,
    ),
    rule(
        Sante,
        r"(?:pour|for) (?P<v>(?:des |de )?raisons? (?:de sante|medicales?)|health reasons|medical reasons)",
        CONF_PHRASE,
    ),
    rule(
        Sante,
        r"(?:on|{POSS}) (?P<v>sick leave|medical leave|sick note)",
        CONF_PHRASE,
    ),
    rule(
        Sante,
        r"{POSS} (?:(?:crises?|problemes?|troubles?|traitement|suivi|soins|symptomes|douleurs?|problems?|symptoms|treatment) (?:d'|de |du |de la |contre |pour |for |of )(?:l'|la |le |une? |the )?)?(?P<v>{T})",
        CONF_ANCHORED,
    ),
    rule(Sante, r"{COP} (?P<v>{A})", CONF_ANCHORED),
    rule(
        Sante,
        r"(?P<v>enceinte de {NUM} (?:mois|semaines))",
        CONF_PHRASE,
    ),
    // --- Condamnations et infractions (art. 10) ---------------------------
    rule_before(
        Judiciaire,
        r"(?P<v>condamnee?(?: (?:en (?:premiere instance|appel)|definitivement|a nouveau|deja|par (?:le |la )(?:tribunal|cour)(?: (?:correctionnel|d'appel|d'assises|de police|criminelle))?(?: (?:de|d') ?[a-z'-]{2,30})?))? (?:pour {DET}{T}|a {PEINE}))",
        CONF_CUE,
        Ctx::Deny(MODAL),
    ),
    rule(
        Judiciaire,
        r"(?P<v>(?:reconnue?|declaree?) coupable(?: {DE}{DET}{T})?)",
        CONF_CUE,
    ),
    rule_before(
        Judiciaire,
        r"(?P<v>(?:poursuivie?|inculpee?|mise? en examen|interpellee?|arretee?|jugee?) (?:(?:en justice|penalement) )?(?:pour|de|d') ?{DET}{T})",
        CONF_CUE,
        Ctx::Deny(MODAL),
    ),
    rule_before(
        Judiciaire,
        r"(?P<v>mise? en examen)",
        CONF_PHRASE,
        Ctx::Deny(r"(?:la|une|toute|cette|de la|d'une|sa|son|leur|peut etre|etre|be) $"),
    ),
    rule_before(
        Judiciaire,
        r"(?P<v>(?:placee? |maintenue? )?en garde a vue)",
        CONF_PHRASE,
        Ctx::Deny(MODAL),
    ),
    rule_before(
        Judiciaire,
        r"(?P<v>(?:placee? |est |reste |demeure |etait )?(?:sous controle judiciaire|en detention provisoire|en liberte conditionnelle|sous bracelet electronique|sous surveillance electronique|en sursis probatoire))",
        CONF_PHRASE,
        Ctx::Deny(MODAL),
    ),
    rule_before(
        Judiciaire,
        r"(?P<v>(?:incarceree?|ecrouee?)(?: (?:a la maison d'arret|au centre penitentiaire|au camp-est|au camp est|a la prison|en prison|pour {DET}{T}))?|(?:detenue?|emprisonnee?) (?:a la maison d'arret|au centre penitentiaire|au camp-est|au camp est|a la prison|en prison|pour {DET}{T}))",
        CONF_CUE,
        Ctx::Deny(MODAL),
    ),
    rule(
        Judiciaire,
        r"{POSS} (?P<v>casier judiciaire|condamnation|condamnations|peine de prison|peine d'emprisonnement|garde a vue|mise en examen|incarceration|bracelet electronique|antecedents judiciaires|criminal record|criminal convictions|prison sentence)",
        CONF_ANCHORED,
    ),
    rule(
        Judiciaire,
        r"(?P<v>casier judiciaire) (?:de |du |d')?{MONSIEUR}",
        CONF_ANCHORED,
    ),
    rule(
        Judiciaire,
        r"(?P<v>(?:a|avait|ont) (?:purge|effectue|execute) (?:une |sa |leur )?peine(?: de prison| d'emprisonnement)?)",
        CONF_CUE,
    ),
    rule(
        Judiciaire,
        r"(?P<v>(?:fait|faisant|a fait|fera|avait fait) l'objet d'une? (?:plainte|enquete (?:penale|de police|judiciaire)|information judiciaire|poursuite|poursuites|procedure penale|signalement au procureur)(?: pour {DET}{T})?)",
        CONF_CUE,
    ),
    rule(
        Judiciaire,
        r"(?P<v>(?:a |avait |ont )?(?:porte|depose) (?:une )?plainte pour {DET}{T})",
        CONF_CUE,
    ),
    rule(
        Judiciaire,
        r"(?P<v>(?:convicted|found guilty) (?:of|for) {DET}{T}|(?:arrested|charged|prosecuted|indicted|jailed|imprisoned) (?:for|with) {DET}{T}|pleaded guilty(?: to {DET}{T})?)",
        CONF_CUE,
    ),
    rule(
        Judiciaire,
        r"(?P<v>sentenced to (?:{NUM} (?:years?|months?|weeks?|days?)(?: in prison| imprisonment| of community service)?|life imprisonment|community service|a suspended sentence))",
        CONF_CUE,
    ),
    rule(
        Judiciaire,
        r"(?:is|was|been|remains) (?P<v>in police custody|on parole|behind bars)",
        CONF_PHRASE,
    ),
    // --- Opinions politiques ou philosophiques ----------------------------
    rule(
        Opinion,
        r"(?P<v>(?:militante?|adherente?|membre|sympathisante?|encartee?|electeur|electrice|partisane?|candidate?|elue?|deputee?|senatrice|senateur|conseillere? (?:municipale?|regionale?|departementale?|provinciale?|territoriale?)|responsable|cadre|dirigeante?|porte-parole|secretaire (?:generale?|federale?|nationale?|departementale?)|tresoriere?|presidente?|fondatrice|fondateur)(?: (?:active?|historique|de longue date|locale?|nationale?))? (?:du |de la |de l'|des |d'|de |au |a la |a l'|chez |sur la liste (?:du |de la |de l'|des |d'|de )?)(?:parti )?{T})",
        CONF_CUE,
    ),
    rule(
        Opinion,
        r"(?P<v>(?:militante?|sympathisante?) (?:{A}|politique))",
        CONF_CUE,
    ),
    rule(Opinion, r"{COP} (?P<v>{A})", CONF_ANCHORED),
    rule(
        Opinion,
        r"(?P<v>(?:a vote|vote|votait|votera|voterait|a soutenu|soutient|soutenait|voted|votes|voting|supports|supported)(?: pour| for)? (?:le |la |les |l'|the )?{T})",
        CONF_CUE,
    ),
    rule(
        Opinion,
        r"(?P<v>(?:membre|initiee?|frere|soeur) (?:d'une |de la |de l'|du |de )?(?:loge(?: maconnique)?|grand orient(?: de france)?|grande loge(?: de france| nationale francaise)?|franc-maconnerie))",
        CONF_CUE,
    ),
    rule(
        Opinion,
        r"{POSS} (?P<v>(?:engagement|militantisme|activisme|appartenance|affiliation|orientation|opinions?|convictions?|sensibilite|idees) politiques?|(?:engagement|appartenance|adhesion) (?:a la |au |a l'|aux )(?:franc-maconnerie|{T})|political (?:views|opinions|affiliation|beliefs|activism)|party (?:membership|affiliation))",
        CONF_ANCHORED,
    ),
    rule(
        Opinion,
        r"(?P<v>(?:member|supporter|activist|candidate|councillor|volunteer) (?:of|for) (?:the )?{T}|{T} (?:member|activist|supporter|councillor|candidate|voter)|registered (?:democrat|republican))",
        CONF_CUE,
    ),
    // --- Appartenance syndicale ------------------------------------------
    rule_before(
        Syndicat,
        r"(?P<v>(?:deleguee?|representante?|militante?|elue?|responsable|permanente?|correspondante?|mandatee?) syndicale?(?: {T})?)",
        CONF_PHRASE,
        Ctx::Deny(GENERAL_ARTICLE),
    ),
    rule(
        Syndicat,
        r"(?P<v>(?:adherente?|membre|militante?|syndiquee?|deleguee?|representante?|elue?|secretaire(?: generale?)?|tresoriere?|presidente?|permanente?)(?: (?:active?|de longue date))? (?:du |de la |de l'|d'un |d'une |de |d'|a la |au |a l'|chez )(?:(?:syndicat|section syndicale|union syndicale|union locale|union departementale|confederation|federation) )?{T})",
        CONF_CUE,
    ),
    rule_after(
        Syndicat,
        r"(?P<v>(?:adherente?|membre|militante?|deleguee?|representante?|elue?|secretaire(?: generale?)?|tresoriere?|presidente?) (?:du |d'un |d'une |de la |de l')(?:syndicat|section syndicale))",
        CONF_CUE,
        Ctx::Deny(NOT_LABOR_UNION),
    ),
    rule(
        Syndicat,
        r"(?P<v>(?:non )?syndiquee?(?: (?:a la |au |a l'|chez ){T})?)",
        CONF_PHRASE,
    ),
    rule(
        Syndicat,
        r"{POSS} (?P<v>(?:appartenance|adhesion|engagement|mandat|activite|activites|carte) syndicales?|(?:adhesion|appartenance) (?:a la |au |a l')(?:syndicat|{T})|union (?:membership|activities|role))",
        CONF_ANCHORED,
    ),
    rule_before(
        Syndicat,
        r"(?P<v>(?:trade )?union (?:member|membership|representative|rep|steward|official|delegate|activist|organi[sz]er)|shop steward|(?:member|representative|steward|official|delegate) of (?:the |a )?(?:(?:trade )?union|{T})|{T} (?:member|representative|rep|steward|official|delegate|activist))",
        CONF_CUE,
        Ctx::Deny(NOT_TRADE_UNION),
    ),
    rule(Syndicat, r"{COP} (?P<v>unioni[sz]ed)", CONF_ANCHORED),
    // --- Religion ---------------------------------------------------------
    rule(
        Religion,
        r"{COP} (?:un |une |a |an )?(?P<v>(?:(?:devout|practi[sc]ing|fervent) )?{A}(?: (?:pratiquante?|convaincue?|fervente?|non pratiquante?))?)",
        CONF_ANCHORED,
    ),
    rule(
        Religion,
        r"(?P<v>de (?:confession|religion|foi|culte|tradition) {A})",
        CONF_PHRASE,
    ),
    rule(
        Religion,
        r"(?P<v>(?:convertie?|s'est convertie?|se convertit|s'etait convertie?) (?:a l'|au |a la |a )(?:{T}|religion {A})|converted to {T})",
        CONF_CUE,
    ),
    rule(
        Religion,
        r"(?P<v>(?:pratique|pratiquait|observe|observait|fait|faisait|respecte|respectait|suit|suivait) (?:le |la |l')?(?:ramadan|careme|shabbat|sabbat|jeune du ramadan|religion {A}|{T}))",
        CONF_CUE,
    ),
    rule(
        Religion,
        r"(?P<v>(?:frequente|frequentait|se rend a|se rend au|se rendait a|se rendait au|va a|va au|allait a|allait au|prie a|prie au|priait a|priait au) (?:la |l'|le )?(?:mosquee|synagogue|eglise|temple|paroisse|pagode|messe|culte|priere du vendredi)|attends (?:the |a )?(?:mosque|synagogue|church|temple))",
        CONF_CUE,
    ),
    rule(
        Religion,
        r"(?P<v>porte (?:le |la |un |une )?(?:voile(?: islamique)?|foulard islamique|hijab|niqab|burqa|kippa|turban sikh))",
        CONF_CUE,
    ),
    rule(
        Religion,
        r"{POSS} (?P<v>religion|confession|foi (?:{A}|religieuse)|convictions? religieuses?|pratique religieuse|appartenance religieuse|religious beliefs)",
        CONF_ANCHORED,
    ),
    rule(
        Religion,
        r"(?P<v>of the (?:muslim|jewish|catholic|christian|hindu|buddhist|sikh) faith)",
        CONF_PHRASE,
    ),
    // --- Orientation sexuelle ---------------------------------------------
    rule(Orientation, r"{COP} (?P<v>{A})", CONF_ANCHORED),
    rule(
        Orientation,
        r"(?P<v>(?:a fait|fait|faire|a fait recemment) son coming[- ]out(?: (?:homosexuel|gay|lesbien|bisexuel|trans))?|came out as {A})",
        CONF_CUE,
    ),
    rule(Orientation, r"{POSS} (?P<v>{T})", CONF_ANCHORED),
    rule(
        Orientation,
        r"(?P<v>en couple (?:homosexuel|gay|lesbien|de meme sexe))|(?:in a|{POSS}) (?P<w>same-sex (?:relationship|marriage|partner))",
        CONF_PHRASE,
    ),
    // --- Origine raciale ou ethnique --------------------------------------
    rule_before(
        Origine,
        r"(?P<v>d'origines? (?:{A}|{T})(?: et (?:{A}|{T}))?|of (?:{A}|{T}) (?:descent|origin|heritage|ancestry|ethnicity))",
        CONF_PHRASE,
        Ctx::Deny(ORIGIN_OBJECTS),
    ),
    rule_before(
        Origine,
        r"(?P<v>de type (?:{A}|europeen|eurasien))",
        CONF_PHRASE,
        Ctx::Require(PERSON_NOUNS),
    ),
    rule(
        Origine,
        r"(?P<v>(?:appartient|appartenant|appartenait|issue?|originaire|membre) (?:a la |de la |a l'|de l'|d'une |a une )(?:communaute|ethnie|tribu|minorite|population) {A})",
        CONF_CUE,
    ),
    rule(Origine, r"{COP} (?P<v>{A})", CONF_ANCHORED),
    rule(
        Origine,
        r"{POSS} (?P<v>origine ethnique|ethnic origin|ethnic background|ethnicity)",
        CONF_ANCHORED,
    ),
    // --- Données génétiques -----------------------------------------------
    rule(Genetique, r"{POSS} (?P<v>{T})", CONF_ANCHORED),
    rule(
        Genetique,
        r"(?P<v>(?:porteuse?|porteur)(?: saine?)? (?:d'une |de la |du |d'un |de l'|d'|des )(?:mutation|anomalie (?:genetique|chromosomique)|gene|variant|translocation|allele)(?: (?:genetique|chromosomique|pathogene))?(?: (?:du |sur le )?gene)?(?: (?:brca[12]|[a-z]{2,6}-?\d{1,2}))?)",
        CONF_CUE,
    ),
    rule(
        Genetique,
        r"(?P<v>(?:a |avait |ont )?(?:subi|passe|realise|effectue|fait) (?:un |une |des |le |la |l')(?:{T})|(?:took|had|underwent|did) (?:a |an )?{T}|(?:tested|screened) (?:positive )?for (?:the )?(?:brca[12]|[a-z0-9]+ (?:gene|mutation)|{T}))",
        CONF_CUE,
    ),
    rule(
        Genetique,
        r"(?P<v>{T}) (?:de |du |d'|of )?{MONSIEUR}",
        CONF_CUE,
    ),
    // --- Données biométriques ---------------------------------------------
    rule(Biometrie, r"{POSS} (?P<v>{T})", CONF_ANCHORED),
    rule(
        Biometrie,
        r"(?P<v>{T}) (?:de |du |d'|of )?{MONSIEUR}",
        CONF_CUE,
    ),
    // --- Événements personnels --------------------------------------------
    rule(
        Evenement,
        r"{POSS} (?:(?:procedure|dossier|entretien|lettre|courrier|notification|convocation) (?:de |d'|du |a )(?:un |une |l')?)?(?P<v>{T})",
        CONF_ANCHORED,
    ),
    rule(
        Evenement,
        r"(?:a ete|avait ete|vient d'etre|venait d'etre|sera|serait|fut) (?P<v>licenciee?(?: (?:pour (?:faute(?: grave| lourde| simple)?|motif economique|motif personnel|inaptitude(?: physique)?|insuffisance professionnelle|abandon de poste)|economiquement|sans preavis))?)",
        CONF_CUE,
    ),
    rule(
        Evenement,
        r"(?P<v>licenciee? (?:pour (?:faute(?: grave| lourde| simple)?|motif economique|motif personnel|inaptitude(?: physique)?|insuffisance professionnelle|abandon de poste)|economiquement))",
        CONF_CUE,
    ),
    rule(
        Evenement,
        r"(?:a ete|avait ete|vient d'etre|sera|est|fut) (?P<v>mise? a pied(?: (?:conservatoire|disciplinaire))?(?: (?:de|pour) {NUM} jours?)?)",
        CONF_CUE,
    ),
    rule(
        Evenement,
        r"(?P<v>en (?:instance|cours|procedure) de divorce|(?:separee?|divorcee?) (?:de |d')(?:son|sa|leur) {R}|(?:a|ont|avait|avaient) divorce)",
        CONF_PHRASE,
    ),
    rule(
        Evenement,
        r"{COP} (?P<v>(?:recemment |nouvellement )?(?:divorcee?|veuve|veuf)|en deuil)",
        CONF_ANCHORED,
    ),
    rule(
        Evenement,
        r"(?P<v>deces (?:de |d')(?:son|sa|ses|leur|leurs|mon|ma|mes) {R}|(?:a perdu|vient de perdre|venait de perdre|a recemment perdu) (?:son|sa|ses|leur|leurs) {R}|{POSS} {R} (?:est |etait |a ete )?(?:decedee?|morte?)|(?:death|passing|loss) of (?:his|her|their|my) {R}|(?:lost|has lost|recently lost) (?:his|her|their|my) {R}|(?:his|her|their) {R} (?:died|passed away))",
        CONF_CUE,
    ),
    rule(
        Evenement,
        r"(?:fait|faisant|a fait|fera|avait fait) l'objet d'une? (?P<v>procedure disciplinaire|procedure de licenciement|sanction disciplinaire|mise a pied(?: conservatoire)?|avertissement(?: disciplinaire)?|blame|mutation disciplinaire|retrogradation|mesure d'expulsion|procedure d'expulsion|procedure de surendettement|rappel a l'ordre)",
        CONF_CUE,
    ),
    rule(
        Evenement,
        r"(?P<v>convoquee? (?:a|pour) (?:un |l')entretien prealable(?: (?:a|au|en vue d'une?) (?:un |une |son |sa )?(?:licenciement|sanction(?: disciplinaire)?|mise a pied))?)",
        CONF_CUE,
    ),
    rule(
        Evenement,
        r"(?P<v>(?:a|avait|ont) saisi (?:le conseil de |les )?prud'?hommes|en (?:litige|conflit|contentieux) avec (?:son|sa|ses|leur) (?:employeur|ancien employeur|bailleur|proprietaire|ex-epoux|ex-epouse|ex-mari|ex-femme|ex-conjointe?)|(?:a signe|ont signe|a conclu|ont conclu|a demande|a obtenu) une rupture conventionnelle)",
        CONF_CUE,
    ),
    rule_before(
        Evenement,
        r"(?P<v>(?:placee?|mise?) sous (?:tutelle|curatelle(?: renforcee)?|sauvegarde de justice)|inscrite? (?:a|aupres de) (?:pole emploi|france travail))",
        CONF_CUE,
        Ctx::Deny(MODAL),
    ),
    rule(
        Evenement,
        r"{COP} (?P<v>en (?:situation de )?surendettement|au chomage|sans emploi)",
        CONF_ANCHORED,
    ),
    rule(
        Evenement,
        r"(?:en|{POSS}|cet|ce|nouvel|nouveau) (?P<v>arret de travail)",
        CONF_PHRASE,
    ),
    rule(
        Evenement,
        r"(?:was|has been|had been|been|got|were|is being) (?P<v>(?:dismissed|fired|laid off|made redundant|suspended)(?: (?:for|due to) (?:gross misconduct|misconduct|poor performance|redundancy|absence))?)",
        CONF_CUE,
    ),
    rule(
        Evenement,
        r"(?P<v>(?:going through|filed for|in the middle of) (?:a |his |her |their )?divorce|subject to (?:a |an )?disciplinary (?:procedure|hearing|action|investigation))",
        CONF_CUE,
    ),
];

/// Remplace les morceaux et les lexiques d'un motif.
fn expand(rule: &Rule) -> String {
    let mut p = rule.pattern.to_string();
    // `{PEINE}` contient `{NUM}` : deux passes suffisent.
    for _ in 0..2 {
        for (name, piece) in PIECES {
            p = p.replace(name, piece);
        }
    }
    for (name, class) in [
        ("{T}", Class::Term),
        ("{A}", Class::Adjective),
        ("{O}", Class::Organ),
        ("{R}", Class::Relative),
    ] {
        if p.contains(name) {
            let kind = if class == Class::Relative {
                Evenement
            } else {
                rule.kind
            };
            p = p.replace(name, &alternation(kind, class));
        }
    }
    format!(r"(?-u:\b)(?:{p})(?-u:\b)")
}

/// Contexte compilé : `(exigé, motif)`.
type CompiledCtx = Option<(bool, Regex)>;

/// Compile un contexte. Un contexte avant (motif qui finit par « $ ») est
/// précédé d'une limite de mot ; un contexte après commence par « ^ ».
fn compile_ctx(ctx: Ctx, before: bool) -> Result<CompiledCtx, regex::Error> {
    let (require, p) = match ctx {
        Ctx::Any => return Ok(None),
        Ctx::Deny(p) => (false, p),
        Ctx::Require(p) => (true, p),
    };
    let p = if before {
        format!(r"(?-u:\b)(?:{p})")
    } else {
        p.to_string()
    };
    Ok(Some((require, Regex::new(&p)?)))
}

struct Compiled {
    kind: EntityKind,
    re: Regex,
    confidence: f32,
    before: CompiledCtx,
    after: CompiledCtx,
}

struct Engine {
    rules: Vec<Compiled>,
    set: Option<RegexSet>,
}

/// Taille maximale d'un automate compilé (les lexiques font de longues
/// alternances).
const REGEX_SIZE_LIMIT: usize = 64 * 1024 * 1024;

fn compile_rule(r: &Rule, pattern: &str) -> Result<Compiled, regex::Error> {
    Ok(Compiled {
        kind: r.kind,
        re: RegexBuilder::new(pattern)
            .size_limit(REGEX_SIZE_LIMIT)
            .build()?,
        confidence: r.confidence,
        before: compile_ctx(r.before, true)?,
        after: compile_ctx(r.after, false)?,
    })
}

fn engine() -> &'static Engine {
    static CELL: LazyLock<Engine> = LazyLock::new(|| {
        let mut rules = Vec::new();
        let mut patterns = Vec::new();
        for r in RULES {
            let p = expand(r);
            if let Ok(c) = compile_rule(r, &p) {
                rules.push(c);
                patterns.push(p);
            }
        }
        let set = RegexSetBuilder::new(&patterns)
            .size_limit(REGEX_SIZE_LIMIT)
            .build()
            .ok();
        Engine { rules, set }
    });
    &CELL
}

// ---------------------------------------------------------------------------
// Propositions générales
// ---------------------------------------------------------------------------

/// Début de proposition qui annonce une règle générale, pas un fait sur une
/// personne.
static GENERIC_START: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^(?:si|s'il|s'ils|s'elle|s'elles|lorsqu'|lorsque|quand|tout|toute|tous|toutes|chaque|aucun|aucune|nul|nulle|quiconque|if|when|whenever|any|every|each|anyone|everyone|in case)\b",
    )
    .ok()
});

/// Tournure générale dans la même proposition, avant la valeur.
static GENERIC_INSIDE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:en cas d|in case of|in the event of|pour tout|pour toute|pour chaque|de tout|de toute|a tout|a toute|(?:tout|toute|chaque|aucun|aucune) (?:salarie|salariee|agent|agente|employe|employee|personne|patient|patiente|individu|membre|adherent|adherente|candidat|candidate|eleve|etudiant|etudiante|beneficiaire|assure|assuree|usager|usagere)|(?:les|des|ces|aux|certains|certaines|plusieurs|nombreux|nombreuses) (?:personnes|salaries|salariees|patients|patientes|agents|employes|habitants|citoyens|eleves|etudiants|fideles|individus|femmes|hommes|enfants|familles|adherents|militants|candidats|benevoles)|people|employees|patients|workers|students|individuals|anyone|everyone)\b",
    )
    .ok()
});

/// Vrai si la proposition qui précède `start` (texte plié) énonce une règle
/// générale : elle commence par « si », « lorsque », « tout », « chaque »,
/// « if », « when »… ou contient « en cas de », « pour tout », « tout
/// salarié », « les salariés », « employees »… La proposition commence après le dernier signe
/// `. ; : ! ? (` ou saut de ligne.
fn generic_clause(folded: &str, start: usize) -> bool {
    let head = folded.get(..start).unwrap_or("");
    let from = head
        .rfind(['.', ';', ':', '!', '?', '(', '\n', '•'])
        .map_or(0, |i| i + 1);
    let clause = head
        .get(from..)
        .unwrap_or("")
        .trim_start_matches(|c: char| !c.is_alphanumeric());
    let starts = GENERIC_START.as_ref().is_some_and(|re| re.is_match(clause));
    let inside = GENERIC_INSIDE
        .as_ref()
        .is_some_and(|re| re.is_match(clause));
    starts || inside
}

/// Fenêtre de `n` caractères qui finit en `end`.
fn window_before(text: &str, end: usize, n: usize) -> &str {
    let head = text.get(..end).unwrap_or("");
    let from = head
        .char_indices()
        .rev()
        .nth(n.saturating_sub(1))
        .map_or(0, |(i, _)| i);
    head.get(from..).unwrap_or("")
}

/// Fenêtre de `n` caractères qui commence en `start`.
fn window_after(text: &str, start: usize, n: usize) -> &str {
    let tail = text.get(start..).unwrap_or("");
    let to = tail.char_indices().nth(n).map_or(tail.len(), |(i, _)| i);
    tail.get(..to).unwrap_or("")
}

fn ctx_ok(ctx: &CompiledCtx, window: &str) -> bool {
    match ctx {
        None => true,
        Some((require, re)) => re.is_match(window) == *require,
    }
}

// ---------------------------------------------------------------------------
// Détection
// ---------------------------------------------------------------------------

/// Ajoute à `out` les données sensibles et événements de `text` dont le type
/// satisfait `wanted`.
pub(crate) fn detect(text: &str, wanted: impl Fn(EntityKind) -> bool, out: &mut Out) {
    let engine = engine();
    if !engine.rules.iter().any(|r| wanted(r.kind)) {
        return;
    }
    let folded = Folded::new(text);
    let f = folded.text.as_str();
    let candidates: Vec<usize> = match &engine.set {
        Some(set) => set.matches(f).into_iter().collect(),
        None => (0..engine.rules.len()).collect(),
    };
    for i in candidates {
        let Some(rule) = engine.rules.get(i) else {
            continue;
        };
        if !wanted(rule.kind) {
            continue;
        }
        for caps in rule.re.captures_iter(f) {
            let Some(whole) = caps.get(0) else {
                continue;
            };
            let value = caps.name("v").or_else(|| caps.name("w")).unwrap_or(whole);
            if !ctx_ok(&rule.before, window_before(f, whole.start(), BEFORE_WINDOW))
                || !ctx_ok(&rule.after, window_after(f, whole.end(), AFTER_WINDOW))
                || generic_clause(f, whole.start())
            {
                continue;
            }
            if let Some((s, e)) = folded.source(value.start(), value.end()) {
                push(out, text, rule.kind, s, e, rule.confidence);
            }
        }
    }
}

#[cfg(test)]
#[path = "sensitive_tests.rs"]
mod tests;
