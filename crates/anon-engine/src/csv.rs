//! Cellules d'un CSV, pour la détection seulement.
//!
//! Le gestionnaire de texte rend un bloc par ligne. Pour que les règles de
//! colonnes s'appliquent (en-tête « Nom », « Téléphone »…), on découpe ici
//! chaque ligne en cellules munies de leur en-tête. Ces blocs ne servent qu'à
//! détecter : le remplacement se fait toujours sur les lignes entières.

use anon_core::{BlockContext, BlockRole, TextBlock};

/// Séparateurs reconnus, par ordre de préférence à égalité.
const SEPARATORS: &[char] = &[';', ',', '\t', '|'];
/// Nombre minimal de colonnes pour traiter le fichier comme un tableau.
const MIN_COLUMNS: usize = 2;

/// Découpe une ligne en cellules (guillemets doubles `"` gérés, `""` = `"`).
fn split(line: &str, sep: char) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' && cur.trim().is_empty() {
            cur.clear();
            quoted = true;
        } else if c == sep {
            cells.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    cells.push(cur);
    cells
}

fn separator(header: &str) -> Option<char> {
    let mut best: Option<(char, usize)> = None;
    for &s in SEPARATORS {
        let n = header.matches(s).count();
        if n + 1 >= MIN_COLUMNS && best.is_none_or(|(_, b)| n > b) {
            best = Some((s, n));
        }
    }
    best.map(|(s, _)| s)
}

/// Blocs de détection d'un CSV : la ligne d'en-tête telle quelle, puis une
/// cellule par bloc avec son en-tête. `None` si le texte n'a pas l'air d'un tableau.
pub(crate) fn cell_blocks(lines: &[TextBlock]) -> Option<Vec<TextBlock>> {
    let (head, rest) = lines.split_first()?;
    let sep = separator(&head.text)?;
    let headers: Vec<String> = split(&head.text, sep)
        .into_iter()
        .map(|h| h.trim().to_string())
        .collect();
    if headers.len() < MIN_COLUMNS {
        return None;
    }
    let mut out = vec![head.clone()];
    for line in rest {
        for (j, cell) in split(&line.text, sep).into_iter().enumerate() {
            if cell.trim().is_empty() {
                continue;
            }
            out.push(TextBlock {
                text: cell,
                ctx: BlockContext {
                    part: format!("{}, colonne {}", line.ctx.part, j + 1),
                    role: BlockRole::Cell,
                    column_header: headers.get(j).cloned(),
                },
            });
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: &str) -> TextBlock {
        TextBlock {
            text: t.into(),
            ctx: BlockContext::new("ligne", BlockRole::Body),
        }
    }

    #[test]
    fn splits_with_headers() {
        let blocks = cell_blocks(&[line("Nom;Prénom;Note"), line("\"A;B\";Jean;")]).unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].text, "A;B");
        assert_eq!(blocks[1].ctx.column_header.as_deref(), Some("Nom"));
        assert_eq!(blocks[2].ctx.column_header.as_deref(), Some("Prénom"));
        assert_eq!(blocks[2].ctx.role, BlockRole::Cell);
    }

    #[test]
    fn plain_text_is_not_a_table() {
        assert!(cell_blocks(&[line("Bonjour à tous")]).is_none());
        assert!(cell_blocks(&[]).is_none());
        assert_eq!(split("a,\"b\"\"c\",d", ','), vec!["a", "b\"c", "d"]);
    }
}
