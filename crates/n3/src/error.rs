use std::fmt;

#[derive(Debug, Clone)]
/// An error from parsing or reasoning, with an optional source offset.
pub struct EyeronError {
    /// The human-readable error text.
    pub message: String,
    /// The byte offset into the source the error points at, if known.
    pub offset: Option<usize>,
}

impl EyeronError {
    /// Creates an error with no source offset.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            offset: None,
        }
    }

    /// Creates an error at a byte offset of the source.
    pub fn at(message: impl Into<String>, offset: usize) -> Self {
        Self {
            message: message.into(),
            offset: Some(offset),
        }
    }

    /// Renders the error with line/column and a caret under the offending character, labeled by `label` (typically the input name).
    pub fn with_source_location(&self, source: &str, label: &str) -> String {
        match self.offset {
            None => format!("{}: {}", label, self.message),
            Some(offset) => {
                let (line, col) = line_col(source, offset);
                let line_text = source.lines().nth(line.saturating_sub(1)).unwrap_or("");
                let caret = format!("{}^", " ".repeat(col.saturating_sub(1)));
                format!(
                    "{}:{}:{}: {}\n{}\n{}",
                    label, line, col, self.message, line_text, caret
                )
            }
        }
    }
}

fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

impl fmt::Display for EyeronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for EyeronError {}

impl From<std::io::Error> for EyeronError {
    fn from(value: std::io::Error) -> Self {
        Self::new(value.to_string())
    }
}

/// The crate's result alias: any failing operation yields an `EyeronError`.
pub type Result<T> = std::result::Result<T, EyeronError>;
