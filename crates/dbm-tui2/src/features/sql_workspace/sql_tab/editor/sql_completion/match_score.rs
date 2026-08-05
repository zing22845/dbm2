/// Score how well `prefix` matches `candidate`.
/// Returns -1 for no match, or a positive score where higher = better match.
///
/// Tiers (ported from DBX `computeMatchScore`):
/// exact > initials > prefix > substring > tight fuzzy > loose fuzzy
pub fn match_score(candidate: &str, prefix: &str) -> i32 {
    if prefix.is_empty() {
        return 1;
    }
    let c = candidate.to_ascii_lowercase();
    let p = prefix.to_ascii_lowercase();

    if c == p {
        return 3000 - c.len() as i32;
    }
    if c.starts_with(&p) {
        return 2000 - c.len() as i32;
    }

    if let Some(initials) = identifier_initials(candidate) {
        let initials_lower = initials.to_ascii_lowercase();
        if initials_lower.starts_with(&p) {
            let exact_initials_bonus = if initials_lower == p { 400 } else { 0 };
            return 2400 + exact_initials_bonus - c.len() as i32;
        }
    }

    if let Some(index) = c.find(&p) {
        let boundary_bonus = if is_identifier_boundary(candidate, index) {
            400
        } else {
            (180 - index as i32 * 12).max(0)
        };
        return 900 + boundary_bonus - c.len() as i32;
    }

    fuzzy_match_score(candidate, &c, &p)
}

pub fn matches_prefix(candidate: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    match_score(candidate, prefix) >= 0
}

fn fuzzy_match_score(candidate: &str, c: &str, p: &str) -> i32 {
    let mut ci = 0usize;
    let mut total_gap = 0i32;
    let mut first_match_pos = -1i32;
    let mut boundary_bonus = 0i32;

    for ch in p.chars() {
        let Some(rel) = c[ci..].find(ch) else {
            return -1;
        };
        let next_pos = ci + rel;
        if first_match_pos < 0 {
            first_match_pos = next_pos as i32;
        }
        if is_identifier_boundary(candidate, next_pos) {
            boundary_bonus += 40;
        }
        total_gap += next_pos as i32 - ci as i32;
        ci = next_pos + ch.len_utf8();
    }

    let early_match_bonus = (700 - first_match_pos * 35).max(0) + boundary_bonus;

    if total_gap >= p.len() as i32 {
        return (400.0 + early_match_bonus as f32 * 0.3 - total_gap as f32 * 20.0 - c.len() as f32)
            as i32;
    }

    let gap_penalty = total_gap * 10;
    1200 + early_match_bonus - gap_penalty - c.len() as i32
}

fn identifier_initials(candidate: &str) -> Option<String> {
    let words = identifier_words(candidate);
    if words.is_empty() {
        return None;
    }
    let initials: String = words.iter().filter_map(|w| w.chars().next()).collect();
    if initials.is_empty() {
        None
    } else {
        Some(initials)
    }
}

fn identifier_words(candidate: &str) -> Vec<String> {
    let mut normalized = String::with_capacity(candidate.len() + 4);
    let chars: Vec<char> = candidate.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if index > 0
            && ch.is_ascii_uppercase()
            && (chars[index - 1].is_ascii_lowercase() || chars[index - 1].is_ascii_digit())
        {
            normalized.push('_');
        }
        normalized.push(ch.to_ascii_lowercase());
    }
    normalized
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_identifier_boundary(candidate: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let chars: Vec<char> = candidate.chars().collect();
    let previous = chars.get(index.saturating_sub(1)).copied().unwrap_or('\0');
    let current = chars.get(index).copied().unwrap_or('\0');
    !previous.is_ascii_alphanumeric()
        || ((previous.is_ascii_lowercase() || previous.is_ascii_digit())
            && current.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_match_scores_higher_than_substring() {
        assert!(match_score("WHERE", "w") > match_score("VIEW", "w"));
    }

    #[test]
    fn substring_matches_wh() {
        assert!(matches_prefix("WHERE", "wh"));
        assert!(!matches_prefix("SELECT", "wh"));
    }

    #[test]
    fn fuzzy_matches_typo() {
        assert!(matches_prefix("SELECT", "selct"));
        assert!(
            match_score("SELECT", "sel") > match_score("SELECT", "selct"),
            "prefix should beat typo fuzzy"
        );
    }

    #[test]
    fn initials_match_group_by() {
        assert!(matches_prefix("GROUP BY", "gb"));
    }
}
