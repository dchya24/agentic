use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::io::{self, stdout, Write};
use termcolor::{Color, ColorSpec, StandardStream, WriteColor};

pub struct MarkdownRenderer {
    stdout: StandardStream,
    in_code_block: bool,
    code_lang: String,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        let stdout = StandardStream::stdout(termcolor::ColorChoice::Always);
        Self {
            stdout,
            in_code_block: false,
            code_lang: String::new(),
        }
    }

    pub fn render(&mut self, markdown: &str) -> io::Result<()> {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);

        let parser = Parser::new_ext(markdown, options);

        for event in parser {
            self.render_event(event)?;
        }

        Ok(())
    }

    fn render_event(&mut self, event: Event) -> io::Result<()> {
        match event {
            Event::Start(tag) => self.render_start_tag(tag),
            Event::End(tag_end) => self.render_end_tag(tag_end),
            Event::Text(text) => self.render_text(&text),
            Event::Code(code) => self.render_code(&code),
            Event::SoftBreak => {
                print!(" ");
                Ok(())
            }
            Event::HardBreak => {
                println!();
                Ok(())
            }
            Event::Rule => {
                println!("\n────────────────────────────────────\n");
                Ok(())
            }
            Event::TaskListMarker(checked) => {
                if checked {
                    print!("✓ ");
                } else {
                    print!("[ ] ");
                }
                stdout().flush()
            }
            _ => Ok(()),
        }
    }

    fn render_start_tag(&mut self, tag: Tag) -> io::Result<()> {
        match tag {
            Tag::Heading { level, .. } => {
                let level_num = match level {
                    pulldown_cmark::HeadingLevel::H1 => 1,
                    pulldown_cmark::HeadingLevel::H2 => 2,
                    pulldown_cmark::HeadingLevel::H3 => 3,
                    pulldown_cmark::HeadingLevel::H4 => 4,
                    pulldown_cmark::HeadingLevel::H5 => 5,
                    pulldown_cmark::HeadingLevel::H6 => 6,
                };
                let color = match level_num {
                    1 => Color::Rgb(255, 165, 0),
                    2 => Color::Rgb(135, 206, 235),
                    3 => Color::Rgb(144, 238, 144),
                    _ => Color::White,
                };
                self.stdout
                    .set_color(ColorSpec::new().set_bold(true).set_fg(Some(color)))?;
                print!("{} ", "#".repeat(level_num));
            }
            Tag::Paragraph => {}
            Tag::Strong => {
                self.stdout.set_color(ColorSpec::new().set_bold(true))?;
            }
            Tag::Emphasis => {
                self.stdout.set_color(ColorSpec::new().set_italic(true))?;
            }
            Tag::CodeBlock(kind) => {
                self.in_code_block = true;
                self.code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                    pulldown_cmark::CodeBlockKind::Indented => "text".to_string(),
                };
                self.stdout
                    .set_color(ColorSpec::new().set_fg(Some(Color::Rgb(46, 204, 113))))?;
                println!(
                    "\n┌─ {} ─",
                    if self.code_lang.is_empty() {
                        "code"
                    } else {
                        &self.code_lang
                    }
                );
                print!("│ ");
            }
            Tag::Link { .. } => {
                self.stdout.set_color(
                    ColorSpec::new()
                        .set_underline(true)
                        .set_fg(Some(Color::Rgb(52, 152, 219))),
                )?;
            }
            Tag::BlockQuote => {
                self.stdout
                    .set_color(ColorSpec::new().set_fg(Some(Color::Rgb(149, 165, 166))))?;
                print!("│ ");
            }
            Tag::List(_) => {}
            Tag::Item => {
                self.stdout
                    .set_color(ColorSpec::new().set_fg(Some(Color::Rgb(155, 89, 182))))?;
                print!("• ");
                self.stdout.reset()?;
            }
            Tag::Table(_) => {}
            Tag::TableHead => {
                self.stdout.set_color(ColorSpec::new().set_bold(true))?;
            }
            Tag::TableRow => {}
            Tag::TableCell => {}
            _ => {}
        }
        Ok(())
    }

    fn render_end_tag(&mut self, tag_end: TagEnd) -> io::Result<()> {
        match tag_end {
            TagEnd::Heading(_) => {
                self.stdout.reset()?;
                println!();
            }
            TagEnd::Paragraph => {
                println!();
            }
            TagEnd::Strong => {
                self.stdout.reset()?;
            }
            TagEnd::Emphasis => {
                self.stdout.reset()?;
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.stdout.reset()?;
                println!("\n└────────────────────────────────────\n");
            }
            TagEnd::Link => {
                self.stdout.reset()?;
            }
            TagEnd::BlockQuote => {
                self.stdout.reset()?;
                println!();
            }
            TagEnd::Item => {
                println!();
            }
            TagEnd::Table => {
                println!();
            }
            TagEnd::TableHead => {
                self.stdout.reset()?;
            }
            TagEnd::TableRow => {
                println!();
            }
            TagEnd::TableCell => {
                print!(" | ");
            }
            _ => {}
        }
        Ok(())
    }

    fn render_text(&mut self, text: &str) -> io::Result<()> {
        if self.in_code_block {
            self.stdout.set_color(ColorSpec::new().set_dimmed(true))?;
        }
        print!("{}", text);
        self.stdout.reset()?;
        Ok(())
    }

    fn render_code(&mut self, code: &str) -> io::Result<()> {
        self.stdout
            .set_color(ColorSpec::new().set_fg(Some(Color::Rgb(241, 196, 15))))?;
        print!("{}", code);
        self.stdout.reset()?;
        Ok(())
    }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn render_markdown(markdown: &str) -> io::Result<()> {
    let mut renderer = MarkdownRenderer::new();
    renderer.render(markdown)
}
