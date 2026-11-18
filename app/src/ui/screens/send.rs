//! Étape « Envoyer » (maquette D). Avant l'écriture : la clé de restauration
//! (phrase de passe proposée, trousseau, durée de conservation, dossier des
//! clés distinct de celui des fichiers), ou, sans clé, le verdict et sa
//! confirmation ; le dossier des fichiers ; la fiche d'évaluation. Après
//! l'écriture : les fichiers prêts, le texte et la consigne à copier, la clé
//! créée, le rappel Ctrl+Maj+V, la fiche.

use super::{busy_card, card_title, control_row, copy_button, file_tile, truncated_text};
use crate::ui::app::Effect;
use crate::ui::runner::Shared;
use crate::ui::service::{BatchReport, Outcome, RETENTION_CHOICES, file_name};
use crate::ui::shortcuts;
use crate::ui::state::anonymize::{AnonymizeState, Busy, ProtectMode};
use crate::ui::state::bilan;
use crate::ui::state::files::FileFormat;
use crate::ui::state::toasts::count_label;
use crate::ui::theme::{CONTROL_MD, CONTROL_SM, Weight, font, palette};
use crate::ui::widgets::field::mono_font;
use crate::ui::widgets::nav::segmented;
use crate::ui::widgets::{
    Badge, Banner, Card, Checkbox, Icon, PillButton, StrengthMeter, TextField, body, caption,
    muted, overline,
};
use anon_core::{AssessSettings, Goal, WarningCode};
use egui::{RichText, Ui};

/// Identifiant du premier champ de mot de passe (focus à l'arrivée sur l'étape).
pub const PASSWORD_FIELD: &str = "mdp";
/// Largeur maximale des champs.
const FIELD_MAX_W: f32 = 420.0;
/// Largeur maximale des champs de la fiche.
const FICHE_FIELD_W: f32 = 560.0;
/// Largeur réservée aux boutons d'une ligne « chemin + Changer ».
const CHANGE_BUTTON_W: f32 = 120.0;

/// Ce que l'écran doit savoir en plus de l'état des documents.
#[derive(Debug, Clone, PartialEq)]
pub struct SendContext {
    pub goal: Goal,
    /// Réglages de l'évaluation (verdict de l'objectif « Anonymiser définitivement »).
    pub settings: AssessSettings,
    /// Raison pour laquelle le trousseau n'est pas disponible.
    pub keychain: Option<String>,
    /// La session a déjà servi à un envoi : mêmes balises, mêmes personnes.
    pub reused: bool,
    /// Date du jour, `AAAA-MM-JJ` (fin de conservation proposée).
    pub today: String,
}

pub fn show(
    ui: &mut Ui,
    st: &mut AnonymizeState,
    ctx: &SendContext,
    progress: &Shared,
    fx: &mut Vec<Effect>,
) {
    match st.busy {
        Some(Busy::Anonymizing) => {
            busy_card(
                ui,
                Busy::Anonymizing.label(),
                &st.document_names(),
                progress,
                fx,
            );
            return;
        }
        Some(Busy::OpeningKey) => {
            busy_card(ui, Busy::OpeningKey.label(), &[], progress, fx);
            return;
        }
        _ => {}
    }
    match st.report.clone() {
        Some(report) => written(ui, &report, ctx.goal, fx),
        None => protect(ui, st, ctx, fx),
    }
    fiche_card(ui, st, ctx, fx);
}

/// Avant l'écriture : clé de restauration (ou verdict sans clé) et dossier
/// des fichiers.
fn protect(ui: &mut Ui, st: &mut AnonymizeState, ctx: &SendContext, fx: &mut Vec<Effect>) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(4);
        match ctx.goal {
            Goal::Mask => key_form(ui, st, ctx, fx),
            Goal::Anonymize => without_key(ui, st, ctx),
        }
    });
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        card_title(ui, Icon::FolderOpen, "Dossier des fichiers");
        let dir = st
            .out_dir
            .as_deref()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "Aucun dossier choisi".to_owned());
        if path_row(ui, &dir, "Choisir le dossier où enregistrer les fichiers") {
            fx.push(Effect::PickOutputDir);
        }
        caption(
            ui,
            "Les fichiers masqués y sont enregistrés ; un fichier existant n'est jamais remplacé.",
        );
    });
}

/// Chemin tronqué et bouton « Changer » ; renvoie vrai au clic.
fn path_row(ui: &mut Ui, path: &str, hint: &str) -> bool {
    let p = palette();
    control_row(ui, CONTROL_SM, |ui| {
        ui.spacing_mut().item_spacing.x = p.space(3);
        let w = (ui.available_width() - CHANGE_BUTTON_W).max(120.0);
        truncated_text(
            ui,
            w,
            CONTROL_SM,
            RichText::new(path)
                .font(font(Weight::Regular, p.text_sm))
                .color(p.text),
        );
        ui.add(PillButton::ghost("Changer").small())
            .on_hover_text(hint)
            .clicked()
    })
}

/// Objectif « Anonymiser définitivement » : aucune clé ; le verdict décide du
/// nom des fichiers, et un envoi sans conclusion d'anonymat se confirme.
fn without_key(ui: &mut Ui, st: &mut AnonymizeState, ctx: &SendContext) {
    card_title(ui, Icon::Lock, "Aucune clé de restauration");
    body(
        ui,
        "Objectif « Anonymiser définitivement » : aucune clé n'est créée. Les balises ne \
pourront jamais être remplacées par les vraies valeurs.",
    );
    let verdict = st.assessment(&ctx.settings).map(|a| {
        (
            a.assessment.is_anonymous(),
            bilan::verdict_line(&a.assessment),
        )
    });
    match verdict {
        None => muted(ui, "Évaluation du dossier en cours…"),
        Some((true, line)) => Banner::success(
            "Les fichiers seront nommés « _anonyme ». Enregistrez la fiche d'évaluation avec le \
dossier.",
        )
        .title(&line)
        .show(ui),
        Some((false, line)) => {
            Banner::warning(
                "Corrigez les points restants à l'étape Bilan, ou confirmez l'envoi : les \
fichiers seront alors nommés « _masque_sans_cle ».",
            )
            .title(&line)
            .show(ui);
            let mut confirmed = st.protect.confirmed;
            let r = crate::ui::widgets::checkbox::toggle(
                ui,
                &mut confirmed,
                "Confirmer l'envoi sans correction : fichiers masqués, sans clé",
            );
            if r.changed() {
                st.protect.confirmed = confirmed;
            }
            r
        }
    };
}

fn key_form(ui: &mut Ui, st: &mut AnonymizeState, ctx: &SendContext, fx: &mut Vec<Effect>) {
    card_title(ui, Icon::KeyRound, "Clé de restauration");
    muted(
        ui,
        "La clé relie chaque balise à sa vraie valeur. Elle est chiffrée : sans son mot de \
passe, la restauration est impossible.",
    );
    let mut mode = st.protect.mode;
    segmented(
        ui,
        "Clé de restauration",
        &mut mode,
        &[
            (ProtectMode::NewPassword, "Nouvelle clé"),
            (ProtectMode::ExistingTable, "Reprendre une clé"),
        ],
    );
    if mode != st.protect.mode {
        st.protect.mode = mode;
        st.protect.password.clear();
        st.protect.confirm.clear();
        if mode == ProtectMode::NewPassword {
            st.protect.propose_passphrase();
        } else {
            st.protect.generated = None;
        }
    }
    let field_w = ui.available_width().min(FIELD_MAX_W);
    match st.protect.mode {
        ProtectMode::NewPassword | ProtectMode::Irreversible => {
            if st.protect.uses_generated() {
                passphrase(ui, st, fx);
            } else {
                own_password(ui, st, field_w);
            }
        }
        ProtectMode::ExistingTable => existing_key(ui, st, field_w, fx),
    }
    if ctx.reused || st.protect.mode == ProtectMode::ExistingTable {
        Banner::warning(
            "Les mêmes balises désignent les mêmes personnes : l'IA peut relier ces \
conversations. Pour un envoi sans lien avec le précédent, commencez une nouvelle session.",
        )
        .title("Clé déjà utilisée pour un envoi")
        .show(ui);
    }
    // Trousseau du système.
    let mut remember = st.protect.remember && ctx.keychain.is_none();
    let r = ui.add(
        Checkbox::new(remember, "Retenir le mot de passe sur cet ordinateur")
            .enabled(ctx.keychain.is_none()),
    );
    if r.clicked() && ctx.keychain.is_none() {
        remember = !remember;
        st.protect.remember = remember;
    }
    match &ctx.keychain {
        Some(why) => caption(ui, &format!("Option indisponible : {why}")),
        None => caption(
            ui,
            "La restauration ne demandera plus le mot de passe sur cet ordinateur.",
        ),
    };
    // Durée de conservation.
    overline(ui, "Conservation de la clé");
    let options: Vec<(u32, String)> = RETENTION_CHOICES
        .iter()
        .map(|m| (*m, format!("{m} mois")))
        .collect();
    let labels: Vec<(u32, &str)> = options.iter().map(|(m, l)| (*m, l.as_str())).collect();
    segmented(
        ui,
        "Durée de conservation de la clé",
        &mut st.protect.keep_months,
        &labels,
    );
    if let Some(until) = anon_vault::retention::add_months(&ctx.today, st.protect.keep_months) {
        caption(
            ui,
            &format!(
                "Jusqu'au {}. Passé cette date, l'étape Réponse vous proposera de la détruire.",
                french_date(&until)
            ),
        );
    }
    // Dossier des clés.
    overline(ui, "Dossier de la clé");
    let dir = st
        .key_dir
        .as_deref()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "Aucun dossier choisi".to_owned());
    if path_row(ui, &dir, "Choisir le dossier de la clé de restauration") {
        fx.push(Effect::PickKeyDir);
    }
    if let Some(name) = &st.key_name {
        caption(ui, &format!("Nom du fichier : {name}"));
    }
    if st.key_beside_outputs() {
        Banner::warning(
            "La clé est dans le dossier des fichiers à envoyer : elle risque de partir avec eux. \
Choisissez un autre dossier.",
        )
        .title("Clé à côté des fichiers")
        .show(ui);
    }
}

/// Phrase de passe proposée : en clair, à copier ou à remplacer.
fn passphrase(ui: &mut Ui, st: &mut AnonymizeState, fx: &mut Vec<Effect>) {
    let p = palette();
    overline(ui, "Phrase de passe");
    let phrase = st.protect.password.clone();
    control_row(ui, CONTROL_SM, |ui| {
        ui.spacing_mut().item_spacing.x = p.space(3);
        ui.label(RichText::new(&phrase).font(mono_font()).color(p.heading))
            .on_hover_text("Phrase de passe de la clé");
        if copy_button(
            ui,
            "copier-phrase",
            PillButton::secondary("Copier").icon(Icon::Copy).small(),
            true,
        )
        .clicked()
        {
            fx.push(Effect::Copy {
                text: phrase.clone(),
                toast: "Phrase de passe copiée",
                sensitive: true,
            });
        }
        if ui
            .add(
                PillButton::ghost("Générer une autre")
                    .icon(Icon::RefreshCw)
                    .small(),
            )
            .clicked()
        {
            st.protect.propose_passphrase();
        }
    });
    caption(
        ui,
        "Notez-la en lieu sûr, ou retenez-la sur cet ordinateur : elle ouvre la clé.",
    );
    if ui
        .add(PillButton::ghost("Saisir votre mot de passe").small())
        .clicked()
    {
        st.protect.choose_own();
    }
}

/// Mot de passe choisi par l'utilisateur, avec sa force et sa confirmation.
fn own_password(ui: &mut Ui, st: &mut AnonymizeState, field_w: f32) {
    let issue = st.protect.password_issue().map(|i| i.message());
    TextField::password("Mot de passe", &mut st.protect.password)
        .id_salt(PASSWORD_FIELD)
        .required(true)
        .error(issue)
        .width(field_w)
        .show(ui);
    ui.scope(|ui| {
        ui.set_width(field_w);
        ui.add(StrengthMeter::of(&st.protect.password));
    });
    let confirm_issue = st.protect.confirm_issue().map(|i| i.message());
    TextField::password("Confirmation du mot de passe", &mut st.protect.confirm)
        .id_salt("mdp-confirmation")
        .required(true)
        .error(confirm_issue)
        .width(field_w)
        .show(ui);
    if ui
        .add(PillButton::ghost("Utiliser une phrase de passe proposée").small())
        .clicked()
    {
        st.protect.propose_passphrase();
    }
}

/// Clé reprise : mêmes balises, même mot de passe.
fn existing_key(ui: &mut Ui, st: &mut AnonymizeState, field_w: f32, fx: &mut Vec<Effect>) {
    let p = palette();
    muted(
        ui,
        "Les valeurs déjà présentes dans cette clé gardent leurs balises. Une nouvelle clé, \
complétée, est enregistrée avec le même mot de passe.",
    );
    match &st.protect.existing {
        Some(t) => {
            ui.horizontal(|ui| {
                ui.add(Badge::brand("Ouverte"));
                body(
                    ui,
                    &format!(
                        "{} : {}",
                        t.name,
                        count_label(t.entries, "valeur", "valeurs")
                    ),
                );
            });
        }
        None => {
            let name = st
                .protect
                .existing_path
                .as_deref()
                .map(file_name)
                .unwrap_or_else(|| "Aucune clé choisie".to_owned());
            control_row(ui, CONTROL_SM, |ui| {
                ui.spacing_mut().item_spacing.x = p.space(3);
                if ui
                    .add(
                        PillButton::secondary("Choisir la clé")
                            .icon(Icon::FolderOpen)
                            .small(),
                    )
                    .clicked()
                {
                    fx.push(Effect::PickExistingMap);
                }
                body(ui, &name);
            });
            let error = st.protect.existing_error.as_deref();
            TextField::password("Mot de passe de la clé", &mut st.protect.password)
                .id_salt("mdp-reprise")
                .error(error)
                .width(field_w)
                .show(ui);
            let ready = st.protect.existing_path.is_some() && !st.protect.password.is_empty();
            if ui
                .add(
                    PillButton::primary("Ouvrir la clé")
                        .icon(Icon::LockOpen)
                        .enabled(ready)
                        .small(),
                )
                .clicked()
            {
                fx.push(Effect::OpenExistingMap);
            }
        }
    }
}

/// `AAAA-MM-JJ` → `JJ/MM/AAAA`.
pub fn french_date(date: &str) -> String {
    match date.split('-').collect::<Vec<_>>().as_slice() {
        [y, m, d] => format!("{d}/{m}/{y}"),
        _ => date.to_owned(),
    }
}

/// Fiche d'évaluation : champs facultatifs, enregistrement (Markdown ou PDF).
fn fiche_card(ui: &mut Ui, st: &mut AnonymizeState, ctx: &SendContext, fx: &mut Vec<Effect>) {
    let p = palette();
    let ready = st.assessment(&ctx.settings).is_some();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(3);
        card_title(ui, Icon::FileText, "Fiche d'évaluation");
        muted(
            ui,
            "Elle consigne l'évaluation : balises, catégories et nombres, jamais une valeur. \
Conservez-la avec le dossier. Les champs sont facultatifs.",
        );
        let w = ui.available_width().min(FICHE_FIELD_W);
        let f = &mut st.bilan.fiche;
        TextField::new("Base légale", &mut f.legal_basis)
            .id_salt("fiche-base-legale")
            .placeholder("Exemple : intérêt légitime, mission d'intérêt public")
            .width(w)
            .show(ui);
        TextField::new("Finalité", &mut f.purpose)
            .id_salt("fiche-finalite")
            .width(w)
            .show(ui);
        TextField::new("Risques acceptés", &mut f.accepted_risks)
            .id_salt("fiche-risques")
            .width(w)
            .show(ui);
        TextField::new(
            "Justification des risques acceptés",
            &mut f.risks_justification,
        )
        .id_salt("fiche-justification")
        .width(w)
        .show(ui);
        crate::ui::widgets::checkbox::toggle(
            ui,
            &mut f.with_file_names,
            "Inclure le nom des fichiers",
        );
        control_row(ui, CONTROL_SM, |ui| {
            ui.spacing_mut().item_spacing.x = p.space(3);
            let go = super::guarded_button(
                ui,
                PillButton::secondary("Enregistrer la fiche")
                    .icon(Icon::Download)
                    .small(),
                (!ready).then_some("Patientez : l'évaluation du dossier est en cours."),
            );
            if go.clicked() {
                fx.push(Effect::SaveFiche);
            }
            caption(
                ui,
                "Markdown ou PDF, selon le format choisi ; proposée à côté des fichiers produits.",
            );
        });
    });
}

/// Après l'écriture : fichiers prêts, deux façons d'envoyer, clé créée.
fn written(ui: &mut Ui, report: &BatchReport, goal: Goal, fx: &mut Vec<Effect>) {
    let p = palette();
    let delivered = report.delivered();
    let total = report.files.len();
    if let Some(fatal) = &report.fatal {
        Banner::error(fatal).title("Aucun fichier écrit").show(ui);
    } else {
        let mut text = if delivered == total {
            let files = if total == 1 {
                "le fichier".to_owned()
            } else {
                format!("les {total} fichiers")
            };
            format!("Vérification finale réussie : aucune valeur masquée ne subsiste dans {files}.")
        } else {
            format!(
                "{} sur {total} : les autres n'ont pas passé la vérification finale ou n'ont pas été traités.",
                count_label(delivered, "fichier prêt", "fichiers prêts")
            )
        };
        if report.cancelled {
            text.push_str(" Traitement annulé : les fichiers restants n'ont pas été traités.");
        }
        let banner = if delivered == total {
            Banner::success(&text)
        } else {
            Banner::warning(&text)
        };
        banner.title("Prêt à envoyer").show(ui);
    }
    if delivered > 0 {
        ui.columns(2, |cols| {
            if let [a, b] = cols {
                way(
                    a,
                    Icon::FolderOpen,
                    "Envoyer les fichiers",
                    "Les fichiers masqués sont enregistrés dans un dossier. Joignez-les à votre \
conversation avec l'IA.",
                    |ui| {
                        if ui
                            .add(PillButton::primary("Ouvrir le dossier des fichiers").small())
                            .clicked()
                        {
                            fx.push(Effect::OpenFolder(report.out_dir.clone()));
                        }
                    },
                );
                way(
                    b,
                    Icon::Clipboard,
                    "Envoyer le texte",
                    "Le texte masqué est copié avec la consigne, prêt à coller dans la conversation.",
                    |ui| {
                        if copy_button(
                            ui,
                            "copier-texte-consigne",
                            PillButton::secondary("Copier le texte et la consigne").small(),
                            true,
                        )
                        .clicked()
                        {
                            fx.push(Effect::CopyTextForAi);
                        }
                    },
                );
            }
        });
        if report.map_path.is_some() {
            muted(
                ui,
                &format!(
                    "Quand l'IA a répondu : copiez sa réponse, puis revenez ici ({}).",
                    shortcuts::PASTE_RESPONSE.label()
                ),
            );
        }
    }
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for f in &report.files {
            file_line(ui, &f.name, &f.outcome, fx);
        }
    });
    ui.columns(2, |cols| {
        if let [a, b] = cols {
            Card::new().show(a, |ui| {
                ui.spacing_mut().item_spacing.y = p.space(2);
                card_title(ui, Icon::Sparkles, "Consigne pour l'IA");
                muted(
                    ui,
                    "Les mots entre crochets, comme [PERSONNE_1], remplacent des données \
personnelles. L'IA doit les garder tels quels dans sa réponse.",
                );
                if copy_button(
                    ui,
                    "copier-consigne",
                    PillButton::ghost("Copier la consigne pour l'IA")
                        .icon(Icon::Copy)
                        .small(),
                    true,
                )
                .clicked()
                {
                    fx.push(Effect::Copy {
                        text: anon_engine::ai_instruction().to_owned(),
                        toast: "Consigne copiée",
                        sensitive: false,
                    });
                }
            });
            Card::new().show(b, |ui| {
                ui.spacing_mut().item_spacing.y = p.space(2);
                card_title(ui, Icon::KeyRound, "Clé de restauration");
                match (&report.map_path, goal) {
                    (Some(map), _) => {
                        body(ui, &file_name(map));
                        if let Some(dir) = map.parent() {
                            caption(ui, &format!("Dans {}", dir.display()));
                        }
                        if let Some(until) = &report.keep_until {
                            muted(ui, &format!("À conserver jusqu'au {}.", french_date(until)));
                        }
                        match &report.remembered {
                            Some(Ok(())) => muted(ui, "Mot de passe retenu sur cet ordinateur."),
                            Some(Err(_)) => {
                                muted(ui, "Mot de passe non retenu sur cet ordinateur : notez-le.")
                            }
                            None => muted(
                                ui,
                                "Ouverte en mémoire jusqu'à la fin de la session : la réponse \
se restaure sans redemander le mot de passe.",
                            ),
                        };
                    }
                    (None, Goal::Anonymize) => {
                        muted(ui, "Aucune clé créée : aucun retour possible.");
                    }
                    (None, Goal::Mask) => {
                        muted(ui, "Aucune clé écrite : aucun fichier n'a été livré.");
                    }
                }
            });
        }
    });
}

/// Une façon d'envoyer : titre, explication, bouton.
fn way(ui: &mut Ui, icon: Icon, title: &str, text: &str, action: impl FnOnce(&mut Ui)) {
    let p = palette();
    Card::new().show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = p.space(2);
        card_title(ui, icon, title);
        muted(ui, text);
        action(ui);
    });
}

/// Ligne d'un fichier du lot : nom produit, état, avertissements.
fn file_line(ui: &mut Ui, name: &str, outcome: &Outcome, fx: &mut Vec<Effect>) {
    let p = palette();
    let format = FileFormat::from_path(std::path::Path::new(name));
    control_row(ui, CONTROL_MD + 8.0, |ui| {
        ui.spacing_mut().item_spacing.x = p.space(3);
        file_tile(ui, format);
        let shown = match outcome {
            Outcome::Written { path, .. } => file_name(path),
            _ => name.to_owned(),
        };
        let w = (ui.available_width() - 240.0).max(120.0);
        truncated_text(
            ui,
            w,
            CONTROL_MD,
            RichText::new(shown)
                .font(font(Weight::Semibold, p.text_sm))
                .color(p.heading),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon, ink, text) = match outcome {
                Outcome::Written { replacements, .. } => (
                    Icon::CircleCheck,
                    p.accent,
                    count_label(*replacements, "valeur masquée", "valeurs masquées"),
                ),
                Outcome::Leak { .. } => (
                    Icon::CircleX,
                    p.error,
                    "Non livré : vérification finale".to_owned(),
                ),
                Outcome::Failed(_) => (Icon::CircleX, p.error, "Non traité".to_owned()),
                Outcome::Cancelled => (Icon::Clock, p.text_secondary, "Annulé".to_owned()),
            };
            ui.label(
                RichText::new(text)
                    .font(font(Weight::Semibold, p.text_xs))
                    .color(ink),
            );
            icon.show(ui, ink, 16.0);
        });
    });
    match outcome {
        Outcome::Written { warnings, .. } => {
            for w in warnings
                .iter()
                .filter(|w| w.code == WarningCode::ImageNotRestorable)
            {
                let text = format!(
                    "{} Le fichier masqué est bien protégé ; seule la restauration des images est impossible.",
                    w.detail
                );
                Banner::warning(&text)
                    .title("Images masquées non restaurables")
                    .show(ui);
            }
            for w in warnings
                .iter()
                .filter(|w| w.code != WarningCode::ImageNotRestorable)
            {
                caption(ui, &format!("{} : {}", w.part, w.detail));
            }
        }
        Outcome::Leak { tags, parts } => {
            let text = format!(
                "Après réécriture, {} restent lisibles ({}). Le fichier n'a pas été enregistré. \
Vous pouvez enregistrer son texte masqué, sans mise en forme.",
                tags.join(", "),
                parts.join(", ")
            );
            Banner::error(&text)
                .title("Vérification finale : fichier bloqué")
                .show(ui);
            if ui
                .add(
                    PillButton::secondary("Enregistrer le texte masqué (.md)")
                        .icon(Icon::Download)
                        .small(),
                )
                .clicked()
            {
                fx.push(Effect::ExportText(name.to_owned()));
            }
        }
        Outcome::Failed(msg) => {
            caption(ui, msg);
        }
        Outcome::Cancelled => {}
    }
}
