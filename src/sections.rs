//! Comparison of recorded section paths against paths computed from source.

/// Compare a recorded heading path against the one computed from the Markdown.
#[must_use]
pub fn paths_match(recorded: &[String], computed: &[String]) -> bool {
    recorded == computed
}
