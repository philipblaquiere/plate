//! plate-export: writes a [`Layout`] to formats CAD tools open directly.
//!
//! - **DXF** (AutoCAD 2000, ASCII): opens in AutoCAD, Revit, ArchiCAD, Rhino, Vectorworks,
//!   SketchUp, LibreCAD, QCAD. Units are set via `$INSUNITS`, one layer per element kind.
//! - **SVG**: scaled for print (1:100 metric, 1/8" = 1'-0" imperial), y axis up like the drawing.
//! - **JSON**: the raw layout, for scripts and tests.
//!
//! IFC (for BIM round-trips) is the natural next format; it slots in as another function here.

use plate_core::Layout;

mod dxf_writer;
mod svg_writer;

pub use dxf_writer::to_dxf;
pub use svg_writer::to_svg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Dxf,
    Svg,
    Json,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        match s.to_ascii_lowercase().as_str() {
            "dxf" => Some(Format::Dxf),
            "svg" => Some(Format::Svg),
            "json" => Some(Format::Json),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Dxf => "dxf",
            Format::Svg => "svg",
            Format::Json => "json",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Format::Dxf => "image/vnd.dxf",
            Format::Svg => "image/svg+xml",
            Format::Json => "application/json",
        }
    }
}

#[derive(Debug)]
pub struct ExportError(pub String);

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExportError {}

pub fn export(layout: &Layout, format: Format) -> Result<Vec<u8>, ExportError> {
    match format {
        Format::Dxf => to_dxf(layout),
        Format::Svg => Ok(to_svg(layout).into_bytes()),
        Format::Json => serde_json::to_vec_pretty(layout).map_err(|e| ExportError(e.to_string())),
    }
}

/// A file-system friendly name derived from the layout name.
pub fn file_stem(layout: &Layout) -> String {
    let stem: String =
        layout.name.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let stem = stem.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    if stem.is_empty() {
        "plate".into()
    } else {
        stem
    }
}
