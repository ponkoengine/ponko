//! Near names for messages: the name a typo was probably meant to be.

/// The names in `known` closest to `name` (ignoring case), at most three,
/// when they are close enough to be what was meant.
pub(crate) fn suggestions<'a>(name: &str, known: &[&'a str]) -> Vec<&'a str> {
    let lower = name.to_lowercase();
    let mut scored: Vec<(usize, &str)> = known
        .iter()
        .map(|k| (edit_distance(&lower, &k.to_lowercase()), *k))
        .filter(|(d, k)| *d <= (k.chars().count().max(name.chars().count()) / 3).max(1) + 1)
        .collect();
    scored.sort();
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(3).map(|(_, k)| k).collect()
}

/// Edits (insert, delete, change, swap two neighbours) between `a` and `b`.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = d;
        }
    }
    rows[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_find_typos_and_case_slips_but_not_strangers() {
        let known = ["collider", "collides_with", "Health", "prefab"];
        assert_eq!(suggestions("colider", &known), ["collider"]);
        assert_eq!(suggestions("health", &known), ["Health"]);
        assert_eq!(suggestions("perfab", &known), ["prefab"]);
        assert!(suggestions("velocity", &known).is_empty());
    }
}
