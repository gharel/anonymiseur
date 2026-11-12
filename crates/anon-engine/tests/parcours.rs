//! Parcours complet sur `exemples/`, cohérence multi-fichiers, restauration
//! d'une réponse d'IA, options de l'utilisateur.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anon_core::text::apply_replacements;
use anon_core::{
    BlockRole, EntityKind, Handler, Profile, Session, Source, Tag, TextTransform, Treatment,
};
use anon_engine::{AnonymizeOptions, Engine, InputFile, NoProgress, ai_instruction};
use anon_office::handler_for;
use anon_replace::{Replacer, Restorer, SessionExt};
use common::*;

fn handler(name: &str) -> &'static dyn Handler {
    handler_for(name.rsplit_once('.').unwrap().1).unwrap()
}

/// Texte des blocs (hors métadonnées et cibles de liens), dans l'ordre.
fn texts(name: &str, bytes: &[u8]) -> Vec<String> {
    body_blocks(handler(name), bytes)
        .into_iter()
        .filter(|b| b.ctx.role != BlockRole::Hyperlink)
        .map(|b| b.text)
        .collect()
}

/// Texte attendu après anonymisation puis restauration : les variantes
/// reviennent sous la forme canonique de l'entité (comportement du `Restorer`).
fn canonical(session: &Session, text: &str) -> String {
    let ctx = anon_core::BlockContext::new("x", BlockRole::Body);
    let r = Replacer::new(session);
    let anonymized = apply_replacements(text, &r.replacements(text, &ctx)).into_owned();
    Restorer::new(session).apply(&anonymized, &ctx).into_owned()
}

/// Exigence 1 : analyser → valider → anonymiser → aucune valeur détectée ne subsiste →
/// restaurer → texte des blocs identique à l'original.
#[test]
fn parcours_complet_sur_chaque_exemple() {
    let engine = Engine::standard();
    let files = handled(&engine);
    for ext in ["docx", "xlsx", "pptx", "txt", "md", "csv"] {
        assert!(
            files.iter().any(|f| f.name.ends_with(ext)),
            "aucun exemple .{ext} géré"
        );
    }
    for f in &files {
        for profile in [Profile::Standard, Profile::Renforce] {
            let file = input(&f.name);
            let analysis = analyze(&engine, std::slice::from_ref(&file), profile);
            assert!(analysis.files[0].error.is_none(), "{}", f.name);
            let mut session = Session::new();
            engine.apply_selection(&analysis, &mut session);
            let out = engine
                .anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default())
                .unwrap_or_else(|e| panic!("{} ({profile:?}) : {e}", f.name));
            assert_eq!(
                out.name,
                f.name.replacen('.', "_anonymise.", 1),
                "nom du fichier produit"
            );
            assert!(out.report.replacements > 0, "{}", f.name);
            assert!(!out.report.risk.advice.is_empty());

            // Aucune valeur attendue détectée (présente dans la session) ne subsiste.
            let deep = handler(&f.name).deep_scan(&out.bytes).unwrap();
            for e in f.expected(profile) {
                let Some(tag) = session.tag_for(e.kind, &e.value) else {
                    continue;
                };
                let mut probe = Session::new();
                probe.intern_with_case(e.kind, &e.value, Source::Manual, true);
                let r = Replacer::new(&probe);
                for b in &deep {
                    assert!(
                        r.matches(&b.text).is_empty(),
                        "{} : {tag} subsiste dans {}",
                        f.name,
                        b.ctx.part
                    );
                }
            }

            // Restauration.
            let anonymized = InputFile::new(out.name.clone(), out.bytes.clone());
            let restored = engine.restore(&anonymized, &session).unwrap();
            assert_eq!(restored.name, f.name.replacen('.', "_restaure.", 1));
            assert!(restored.report.unknown.is_empty(), "{}", f.name);
            let before = texts(&f.name, &file.bytes);
            let after = texts(&f.name, &restored.bytes);
            assert_eq!(before.len(), after.len(), "{} : nombre de blocs", f.name);
            let mut identical = 0;
            for (b, a) in before.iter().zip(&after) {
                assert_eq!(*a, canonical(&session, b), "{} ({profile:?})", f.name);
                if a == b {
                    identical += 1;
                }
            }
            // Seules les variantes (casse, `DUPONT Jean`, `J. Dupont`) diffèrent.
            let has_variants = session.entities.iter().any(|e| !e.variants.is_empty());
            if !has_variants {
                assert_eq!(identical, before.len(), "{}", f.name);
            }
        }
    }
}

/// Exigence 3 : la même personne reçoit la même balise dans le docx, le xlsx et le pptx ;
/// une session rechargée (clone) redonne les mêmes balises.
#[test]
fn coherence_multi_fichiers() {
    let engine = Engine::standard();
    let names = [
        "contrat-prestation.docx",
        "fichier-clients.xlsx",
        "presentation-projet.pptx",
    ];
    let files: Vec<InputFile> = names.iter().map(|n| input(n)).collect();
    let analysis = analyze(&engine, &files, Profile::Standard);
    assert!(analysis.files.iter().all(|f| f.error.is_none()));
    // Une seule ligne pour Jean Dupont, présente dans les trois fichiers.
    let jean: Vec<_> = analysis
        .candidates
        .iter()
        .filter(|c| c.kind == EntityKind::Personne && squash(&c.value) == "jeandupont")
        .collect();
    assert_eq!(jean.len(), 1);
    for n in names {
        assert!(jean[0].files.iter().any(|f| f == n), "{n}");
    }
    // Les formes « DUPONT Jean » et « J. Dupont » sont des variantes.
    assert!(jean[0].variants.iter().any(|v| v == "DUPONT Jean"));
    assert!(jean[0].variants.iter().any(|v| v == "J. Dupont"));

    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    let tag = session
        .tag_for(EntityKind::Personne, "Jean Dupont")
        .unwrap();
    assert_eq!(
        session.tag_for(EntityKind::Personne, "J. Dupont"),
        Some(tag)
    );
    // Nom et prénom isolés du tableur liés à la même personne.
    assert_eq!(
        session.tag_for(EntityKind::Prenom, "Jean"),
        Some(Tag::new(EntityKind::Prenom, tag.index))
    );
    assert_eq!(
        session.tag_for(EntityKind::Nom, "Dupont"),
        Some(Tag::new(EntityKind::Nom, tag.index))
    );

    let reloaded = session.clone();
    let mut outputs = Vec::new();
    for f in &files {
        let out = engine
            .anonymize(f, &analysis, &mut session, &AnonymizeOptions::default())
            .unwrap();
        let text: String = handler(&f.name)
            .deep_scan(&out.bytes)
            .unwrap()
            .into_iter()
            .map(|b| b.text + "\n")
            .collect();
        let person = tag.to_string();
        let parts = [
            person.clone(),
            Tag::new(EntityKind::Prenom, tag.index).to_string(),
        ];
        assert!(
            parts.iter().any(|p| text.contains(p.as_str())),
            "{} : balise de Jean Dupont absente",
            f.name
        );
        outputs.push(text);
    }
    // Session rechargée : nouvelle analyse, mêmes balises, même résultat.
    let again = engine.analyze(&files, &opts(Profile::Standard), &reloaded, &NoProgress);
    let jean2 = again
        .candidates
        .iter()
        .find(|c| c.kind == EntityKind::Personne && squash(&c.value) == "jeandupont")
        .unwrap();
    assert_eq!(jean2.existing_tag, Some(tag));
    assert!(jean2.selected);
    let mut s2 = reloaded.clone();
    engine.apply_selection(&again, &mut s2);
    assert_eq!(s2.entities.len(), reloaded.entities.len());
    for (f, previous) in files.iter().zip(&outputs) {
        let out = engine
            .anonymize(f, &again, &mut s2, &AnonymizeOptions::default())
            .unwrap();
        let text: String = handler(&f.name)
            .deep_scan(&out.bytes)
            .unwrap()
            .into_iter()
            .map(|b| b.text + "\n")
            .collect();
        assert_eq!(&text, previous, "{}", f.name);
    }
}

/// Exigence 4 : restauration de la réponse d'IA simulée avec la session du contrat.
#[test]
fn restauration_de_la_reponse_ia() {
    let engine = Engine::standard();
    let contrat = input("contrat-prestation.docx");
    let analysis = analyze(&engine, std::slice::from_ref(&contrat), Profile::Renforce);
    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    let value = |kind, i| session.entity(Tag::new(kind, i)).unwrap().value.clone();
    let p1 = value(EntityKind::Personne, 1);
    let p2 = value(EntityKind::Personne, 2);
    let s1 = value(EntityKind::Societe, 1);

    let reponse = input("reponse-ia-exemple.md");
    let restored = engine.restore(&reponse, &session).unwrap();
    assert_eq!(restored.name, "reponse-ia-exemple_restaure.md");
    let text = String::from_utf8(restored.bytes).unwrap();
    for altered in [
        "[Personne 1]",
        "[PERSONNE\\_1]",
        "[personne_2]",
        "[SOCIETE_1]",
    ] {
        assert!(!text.contains(altered), "{altered} non restaurée");
    }
    assert!(text.contains(&format!("**{s1}**")));
    assert!(text.contains(&format!("Casse et espace modifiées : {p1} doit")));
    assert!(text.contains(&format!("Soulignement échappé (Markdown) : {p1} reste")));
    assert!(text.contains(&format!("Minuscules : {p2} correspond")));
    assert!(text.contains("[PERSONNE_99]"));
    assert_eq!(restored.report.unknown, vec!["[PERSONNE_99]".to_string()]);
    assert!(restored.report.restored >= 10);

    // Même résultat en texte collé.
    let raw = String::from_utf8(reponse.bytes.clone()).unwrap();
    let (t2, rep2) = engine.restore_text(&raw, &session);
    assert_eq!(t2, text);
    assert_eq!(rep2.unknown, vec!["[PERSONNE_99]".to_string()]);
}

const LETTRE: &str = "Madame Hélène Kaloï, 44 ans, a rencontré Pierre Garnier.\n\
    Votre projet est la pierre angulaire de notre action.\n\
    Contact : helene.kaloi@example.com, code interne ZORGLUB-7.\n\
    Nautilus Pacifique SARL";

/// Exigence 5 : mode irréversible, listes « jamais » et « toujours », ajout manuel,
/// candidat décoché, valeur ambiguë, généralisation.
#[test]
fn options_utilisateur() {
    let engine = Engine::standard();

    // Liste « toujours » et liste « jamais ».
    let mut o = opts(Profile::Renforce);
    o.always.push(("ZORGLUB-7".into(), EntityKind::Identifiant));
    o.never.push("Nautilus Pacifique SARL".into());
    let analysis = engine.analyze_text(LETTRE, &o, &Session::new());
    let zorglub = analysis
        .candidates
        .iter()
        .find(|c| c.value == "ZORGLUB-7")
        .unwrap();
    assert_eq!(zorglub.kind, EntityKind::Identifiant);
    assert_eq!(zorglub.source, Source::List);
    assert!(zorglub.selected);
    assert!(
        !analysis
            .candidates
            .iter()
            .any(|c| c.value.contains("Nautilus"))
    );
    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    assert!(session.is_never("nautilus pacifique sarl"));
    let out = engine.anonymize_text(LETTRE, &session);
    assert!(out.contains("Nautilus Pacifique SARL"));
    assert!(!out.contains("ZORGLUB-7"));

    // Valeur ambiguë : « Pierre » isolé ne touche pas « pierre angulaire ».
    let mut analysis = engine.analyze_text(LETTRE, &opts(Profile::Standard), &Session::new());
    let id = analysis.add_manual(EntityKind::Prenom, "Pierre").unwrap();
    let pierre = analysis.candidate(id).unwrap();
    assert!(pierre.ambiguous && pierre.case_sensitive && pierre.selected);
    assert_eq!(pierre.source, Source::Manual);
    // « Pierre » n'est remplacé seul nulle part : il fait partie de « Pierre Garnier ».
    assert_eq!(pierre.occurrences, 0);
    assert!(analysis.add_manual(EntityKind::Prenom, "  ").is_none());

    // Ajout manuel : occurrences recomptées.
    let code = analysis
        .add_manual(EntityKind::Identifiant, "ZORGLUB-7")
        .unwrap();
    assert_eq!(analysis.candidate(code).unwrap().occurrences, 1);
    assert!(
        analysis
            .candidate(code)
            .unwrap()
            .context
            .contains("code interne")
    );
    // Même valeur ajoutée deux fois : même ligne.
    assert_eq!(
        analysis.add_manual(EntityKind::Identifiant, "zorglub-7"),
        Some(code)
    );

    // Candidat décoché : non remplacé.
    let email = analysis
        .candidates
        .iter()
        .find(|c| c.kind == EntityKind::Email)
        .unwrap()
        .id;
    assert!(analysis.set_selected(email, false));
    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    let out = engine.anonymize_text(LETTRE, &session);
    assert!(out.contains("helene.kaloi@example.com"));
    assert!(out.contains("pierre angulaire"));
    assert!(!out.contains("ZORGLUB-7"));
    assert!(!out.contains("Garnier"));

    // Décoché alors que déjà dans la session : désactivé.
    let again = engine.analyze_text(LETTRE, &opts(Profile::Standard), &{
        let mut s = session.clone();
        s.intern(EntityKind::Email, "helene.kaloi@example.com", Source::Rule);
        s
    });
    let mut s3 = session.clone();
    s3.intern(EntityKind::Email, "helene.kaloi@example.com", Source::Rule);
    let mut again = again;
    let e = again
        .candidates
        .iter()
        .find(|c| c.kind == EntityKind::Email)
        .unwrap();
    assert!(e.existing_tag.is_some() && e.selected);
    let eid = e.id;
    again.set_selected(eid, false);
    engine.apply_selection(&again, &mut s3);
    assert!(
        engine
            .anonymize_text(LETTRE, &s3)
            .contains("helene.kaloi@example.com")
    );

    // Changement de type et suppression.
    assert!(analysis.set_kind(code, EntityKind::Matricule));
    assert_eq!(
        analysis.candidate(code).unwrap().kind,
        EntityKind::Matricule
    );
    assert!(analysis.remove(code));
    assert!(analysis.candidate(code).is_none());
    assert!(!analysis.remove(code));

    // Généralisation : valeur lisible, non restaurée.
    let mut a = engine.analyze_text(LETTRE, &opts(Profile::Renforce), &Session::new());
    let age = a
        .candidates
        .iter()
        .find(|c| c.kind == EntityKind::Age)
        .unwrap()
        .id;
    assert!(a.set_treatment(age, Treatment::Generalize("40-49 ans".into())));
    let mut s = Session::new();
    engine.apply_selection(&a, &mut s);
    let out = engine.anonymize_text(LETTRE, &s);
    assert!(out.contains("40-49 ans") && !out.contains("44 ans"));
    let (back, _) = engine.restore_text(&out, &s);
    assert!(back.contains("40-49 ans"));
    assert!(back.contains("Hélène Kaloï"));
}

/// Exigence 5 (suite) : mode irréversible : rien n'est gardé dans `session.media`
/// (vérifié aussi avec images dans `images.rs`) et la consigne IA est fournie.
#[test]
fn irreversible_et_consigne() {
    let engine = Engine::standard();
    let file = input("presentation-projet.pptx");
    let analysis = analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    let mut session = Session {
        irreversible: true,
        ..Session::new()
    };
    engine.apply_selection(&analysis, &mut session);
    engine
        .anonymize(&file, &analysis, &mut session, &AnonymizeOptions::default())
        .unwrap();
    assert!(session.media.is_empty());
    // Sans traitement d'images, l'image de la diapositive est signalée.
    assert!(
        analysis.files[0]
            .warnings
            .iter()
            .any(|w| w.code == anon_core::WarningCode::ImageNotProcessed)
    );
    assert!(ai_instruction().contains("crochets"));
}

/// Texte exporté : lisible, anonymisé, sans métadonnées.
#[test]
fn export_texte() {
    let engine = Engine::standard();
    let file = input("contrat-prestation.docx");
    let analysis = analyze(&engine, std::slice::from_ref(&file), Profile::Standard);
    let mut session = Session::new();
    engine.apply_selection(&analysis, &mut session);
    let text = engine.export_text(&file, &session).unwrap();
    assert!(text.contains("[PERSONNE_"));
    assert!(!text.contains("Jean Dupont"));
    assert!(!text.contains("jean.dupont@example.com"));
    assert!(text.lines().count() > 5);
}
