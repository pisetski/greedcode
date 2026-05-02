use std::io::Write;

use anyhow::Result;
use clap::ValueEnum;
use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd, TextMergeStream,
};

const ANSI_RESET: &str = "\x1b[0m";

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum MarkdownMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResolvedOutputMode {
    Plain,
    Markdown,
}

pub struct ResponseWriter<W: Write> {
    writer: W,
    mode: ResolvedOutputMode,
    markdown: MarkdownState,
}

impl<W: Write> ResponseWriter<W> {
    pub fn new(writer: W, mode: MarkdownMode, stdout_is_terminal: bool) -> Self {
        let mode = match mode {
            MarkdownMode::Always => ResolvedOutputMode::Markdown,
            MarkdownMode::Never => ResolvedOutputMode::Plain,
            MarkdownMode::Auto => {
                if stdout_is_terminal {
                    ResolvedOutputMode::Markdown
                } else {
                    ResolvedOutputMode::Plain
                }
            }
        };

        Self {
            writer,
            mode,
            markdown: MarkdownState::default(),
        }
    }

    pub fn push_delta(&mut self, chunk: &str) -> Result<()> {
        match self.mode {
            ResolvedOutputMode::Plain => {
                self.writer.write_all(chunk.as_bytes())?;
                self.writer.flush()?;
            }
            ResolvedOutputMode::Markdown => {
                self.markdown.push(chunk);
                let flush_idx = self.markdown.largest_safe_prefix();
                if flush_idx > 0 {
                    let stable = self.markdown.take_prefix(flush_idx);
                    let rendered = render_markdown_to_ansi(&stable);
                    self.writer.write_all(rendered.as_bytes())?;
                    self.writer.flush()?;
                }
            }
        }

        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        if self.mode == ResolvedOutputMode::Markdown && !self.markdown.is_empty() {
            let rendered = render_markdown_to_ansi(self.markdown.take_all().as_str());
            self.writer.write_all(rendered.as_bytes())?;
        }

        self.writer.flush()?;
        Ok(())
    }

    #[cfg(test)]
    fn into_inner(self) -> W {
        self.writer
    }
}

#[cfg(test)]
impl<W> ResponseWriter<W>
where
    W: Write + AsRef<[u8]>,
{
    fn snapshot(&self) -> String {
        String::from_utf8(self.writer.as_ref().to_vec()).unwrap()
    }
}

#[derive(Default)]
struct MarkdownState {
    raw_buffer: String,
}

impl MarkdownState {
    fn push(&mut self, chunk: &str) {
        self.raw_buffer.push_str(chunk);
    }

    fn is_empty(&self) -> bool {
        self.raw_buffer.is_empty()
    }

    fn take_prefix(&mut self, idx: usize) -> String {
        self.raw_buffer.drain(..idx).collect()
    }

    fn take_all(&mut self) -> String {
        std::mem::take(&mut self.raw_buffer)
    }

    fn largest_safe_prefix(&self) -> usize {
        let mut scanner = BoundaryScanner::default();

        for line in self.raw_buffer.split_inclusive('\n') {
            scanner.observe_line(line);
        }

        scanner.last_safe_idx
    }
}

#[derive(Clone, Copy, Debug)]
struct FenceState {
    marker: char,
    len: usize,
}

#[derive(Default)]
struct BoundaryScanner {
    idx: usize,
    last_safe_idx: usize,
    fence: Option<FenceState>,
}

impl BoundaryScanner {
    fn observe_line(&mut self, line: &str) {
        self.idx += line.len();
        let trimmed = line.trim_end_matches(['\r', '\n']);

        if let Some(fence) = parse_fence_line(trimmed) {
            match self.fence {
                Some(open) if open.marker == fence.marker && fence.len >= open.len => {
                    self.fence = None;
                    self.last_safe_idx = self.idx;
                    return;
                }
                None => {
                    self.fence = Some(fence);
                    return;
                }
                _ => return,
            }
        }

        if self.fence.is_none() && trimmed.is_empty() {
            self.last_safe_idx = self.idx;
        }
    }
}

fn parse_fence_line(line: &str) -> Option<FenceState> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    if len < 3 {
        return None;
    }

    Some(FenceState { marker, len })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InlineStyle {
    Heading(HeadingLevel),
    Strong,
    Emphasis,
    Strikethrough,
    Link,
    InlineCode,
}

#[derive(Clone, Debug)]
struct ListState {
    next_index: Option<u64>,
}

#[derive(Clone, Debug)]
struct LinkState {
    dest: String,
}

struct RenderState {
    output: String,
    styles: Vec<InlineStyle>,
    lists: Vec<ListState>,
    link_stack: Vec<LinkState>,
    blockquote_depth: usize,
    list_item_depth: usize,
    in_code_block: bool,
    pending_code_block_label: Option<String>,
}

impl RenderState {
    fn new() -> Self {
        Self {
            output: String::new(),
            styles: Vec::new(),
            lists: Vec::new(),
            link_stack: Vec::new(),
            blockquote_depth: 0,
            list_item_depth: 0,
            in_code_block: false,
            pending_code_block_label: None,
        }
    }

    fn write_text(&mut self, text: &str) {
        self.output.push_str(&sanitize_text(text));
    }

    fn write_prefixed_linebreak(&mut self) {
        self.output.push('\n');
        if self.blockquote_depth > 0 {
            self.output.push_str("\x1b[2m> \x1b[0m");
            self.reapply_styles();
        }
    }

    fn push_style(&mut self, style: InlineStyle) {
        self.styles.push(style);
        self.output.push_str(style_code(style));
    }

    fn pop_style(&mut self, style: InlineStyle) {
        if let Some(pos) = self.styles.iter().rposition(|active| *active == style) {
            self.styles.remove(pos);
            self.output.push_str(ANSI_RESET);
            self.reapply_styles();
        }
    }

    fn reapply_styles(&mut self) {
        for style in &self.styles {
            self.output.push_str(style_code(*style));
        }
    }

    fn ensure_block_gap(&mut self) {
        if self.output.is_empty() {
            return;
        }

        if self.output.ends_with("\n\n") {
            return;
        }

        if self.output.ends_with('\n') {
            self.output.push('\n');
        } else {
            self.output.push_str("\n\n");
        }
    }

    fn finish_paragraph(&mut self) {
        if self.list_item_depth > 0 {
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
        } else if !self.output.ends_with("\n\n") {
            if self.output.ends_with('\n') {
                self.output.push('\n');
            } else {
                self.output.push_str("\n\n");
            }
        }
    }

    fn start_list_item(&mut self) {
        let indent = "  ".repeat(self.lists.len().saturating_sub(1));
        self.output.push_str(&indent);
        if let Some(list) = self.lists.last_mut() {
            if let Some(next_index) = &mut list.next_index {
                self.output.push_str(&format!("{}. ", *next_index));
                *next_index += 1;
            } else {
                self.output.push_str("- ");
            }
        } else {
            self.output.push_str("- ");
        }
        self.list_item_depth += 1;
    }

    fn end_list_item(&mut self) {
        self.list_item_depth = self.list_item_depth.saturating_sub(1);
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    fn start_code_block(&mut self, kind: &CodeBlockKind<'_>) {
        self.ensure_block_gap();
        self.in_code_block = true;
        self.pending_code_block_label = match kind {
            CodeBlockKind::Indented => None,
            CodeBlockKind::Fenced(info) => {
                let label = info.split_whitespace().next().unwrap_or_default().trim();
                if label.is_empty() {
                    None
                } else {
                    Some(label.to_string())
                }
            }
        };

        if let Some(label) = &self.pending_code_block_label {
            self.output.push_str("\x1b[1;90m");
            self.output.push_str(label);
            self.output.push_str("\x1b[0m\n");
        }

        self.output.push_str("\x1b[38;5;252;48;5;236m");
    }

    fn end_code_block(&mut self) {
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.output.push_str(ANSI_RESET);
        self.output.push('\n');
        self.in_code_block = false;
        self.pending_code_block_label = None;
    }
}

fn render_markdown_to_ansi(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = TextMergeStream::new(Parser::new_ext(markdown, options));
    let mut state = RenderState::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => {
                    state.ensure_block_gap();
                    state.push_style(InlineStyle::Heading(level));
                }
                Tag::BlockQuote(_) => {
                    state.ensure_block_gap();
                    state.blockquote_depth += 1;
                    state.output.push_str("\x1b[2m> \x1b[0m");
                    state.reapply_styles();
                }
                Tag::CodeBlock(kind) => state.start_code_block(&kind),
                Tag::List(start) => state.lists.push(ListState { next_index: start }),
                Tag::Item => state.start_list_item(),
                Tag::Emphasis => state.push_style(InlineStyle::Emphasis),
                Tag::Strong => state.push_style(InlineStyle::Strong),
                Tag::Strikethrough => state.push_style(InlineStyle::Strikethrough),
                Tag::Link { dest_url, .. } => {
                    state.link_stack.push(LinkState {
                        dest: dest_url.into_string(),
                    });
                    state.push_style(InlineStyle::Link);
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => state.finish_paragraph(),
                TagEnd::Heading(_) => {
                    state.pop_style(InlineStyle::Heading(HeadingLevel::H1));
                    state.pop_style(InlineStyle::Heading(HeadingLevel::H2));
                    state.pop_style(InlineStyle::Heading(HeadingLevel::H3));
                    state.pop_style(InlineStyle::Heading(HeadingLevel::H4));
                    state.pop_style(InlineStyle::Heading(HeadingLevel::H5));
                    state.pop_style(InlineStyle::Heading(HeadingLevel::H6));
                    state.finish_paragraph();
                }
                TagEnd::BlockQuote(_) => {
                    state.blockquote_depth = state.blockquote_depth.saturating_sub(1);
                    state.finish_paragraph();
                }
                TagEnd::CodeBlock => state.end_code_block(),
                TagEnd::List(_) => {
                    state.lists.pop();
                    if !state.output.ends_with('\n') {
                        state.output.push('\n');
                    }
                }
                TagEnd::Item => state.end_list_item(),
                TagEnd::Emphasis => state.pop_style(InlineStyle::Emphasis),
                TagEnd::Strong => state.pop_style(InlineStyle::Strong),
                TagEnd::Strikethrough => state.pop_style(InlineStyle::Strikethrough),
                TagEnd::Link => {
                    state.pop_style(InlineStyle::Link);
                    if let Some(link) = state.link_stack.pop() {
                        state.output.push(' ');
                        state.output.push_str("\x1b[2m<");
                        state.write_text(&link.dest);
                        state.output.push_str(">\x1b[0m");
                        state.reapply_styles();
                    }
                }
                _ => {}
            },
            Event::Text(text) => state.write_text(&text),
            Event::Code(text) => {
                state.push_style(InlineStyle::InlineCode);
                state.write_text(&text);
                state.pop_style(InlineStyle::InlineCode);
            }
            Event::SoftBreak => {
                if state.in_code_block {
                    state.write_prefixed_linebreak();
                } else {
                    state.output.push(' ');
                }
            }
            Event::HardBreak => state.write_prefixed_linebreak(),
            Event::Rule => {
                state.ensure_block_gap();
                state.output.push_str("\x1b[2m────────────────\x1b[0m\n\n");
            }
            Event::TaskListMarker(checked) => {
                if checked {
                    state.output.push_str("[x] ");
                } else {
                    state.output.push_str("[ ] ");
                }
            }
            Event::Html(text)
            | Event::InlineHtml(text)
            | Event::DisplayMath(text)
            | Event::InlineMath(text)
            | Event::FootnoteReference(text) => state.write_text(&text),
        }
    }

    if !state.output.ends_with('\n') {
        state.output.push('\n');
    }

    state.output
}

fn style_code(style: InlineStyle) -> &'static str {
    match style {
        InlineStyle::Heading(HeadingLevel::H1) => "\x1b[1;96m",
        InlineStyle::Heading(HeadingLevel::H2) => "\x1b[1;94m",
        InlineStyle::Heading(_) => "\x1b[1m",
        InlineStyle::Strong => "\x1b[1m",
        InlineStyle::Emphasis => "\x1b[3m",
        InlineStyle::Strikethrough => "\x1b[9m",
        InlineStyle::Link => "\x1b[4;34m",
        InlineStyle::InlineCode => "\x1b[38;5;214;48;5;236m",
    }
}

fn sanitize_text(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            sanitized.push(ch);
            continue;
        }

        if chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if matches!(next, '@'..='~') {
                    break;
                }
            }
        }
    }

    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn into_string(writer: ResponseWriter<Vec<u8>>) -> String {
        String::from_utf8(writer.into_inner()).unwrap()
    }

    #[test]
    fn plain_mode_passthrough() {
        let mut writer = ResponseWriter::new(Vec::new(), MarkdownMode::Never, true);
        writer.push_delta("**hello**").unwrap();
        writer.finish().unwrap();

        assert_eq!(into_string(writer), "**hello**");
    }

    #[test]
    fn auto_mode_uses_plain_for_non_terminal() {
        let mut writer = ResponseWriter::new(Vec::new(), MarkdownMode::Auto, false);
        writer.push_delta("# hi\n\n").unwrap();
        writer.finish().unwrap();

        assert_eq!(into_string(writer), "# hi\n\n");
    }

    #[test]
    fn paragraph_flushes_after_blank_line() {
        let mut writer = ResponseWriter::new(Vec::new(), MarkdownMode::Always, true);
        writer.push_delta("Hello **wor").unwrap();
        assert_eq!(writer.snapshot(), "");

        writer.push_delta("ld**\n\nNext").unwrap();
        let out = into_string(writer);
        assert!(out.contains("Hello "));
        assert!(out.contains("world"));
        assert!(out.contains("\x1b[1m"));
    }

    #[test]
    fn fenced_code_block_waits_for_closing_fence() {
        let mut writer = ResponseWriter::new(Vec::new(), MarkdownMode::Always, true);
        writer.push_delta("```rs\nfn main() {\n").unwrap();
        assert_eq!(writer.snapshot(), "");

        writer.push_delta("}\n```\n").unwrap();
        let out = into_string(writer);
        assert!(out.contains("fn main()"));
        assert!(out.contains("\x1b[38;5;252;48;5;236m"));
    }

    #[test]
    fn finish_flushes_remaining_tail() {
        let mut writer = ResponseWriter::new(Vec::new(), MarkdownMode::Always, true);
        writer.push_delta("A [link](https://example.com)").unwrap();
        writer.finish().unwrap();

        let out = into_string(writer);
        assert!(out.contains("link"));
        assert!(out.contains("https://example.com"));
        assert!(out.contains("\x1b[4;34m"));
    }

    #[test]
    fn renderer_strips_ansi_from_model_text() {
        let mut writer = ResponseWriter::new(Vec::new(), MarkdownMode::Always, true);
        writer.push_delta("Hello \x1b[31mred\x1b[0m\n\n").unwrap();

        let out = into_string(writer);
        assert!(out.contains("Hello red"));
        assert!(!out.contains("\x1b[31mred"));
    }
}
