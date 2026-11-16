//! Déplacer le focus dans une grille de cases (le sélecteur d'emojis) à la flèche, sans I/O : le client
//! donne la place de chaque case à l'écran, ce module dit laquelle prend le focus.

/// Une touche de déplacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    /// Page précédente ou suivante : `PAGE_ROWS` lignes d'un coup.
    PageUp,
    PageDown,
    /// Début et fin de la ligne ; avec Ctrl, de toute la grille.
    Home,
    End,
    First,
    Last,
}

/// Lignes sautées par page précédente ou suivante.
pub const PAGE_ROWS: usize = 5;

impl Move {
    /// La touche (`KeyboardEvent.key`) et si Ctrl est enfoncé : le déplacement qu'ils demandent.
    pub fn from_key(key: &str, ctrl: bool) -> Option<Self> {
        Some(match (key, ctrl) {
            ("ArrowLeft", false) => Self::Left,
            ("ArrowRight", false) => Self::Right,
            ("ArrowUp", false) => Self::Up,
            ("ArrowDown", false) => Self::Down,
            ("PageUp", false) => Self::PageUp,
            ("PageDown", false) => Self::PageDown,
            ("Home", false) => Self::Home,
            ("End", false) => Self::End,
            ("Home", true) => Self::First,
            ("End", true) => Self::Last,
            _ => return None,
        })
    }
}

/// La case qui prend le focus depuis `from`. `cells` : pour chaque case, dans l'ordre du document, le haut
/// de sa ligne et le milieu de sa largeur (en pixels) ; les cases qui ont le même haut sont sur la même
/// ligne, que la grille compte une ou plusieurs rubriques. Gauche et droite passent à la ligne d'après ou
/// d'avant puis reviennent de l'autre bout ; haut et bas gardent la colonne la plus proche et font de même ;
/// les pages s'arrêtent à la première ou à la dernière ligne. Vide : `None`.
pub fn step(cells: &[(i32, i32)], from: usize, mv: Move) -> Option<usize> {
    let n = cells.len();
    if n == 0 {
        return None;
    }
    let from = from.min(n - 1);
    // Les lignes : suites de cases de même haut.
    let mut rows: Vec<std::ops::Range<usize>> = Vec::new();
    for (i, c) in cells.iter().enumerate() {
        match rows.last_mut() {
            Some(r) if cells[r.start].0 == c.0 => r.end = i + 1,
            _ => rows.push(i..i + 1),
        }
    }
    let row = rows.iter().position(|r| r.contains(&from))?;
    let near = |r: usize| rows[r].clone().min_by_key(|&i| (cells[i].1 - cells[from].1).abs());
    match mv {
        Move::Left => Some(if from == 0 { n - 1 } else { from - 1 }),
        Move::Right => Some(if from + 1 == n { 0 } else { from + 1 }),
        Move::Up => near(if row == 0 { rows.len() - 1 } else { row - 1 }),
        Move::Down => near(if row + 1 == rows.len() { 0 } else { row + 1 }),
        Move::PageUp => near(row.saturating_sub(PAGE_ROWS)),
        Move::PageDown => near((row + PAGE_ROWS).min(rows.len() - 1)),
        Move::Home => Some(rows[row].start),
        Move::End => Some(rows[row].end - 1),
        Move::First => Some(0),
        Move::Last => Some(n - 1),
    }
}

/// L'élément d'après (`forward`) ou d'avant dans une liste de `len` éléments, en revenant de l'autre bout.
/// Sans élément courant (`at` à `None`), le premier en avant et le dernier en arrière. Liste vide : `None`.
pub fn cycle(len: usize, at: Option<usize>, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (at.filter(|&i| i < len), forward) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_wraps_both_ways() {
        assert_eq!(cycle(3, Some(2), true), Some(0));
        assert_eq!(cycle(3, Some(0), false), Some(2));
        assert_eq!(cycle(3, Some(1), true), Some(2));
        assert_eq!(cycle(3, None, true), Some(0));
        assert_eq!(cycle(3, None, false), Some(2));
        assert_eq!(cycle(3, Some(9), true), Some(0));
        assert_eq!(cycle(0, None, true), None);
    }

    /// 3 colonnes de 40 px ; 7 cases dans la première rubrique, puis 2 dans une seconde (autres lignes).
    fn grid() -> Vec<(i32, i32)> {
        let mut cells: Vec<(i32, i32)> = (0..7).map(|i| (10 + 40 * (i / 3), 20 + 40 * (i % 3))).collect();
        cells.extend((0..2).map(|i| (200, 20 + 40 * i)));
        cells
    }

    #[test]
    fn left_and_right_wrap_across_lines_and_ends() {
        let g = grid();
        assert_eq!(step(&g, 2, Move::Right), Some(3));
        assert_eq!(step(&g, 3, Move::Left), Some(2));
        assert_eq!(step(&g, 8, Move::Right), Some(0));
        assert_eq!(step(&g, 0, Move::Left), Some(8));
    }

    #[test]
    fn up_and_down_keep_the_column_and_wrap() {
        let g = grid();
        assert_eq!(step(&g, 1, Move::Down), Some(4));
        // Une ligne d'une seule case (colonne 0) : on la prend, puis la rubrique suivante garde la colonne la plus proche.
        assert_eq!(step(&g, 4, Move::Down), Some(6));
        assert_eq!(step(&g, 6, Move::Down), Some(7));
        assert_eq!(step(&g, 7, Move::Up), Some(6));
        // Au bout de la grille, on repart de l'autre côté.
        assert_eq!(step(&g, 7, Move::Down), Some(0));
        assert_eq!(step(&g, 1, Move::Up), Some(8));
    }

    #[test]
    fn home_end_and_pages() {
        let g = grid();
        assert_eq!(step(&g, 4, Move::Home), Some(3));
        assert_eq!(step(&g, 4, Move::End), Some(5));
        assert_eq!(step(&g, 4, Move::First), Some(0));
        assert_eq!(step(&g, 4, Move::Last), Some(8));
        // Quatre lignes : une page de cinq lignes va jusqu'à la dernière (ou la première), sans passer de l'autre côté.
        assert_eq!(step(&g, 1, Move::PageDown), Some(8));
        assert_eq!(step(&g, 7, Move::PageUp), Some(0));
        assert_eq!(step(&g, 1, Move::PageUp), Some(1));
    }

    #[test]
    fn keys_map_to_moves() {
        assert_eq!(Move::from_key("ArrowUp", false), Some(Move::Up));
        assert_eq!(Move::from_key("Home", true), Some(Move::First));
        assert_eq!(Move::from_key("ArrowUp", true), None);
        assert_eq!(Move::from_key("a", false), None);
        assert_eq!(step(&[], 0, Move::Right), None);
        assert_eq!(step(&[(0, 0)], 5, Move::Left), Some(0));
    }
}
