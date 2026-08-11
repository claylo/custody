//! Comparison of recorded section paths against paths computed from source.

/// Compare a recorded heading path against the one computed from the Markdown.
#[must_use]
pub fn paths_match(recorded: &[String], computed: &[String]) -> bool {
    recorded == computed
}

/// Whether any element of the section path matches a weak heading.
///
/// Matched case-insensitively as a substring, because converter output varies:
/// `5. Limitations`, `Limitations and Future Work`, etc.
#[must_use]
pub fn is_weak_section(section: &[String], lowercase_weak_list: &[String]) -> bool {
    if lowercase_weak_list.is_empty() {
        return false;
    }
    section.iter().any(|heading| {
        let lower = heading.to_lowercase();
        lowercase_weak_list.iter().any(|weak| lower.contains(weak))
    })
}
