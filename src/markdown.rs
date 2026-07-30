use anyhow::{Result, bail};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use crate::normalize::normalize;

/// Semantic Markdown boundaries accepted by evidence locators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Heading,
    Paragraph,
    ListItem,
    Blockquote,
    TableCell,
    CodeBlock,
}

/// Normalized visible text and source coordinates for one semantic unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownUnit {
    pub kind: UnitKind,
    pub text: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug)]
struct UnitBuilder {
    kind: UnitKind,
    text: String,
    start: usize,
}

/// Parse Markdown into bounded evidence units.
#[must_use]
pub fn parse_units(source: &str) -> Vec<MarkdownUnit> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH;
    let parser = Parser::new_ext(source, options).into_offset_iter();
    let mut units = Vec::new();
    let mut active: Option<UnitBuilder> = None;
    let mut item_depth = 0_usize;
    let mut blockquote_depth = 0_usize;
    let mut image_depth = 0_usize;
    let mut html_block_depth = 0_usize;

    for (event, range) in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Item => {
                    item_depth += 1;
                    if active.is_none() && html_block_depth == 0 {
                        active = Some(UnitBuilder::new(UnitKind::ListItem, range.start));
                    }
                }
                Tag::BlockQuote(_) => blockquote_depth += 1,
                Tag::Image { .. } => image_depth += 1,
                Tag::HtmlBlock => html_block_depth += 1,
                Tag::Paragraph if html_block_depth == 0 && active.is_none() => {
                    let kind = if item_depth > 0 {
                        UnitKind::ListItem
                    } else if blockquote_depth > 0 {
                        UnitKind::Blockquote
                    } else {
                        UnitKind::Paragraph
                    };
                    active = Some(UnitBuilder::new(kind, range.start));
                }
                Tag::Heading { .. } if html_block_depth == 0 => {
                    active = Some(UnitBuilder::new(UnitKind::Heading, range.start));
                }
                Tag::TableCell if html_block_depth == 0 => {
                    active = Some(UnitBuilder::new(UnitKind::TableCell, range.start));
                }
                Tag::CodeBlock(_) if html_block_depth == 0 => {
                    active = Some(UnitBuilder::new(UnitKind::CodeBlock, range.start));
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::TableCell | TagEnd::CodeBlock => {
                    finish_unit(source, &mut active, &mut units);
                }
                TagEnd::Item => {
                    finish_unit(source, &mut active, &mut units);
                    item_depth = item_depth.saturating_sub(1);
                }
                TagEnd::BlockQuote(_) => {
                    blockquote_depth = blockquote_depth.saturating_sub(1);
                }
                TagEnd::Image => image_depth = image_depth.saturating_sub(1),
                TagEnd::HtmlBlock => html_block_depth = html_block_depth.saturating_sub(1),
                _ => {}
            },
            Event::Text(text)
            | Event::Code(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text)
                if image_depth == 0 && html_block_depth == 0 =>
            {
                if let Some(builder) = active.as_mut() {
                    builder.text.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak if image_depth == 0 && html_block_depth == 0 => {
                if let Some(builder) = active.as_mut() {
                    builder.text.push(' ');
                }
            }
            Event::InlineHtml(tag) if image_depth == 0 && html_block_depth == 0 => {
                let trimmed = tag.trim_start();
                if trimmed.len() >= 3
                    && trimmed[..3].eq_ignore_ascii_case("<br")
                    && let Some(builder) = active.as_mut()
                {
                    builder.text.push(' ');
                }
            }
            Event::FootnoteReference(label) if image_depth == 0 && html_block_depth == 0 => {
                if let Some(builder) = active.as_mut() {
                    builder.text.push_str(&label);
                }
            }
            Event::TaskListMarker(checked) if image_depth == 0 && html_block_depth == 0 => {
                if let Some(builder) = active.as_mut() {
                    builder.text.push_str(if checked { "[x] " } else { "[ ] " });
                }
            }
            _ => {}
        }
    }

    finish_unit(source, &mut active, &mut units);
    units
}

/// Resolve exactly one unit at one-based source coordinates.
pub fn resolve_unit(
    units: &[MarkdownUnit],
    kind: UnitKind,
    line: usize,
    column: usize,
) -> Result<&MarkdownUnit> {
    if line == 0 || column == 0 {
        bail!("Markdown line and column must be one-based");
    }
    let mut matches = units
        .iter()
        .filter(|unit| unit.kind == kind && unit.line == line && unit.column == column);
    let Some(found) = matches.next() else {
        bail!("no {kind:?} unit starts at line {line}, column {column}");
    };
    if matches.next().is_some() {
        bail!("multiple {kind:?} units start at line {line}, column {column}");
    }
    Ok(found)
}

/// Count non-overlapping literal occurrences.
#[must_use]
pub fn exact_count(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack.match_indices(needle).count()
}

impl UnitBuilder {
    fn new(kind: UnitKind, start: usize) -> Self {
        Self {
            kind,
            text: String::new(),
            start,
        }
    }
}

fn finish_unit(source: &str, active: &mut Option<UnitBuilder>, units: &mut Vec<MarkdownUnit>) {
    let Some(builder) = active.take() else {
        return;
    };
    let text = normalize(&builder.text);
    if text.is_empty() {
        return;
    }
    let (line, column) = line_column(source, builder.start);
    units.push(MarkdownUnit {
        kind: builder.kind,
        text,
        line,
        column,
    });
}

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, tail)| tail)
        .chars()
        .count()
        + 1;
    (line, column)
}
