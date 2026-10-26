use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A source range, 1-based lines and columns (Monaco's convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl Span {
    /// Build a span from byte offsets into `source`.
    pub fn from_offsets(source: &str, start: usize, end: usize) -> Self {
        let (start_line, start_col) = line_col(source, start);
        let (end_line, end_col) = line_col(source, end.max(start));
        Span { start_line, start_col, end_line, end_col }
    }
}

fn line_col(source: &str, offset: usize) -> (u32, u32) {
    let offset = offset.min(source.len());
    let before = &source[..floor_char_boundary(source, offset)];
    let line = before.matches('\n').count() as u32 + 1;
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = before[line_start..].chars().count() as u32 + 1;
    (line, col)
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Anything the user should see next to their configuration.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// The value the diagnostic is about, as a JSON pointer into the configuration (e.g.
    /// `/rules/2`). The editor maps it to a span with the source map.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    /// The file `span` is in, relative to the workspace (the plate file when absent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// What sort of solver finding it is. Notes hold what to change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<Code>,
}

/// What sort of solver finding a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Code {
    /// A requirement the layout breaks.
    Unmet,
    /// A preference it misses.
    Preference,
    /// Floor no space covers.
    Leftover,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            notes: vec![],
            span: None,
            pointer: None,
            file: None,
            code: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
            notes: vec![],
            span: None,
            pointer: None,
            file: None,
            code: None,
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Diagnostic { severity: Severity::Info, ..Diagnostic::warning(message) }
    }

    pub fn with_span(mut self, span: Option<Span>) -> Self {
        self.span = span;
        self
    }

    pub fn in_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn with_pointer(mut self, pointer: impl Into<String>) -> Self {
        self.pointer = Some(pointer.into());
        self
    }

    pub fn with_code(mut self, code: Code) -> Self {
        self.code = Some(code);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_map_to_one_based_line_col() {
        let src = "ab\ncdé\nf";
        let span = Span::from_offsets(src, 3, 5);
        assert_eq!((span.start_line, span.start_col), (2, 1));
        assert_eq!((span.end_line, span.end_col), (2, 3));
    }
}
