use std::fmt;

#[derive(Debug, Clone)]
pub enum OutputCategory {
    Thought,
    Tool,
    ToolOutput,
    System,
    Error,
}

impl OutputCategory {
    pub fn color(&self) -> &str {
        match self {
            OutputCategory::Thought => "\x1b[36m",    // Cyan
            OutputCategory::Tool => "\x1b[33m",       // Yellow
            OutputCategory::ToolOutput => "\x1b[32m", // Green
            OutputCategory::System => "\x1b[35m",     // Magenta
            OutputCategory::Error => "\x1b[31m",      // Red
        }
    }

    pub fn reset(&self) -> &str {
        "\x1b[0m"
    }
}

pub struct Output {
    pub category: OutputCategory,
    pub content: String,
    pub color: bool,
}

impl Output {
    pub fn new(category: OutputCategory, content: impl Into<String>) -> Self {
        Self {
            category,
            content: content.into(),
            color: true,
        }
    }

    pub fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    pub fn print(&self) {
        if self.color {
            print!(
                "{}{}{}",
                self.category.color(),
                self.content,
                self.category.reset()
            );
        } else {
            print!("{}", self.content);
        }
    }

    pub fn println(&self) {
        self.print();
        println!();
    }
}

impl fmt::Display for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.color {
            write!(
                f,
                "{}{}{}",
                self.category.color(),
                self.content,
                self.category.reset()
            )
        } else {
            write!(f, "{}", self.content)
        }
    }
}
