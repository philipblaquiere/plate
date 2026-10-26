//! Where each value of the evaluated configuration comes from in the user's file.
//!
//! Nickel keeps the source position of every value through evaluation, including values that
//! travel through `let` bindings and merges. Walking the evaluated value gives a map from each
//! JSON pointer (`/start/points/2/0`) to the text that produced it. The editor uses it to put
//! squiggles on values that fail a geometric check, and to write edits made in the drawing
//! (dragging a point) back into the exact numbers in the file.

use nickel_lang_core::{
    eval::value::{NickelValue, ValueContentRef},
    files::FileId,
    position::{PosTable, TermPos},
};
use plate_core::Span;
use serde::Serialize;

/// One value's origin in the user's source.
#[derive(Debug, Clone, Serialize)]
pub struct SourceSpan {
    /// JSON pointer into the evaluated configuration, e.g. `/start/points/2/0`.
    pub pointer: String,
    /// The file the value is written in, relative to the workspace.
    pub file: String,
    pub span: Span,
    /// `number`, `string`, `array`, `record` or `other`.
    pub kind: &'static str,
    /// True when the span is the literal itself (e.g. `30`), false when the value was computed
    /// from an expression (e.g. `side * 2`). Only literals can be rewritten from the drawing.
    pub literal: bool,
}

pub(crate) struct Walker<'a> {
    pub table: &'a PosTable,
    /// Maps a byte range in a Nickel file to (index into `files`, start, end) in the user's
    /// source; `None` when it isn't the user's text.
    pub map: &'a dyn Fn(FileId, usize, usize) -> Option<(usize, usize, usize)>,
    /// The user's files: (path, source).
    pub files: &'a [(&'a str, &'a str)],
    pub out: Vec<SourceSpan>,
}

impl Walker<'_> {
    pub fn walk(&mut self, value: &NickelValue, pointer: &str, depth: usize) {
        if depth > 64 {
            return;
        }
        let kind = match value.content_ref() {
            ValueContentRef::Record(record) => {
                if let Some(record) = record.into_opt() {
                    for (id, field) in &record.fields {
                        if let Some(child) = &field.value {
                            let key = id.label().replace('~', "~0").replace('/', "~1");
                            self.walk(child, &format!("{pointer}/{key}"), depth + 1);
                        }
                    }
                }
                "record"
            }
            ValueContentRef::Array(array) => {
                if let Some(array) = array.into_opt() {
                    for (i, child) in array.array.iter().enumerate() {
                        self.walk(child, &format!("{pointer}/{i}"), depth + 1);
                    }
                }
                "array"
            }
            ValueContentRef::Thunk(thunk) => {
                let inner = thunk.borrow().value.clone();
                return self.walk(&inner, pointer, depth + 1);
            }
            ValueContentRef::Number(_) => "number",
            ValueContentRef::String(_) => "string",
            _ => "other",
        };
        self.record(value, pointer, kind);
    }

    fn record(&mut self, value: &NickelValue, pointer: &str, kind: &'static str) {
        let raw = match value.pos(self.table) {
            TermPos::Original(span) | TermPos::Inherited(span) => span,
            TermPos::None => return,
        };
        let Some((index, start, end)) = (self.map)(raw.src_id, raw.start.0 as usize, raw.end.0 as usize)
        else {
            return;
        };
        let (file, source) = self.files[index];
        // Literal-ness is judged from the text: `30` or `"98'-6"` can be rewritten in place,
        // `side * 2` can't.
        let snippet = source[start..end].trim();
        let literal = match kind {
            "number" => snippet.parse::<f64>().is_ok(),
            "string" => {
                snippet.len() >= 2
                    && snippet.starts_with('"')
                    && snippet.ends_with('"')
                    && !snippet[1..snippet.len() - 1].contains(['"', '%'])
            }
            "array" => snippet.starts_with('[') && snippet.ends_with(']'),
            "record" => snippet.starts_with('{') && snippet.ends_with('}'),
            _ => false,
        };
        self.out.push(SourceSpan {
            pointer: pointer.to_owned(),
            file: file.to_owned(),
            span: Span::from_offsets(source, start, end),
            kind,
            literal,
        });
    }
}

/// The best span for a pointer: its own, or the closest ancestor's.
pub fn lookup<'a>(sources: &'a [SourceSpan], pointer: &str) -> Option<&'a SourceSpan> {
    let mut p = pointer;
    loop {
        if let Some(s) = sources.iter().find(|s| s.pointer == p) {
            return Some(s);
        }
        let cut = p.rfind('/')?;
        p = &p[..cut];
    }
}
