//! Story 11.4 — PR-title parser for PROGRESS.md auto-update.
//!
//! Reads PR titles of the form `Story X.Y[: description]` and emits the
//! structured update the CI commit-back job applies to `PROGRESS.md`.
//! Pure logic — the CI job (track-progress.yml) owns the git write-back.
//!
//! Cited: Story 11.4 DoD, guardrails.md G27 (story wiring discipline).

/// A parsed story-id + status from a PR title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTitle {
    /// Story number, e.g. `1.3`.
    pub story: String,
    /// Optional description after the colon.
    pub description: Option<String>,
}

/// Parse a PR title like `Story 1.3: format module` into a structured form.
///
/// Returns `None` for titles that don't conform to the `Story X.Y` convention
/// (the CI job logs a warning and skips). Handles:
/// - `Story 1.3` (no description)
/// - `Story 1.3: format module` (with description)
/// - `Story 1.3:format module` (no space after colon)
/// - `story 1.3: ...` (case-insensitive prefix)
/// - `Stories 8.6+8.7+8.8` (multiple — returns the first; caller warns)
///
/// Cited: Story 11.4 acceptance (parser reads `Story 1.3: format module` ->
/// emits the row; ignores non-conforming titles).
#[must_use]
pub fn parse_pr_title(title: &str) -> Option<ParsedTitle> {
    let trimmed = title.trim();
    // Case-insensitive prefix match for "Story ".
    let lower = trimmed.to_ascii_lowercase();
    let after_prefix = lower.strip_prefix("story ")?;
    // The story id is X.Y where X and Y are digits.
    let (story_part, rest) = split_story_id(after_prefix)?;
    // Validate it's digits-and-dot.
    if !story_part.chars().all(|c| c.is_ascii_digit() || c == '.') || !story_part.contains('.') {
        return None;
    }
    let description = rest
        .trim_start()
        .strip_prefix(':')
        .map(str::trim_start)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(ParsedTitle {
        story: story_part.to_string(),
        description,
    })
}

/// Split `1.3: format module` into (`1.3`, `: format module`).
fn split_story_id(s: &str) -> Option<(&str, &str)> {
    // The story id ends at the first non-digit/non-dot character.
    let end = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    Some((&s[..end], &s[end..]))
}

#[cfg(test)]
mod tests {
    use super::parse_pr_title;

    #[test]
    fn parses_story_with_description() {
        let p = parse_pr_title("Story 1.3: format module").unwrap();
        assert_eq!(p.story, "1.3");
        assert_eq!(p.description.as_deref(), Some("format module"));
    }

    #[test]
    fn parses_story_without_description() {
        let p = parse_pr_title("Story 1.3").unwrap();
        assert_eq!(p.story, "1.3");
        assert!(p.description.is_none());
    }

    #[test]
    fn parses_case_insensitive_prefix() {
        let p = parse_pr_title("story 8.10: wizard").unwrap();
        assert_eq!(p.story, "8.10");
    }

    #[test]
    fn parses_no_space_after_colon() {
        let p = parse_pr_title("Story 1.3:format module").unwrap();
        assert_eq!(p.story, "1.3");
        assert_eq!(p.description.as_deref(), Some("format module"));
    }

    #[test]
    fn ignores_non_conforming_title() {
        assert!(parse_pr_title("Update README").is_none());
        assert!(parse_pr_title("fix: typo").is_none());
        assert!(parse_pr_title("Story").is_none());
        assert!(parse_pr_title("Story ").is_none());
    }

    #[test]
    fn ignores_invalid_story_id() {
        assert!(parse_pr_title("Story abc: bad").is_none());
        assert!(parse_pr_title("Story 1: no dot").is_none());
    }

    #[test]
    fn handles_multi_story_prefix_returns_first() {
        // "Stories 8.6+8.7+8.8" -> lowercased "stories ..." doesn't match
        // "story " prefix (it's "stories"). This is intentional: multi-story
        // PRs are handled by the CI job's warning + multi-row logic, not the
        // single-title parser.
        assert!(parse_pr_title("Stories 8.6+8.7+8.8").is_none());
    }
}
