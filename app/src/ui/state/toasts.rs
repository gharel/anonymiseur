//! Notifications discrètes (coin bas droit) : succès et informations
//! disparaissent après 3 s ; une erreur reste jusqu'à ce qu'on la ferme.
//! Le temps est passé en paramètre (secondes) : tout se teste sans horloge.

/// Durée d'affichage d'une notification ordinaire, en secondes.
pub const TOAST_SECONDS: f64 = 3.0;
/// Notifications affichées au plus ; les plus anciennes partent d'abord.
pub const MAX_TOASTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub text: String,
    /// Instant d'apparition (secondes).
    pub since: f64,
}

impl Toast {
    /// Une erreur reste affichée jusqu'à sa fermeture.
    pub fn is_sticky(&self) -> bool {
        self.kind == ToastKind::Error
    }

    /// Temps restant avant disparition (`None` : reste affichée).
    pub fn remaining(&self, now: f64) -> Option<f64> {
        (!self.is_sticky()).then(|| (self.since + TOAST_SECONDS - now).max(0.0))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Toasts {
    items: Vec<Toast>,
    next_id: u64,
}

impl Toasts {
    /// Ajoute une notification ; un message identique déjà affiché est
    /// renouvelé plutôt que doublé.
    pub fn push(&mut self, kind: ToastKind, text: impl Into<String>, now: f64) -> u64 {
        let text = text.into();
        if let Some(t) = self
            .items
            .iter_mut()
            .find(|t| t.kind == kind && t.text == text)
        {
            t.since = now;
            return t.id;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Toast {
            id,
            kind,
            text,
            since: now,
        });
        while self.items.len() > MAX_TOASTS {
            // On garde les erreurs : la plus ancienne notification ordinaire part.
            match self.items.iter().position(|t| !t.is_sticky()) {
                Some(i) => {
                    self.items.remove(i);
                }
                None => {
                    self.items.remove(0);
                }
            }
        }
        id
    }

    /// Retire les notifications échues.
    pub fn expire(&mut self, now: f64) {
        self.items
            .retain(|t| t.remaining(now).is_none_or(|r| r > 0.0));
    }

    pub fn dismiss(&mut self, id: u64) {
        self.items.retain(|t| t.id != id);
    }

    pub fn items(&self) -> &[Toast] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Prochaine échéance, pour redessiner à temps.
    pub fn next_deadline(&self, now: f64) -> Option<f64> {
        self.items
            .iter()
            .filter_map(|t| t.remaining(now))
            .fold(None, |acc: Option<f64>, r| {
                Some(acc.map_or(r, |a| a.min(r)))
            })
    }

    /// Textes affichés (tests).
    pub fn texts(&self) -> Vec<&str> {
        self.items.iter().map(|t| t.text.as_str()).collect()
    }
}

/// « 1 fichier ajouté », « 3 fichiers ajoutés ».
pub fn count_label(n: usize, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{n} {plural}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_toasts_expire_after_three_seconds() {
        let mut t = Toasts::default();
        t.push(ToastKind::Success, "Texte copié", 10.0);
        t.expire(12.9);
        assert_eq!(t.texts(), ["Texte copié"]);
        assert_eq!(t.next_deadline(12.0), Some(1.0));
        t.expire(13.0);
        assert!(t.is_empty());
    }

    #[test]
    fn errors_stay_until_dismissed() {
        let mut t = Toasts::default();
        let id = t.push(ToastKind::Error, "Écriture impossible", 0.0);
        t.expire(1000.0);
        assert_eq!(t.items().len(), 1);
        assert_eq!(t.next_deadline(1000.0), None);
        t.dismiss(id);
        assert!(t.is_empty());
    }

    #[test]
    fn toasts_stack_without_duplicates() {
        let mut t = Toasts::default();
        t.push(ToastKind::Error, "Erreur", 0.0);
        for i in 0..6 {
            t.push(ToastKind::Info, format!("Message {i}"), f64::from(i));
        }
        t.push(ToastKind::Info, "Message 5", 7.0);
        assert_eq!(t.items().len(), MAX_TOASTS);
        assert_eq!(t.texts()[0], "Erreur", "l'erreur reste");
        assert_eq!(t.texts().last(), Some(&"Message 5"));
        assert_eq!(t.items().last().map(|x| x.since), Some(7.0), "renouvelé");
    }

    #[test]
    fn counts() {
        assert_eq!(
            count_label(1, "fichier ajouté", "fichiers ajoutés"),
            "1 fichier ajouté"
        );
        assert_eq!(
            count_label(4, "fichier ajouté", "fichiers ajoutés"),
            "4 fichiers ajoutés"
        );
    }
}
