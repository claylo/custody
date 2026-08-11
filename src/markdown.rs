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

impl UnitKind {
    /// The serialized name, so human output and JSON agree.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Heading => "heading",
            Self::Paragraph => "paragraph",
            Self::ListItem => "list_item",
            Self::Blockquote => "blockquote",
            Self::TableCell => "table_cell",
            Self::CodeBlock => "code_block",
        }
    }
}

/// Normalized visible text and source coordinates for one semantic unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownUnit {
    pub kind: UnitKind,
    pub text: String,
    pub line: usize,
    pub column: usize,
    /// Texts of the headings this unit sits under, outermost first.
    pub section: Vec<String>,
}

#[derive(Debug)]
struct UnitBuilder {
    kind: UnitKind,
    text: String,
    start: usize,
    heading_level: Option<u8>,
}

/// Parse Markdown into bounded evidence units.
#[must_use]
pub fn parse_units(source: &str) -> Vec<MarkdownUnit> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH;
    let parser = Parser::new_ext(source, options).into_offset_iter();
    let line_starts = build_line_starts(source);
    let mut units = Vec::new();
    let mut active: Option<UnitBuilder> = None;
    let mut item_depth = 0_usize;
    let mut blockquote_depth = 0_usize;
    let mut image_depth = 0_usize;
    let mut html_block_depth = 0_usize;
    let mut heading_stack: Vec<(u8, String)> = Vec::new();

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
                Tag::Heading { level, .. } if html_block_depth == 0 => {
                    let mut builder = UnitBuilder::new(UnitKind::Heading, range.start);
                    builder.heading_level = Some(level as u8);
                    active = Some(builder);
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
                    finish_unit(&line_starts, &mut active, &mut units, &mut heading_stack);
                }
                TagEnd::Item => {
                    finish_unit(&line_starts, &mut active, &mut units, &mut heading_stack);
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
                if trimmed
                    .as_bytes()
                    .get(..3)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"<br"))
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

    finish_unit(&line_starts, &mut active, &mut units, &mut heading_stack);
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
            heading_level: None,
        }
    }
}

fn finish_unit(
    line_starts: &[usize],
    active: &mut Option<UnitBuilder>,
    units: &mut Vec<MarkdownUnit>,
    heading_stack: &mut Vec<(u8, String)>,
) {
    let Some(builder) = active.take() else {
        return;
    };
    let text = normalize(&builder.text);
    if text.is_empty() {
        return;
    }
    let (line, column) = line_column_from_index(line_starts, builder.start);
    let section: Vec<String> = heading_stack
        .iter()
        .map(|(_, heading)| heading.clone())
        .collect();
    // A heading's own path is its ancestors, so record the path before it
    // replaces every same-or-deeper level on the stack.
    if let Some(level) = builder.heading_level {
        heading_stack.retain(|(open, _)| *open < level);
        heading_stack.push((level, text.clone()));
    }
    units.push(MarkdownUnit {
        kind: builder.kind,
        text,
        line,
        column,
        section,
    });
}

fn build_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn line_column_from_index(line_starts: &[usize], offset: usize) -> (usize, usize) {
    let line_index = line_starts
        .partition_point(|&start| start <= offset)
        .saturating_sub(1);
    let line_start = line_starts[line_index];
    let column = offset.saturating_sub(line_start) + 1;
    (line_index + 1, column)
}
