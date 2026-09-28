//! Text normalization applied identically to literals, Markdown units, and PDF
//! page text before any comparison.
//!
//! Three rules, each independently configurable through `normalize:` in the
//! corpus configuration and all on by default:
//!
//! - **whitespace** (always on): every non-empty run of Unicode whitespace
//!   becomes one ASCII space.
//! - **dehyphenate**: a hyphen at the end of a line, followed by a line that
//!   starts with a lowercase letter, is a typesetter's line break inside a word.
//!   The hyphen and the break are removed (`adjust-\nment` → `adjustment`).
//!   Only whitespace runs containing a line break qualify; `self- report` on
//!   one line is left alone.
//! - **quotes**: typographic single and double quotes fold to their ASCII
//!   forms, because PDF text layers carry the curly forms and most converters
//!   emit the straight ones.
//!
//! Every input that reaches [`normalize`] must therefore preserve line breaks
//! as `\n` where the source had them; joining lines with a space before
//! normalization defeats dehyphenation.

use std::sync::OnceLock;

/// Which optional normalization rules are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizeOptions {
    pub dehyphenate: bool,
    pub quotes: bool,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self {
            dehyphenate: true,
            quotes: true,
        }
    }
}

impl NormalizeOptions {
    /// Whitespace collapsing only, matching the tool's original behaviour.
    #[must_use]
    pub const fn whitespace_only() -> Self {
        Self {
            dehyphenate: false,
            quotes: false,
        }
    }

    /// Short human-readable summary for `doctor` and extraction frontmatter.
    #[must_use]
    pub fn describe(self) -> String {
        format!(
            "whitespace, dehyphenate={}, quotes={}",
            on_off(self.dehyphenate),
            on_off(self.quotes)
        )
    }
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

static OPTIONS: OnceLock<NormalizeOptions> = OnceLock::new();

/// Install the process-wide options from the corpus configuration.
///
/// Later calls are ignored: normalization rules are a property of the corpus,
/// set once at startup, and must not change between comparisons.
pub fn configure(options: NormalizeOptions) {
    let _ = OPTIONS.set(options);
}

/// The process-wide options, or the defaults if [`configure`] was never called.
#[must_use]
pub fn current() -> NormalizeOptions {
    OPTIONS.get().copied().unwrap_or_default()
}

/// Normalize with the process-wide options.
#[must_use]
pub fn normalize(input: &str) -> String {
    normalize_with(input, current())
}

/// Normalize with explicit options.
#[must_use]
pub fn normalize_with(input: &str, options: NormalizeOptions) -> String {
    let chars: Vec<char> = if options.quotes {
        input.chars().map(fold_quote).collect()
    } else {
        input.chars().collect()
    };
    let mut output = String::with_capacity(input.len());
    let mut pending_space = false;
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index];
        if character.is_whitespace() {
            pending_space = !output.is_empty();
            index += 1;
            continue;
        }
        if options.dehyphenate
            && is_hyphen(character)
            && let Some(resume) = line_break_join(&chars, index + 1)
        {
            // Drop the hyphen and the whitespace run; the next character is
            // the continuation of the same word.
            index = resume;
            continue;
        }
        if pending_space {
            output.push(' ');
            pending_space = false;
        }
        output.push(character);
        index += 1;
    }

    output
}

/// If `chars[start..]` opens with a whitespace run that includes a line break
/// and is followed by a lowercase letter, return the index of that letter.
fn line_break_join(chars: &[char], start: usize) -> Option<usize> {
    let mut index = start;
    let mut saw_line_break = false;
    while let Some(&character) = chars.get(index) {
        if !character.is_whitespace() {
            break;
        }
        saw_line_break |= matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}');
        index += 1;
    }
    if !saw_line_break || index == start {
        return None;
    }
    let next = *chars.get(index)?;
    next.is_lowercase().then_some(index)
}

const fn is_hyphen(character: char) -> bool {
    matches!(character, '-' | '\u{00AD}' | '\u{2010}')
}

const fn fold_quote(character: char) -> char {
    match character {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        other => other,
    }
}
