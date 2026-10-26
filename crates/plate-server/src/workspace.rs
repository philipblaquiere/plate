//! The workspace: a folder of `.ncl` files the editor browses, opens and saves, like a folder
//! opened in VS Code. Plate files (with a `start`) can import plans and libraries from it.
//!
//! Paths in the API are relative to the folder, with `/` separators. Only `.ncl` files are
//! listed or touched, nothing outside the folder is reachable, and hidden files and folders
//! (`.git`, …) are skipped.

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

/// Examples copied into a new, empty workspace: a simple plate, a medium one and a building that
/// uses most of the format across several files, then real-life scenarios (scenarios/) and the
/// plans they share (plans/, plans/rooms/). A test checks it lists every file in examples/.
const SEED: &[(&str, &str)] = &[
    ("1-studio.ncl", include_str!("../../../examples/1-studio.ncl")),
    ("2-apartment.ncl", include_str!("../../../examples/2-apartment.ncl")),
    ("3-hotel.ncl", include_str!("../../../examples/3-hotel.ncl")),
    ("plans/bathroom.ncl", include_str!("../../../examples/plans/bathroom.ncl")),
    ("plans/cafe.ncl", include_str!("../../../examples/plans/cafe.ncl")),
    ("plans/guest-room.ncl", include_str!("../../../examples/plans/guest-room.ncl")),
    ("plans/public.ncl", include_str!("../../../examples/plans/public.ncl")),
    ("plans/rooms/bedroom.ncl", include_str!("../../../examples/plans/rooms/bedroom.ncl")),
    ("plans/rooms/kitchen.ncl", include_str!("../../../examples/plans/rooms/kitchen.ncl")),
    ("plans/rooms/living.ncl", include_str!("../../../examples/plans/rooms/living.ncl")),
    ("plans/wing.ncl", include_str!("../../../examples/plans/wing.ncl")),
    ("scenarios/beach-villa.ncl", include_str!("../../../examples/scenarios/beach-villa.ncl")),
    ("scenarios/family-house.ncl", include_str!("../../../examples/scenarios/family-house.ncl")),
    ("scenarios/mansion.ncl", include_str!("../../../examples/scenarios/mansion.ncl")),
    ("scenarios/mansion/centre.ncl", include_str!("../../../examples/scenarios/mansion/centre.ncl")),
    ("scenarios/mansion/east.ncl", include_str!("../../../examples/scenarios/mansion/east.ncl")),
    ("scenarios/mansion/suite.ncl", include_str!("../../../examples/scenarios/mansion/suite.ncl")),
    ("scenarios/mansion/west.ncl", include_str!("../../../examples/scenarios/mansion/west.ncl")),
    ("scenarios/office-building.ncl", include_str!("../../../examples/scenarios/office-building.ncl")),
    ("scenarios/office-tower-floor.ncl", include_str!("../../../examples/scenarios/office-tower-floor.ncl")),
    ("scenarios/office/core.ncl", include_str!("../../../examples/scenarios/office/core.ncl")),
    ("scenarios/office/desks.ncl", include_str!("../../../examples/scenarios/office/desks.ncl")),
    ("scenarios/office/meeting.ncl", include_str!("../../../examples/scenarios/office/meeting.ncl")),
    ("scenarios/office/tower-core.ncl", include_str!("../../../examples/scenarios/office/tower-core.ncl")),
    ("scenarios/office/workplace.ncl", include_str!("../../../examples/scenarios/office/workplace.ncl")),
    ("scenarios/small-apartment.ncl", include_str!("../../../examples/scenarios/small-apartment.ncl")),
    ("scenarios/villa/house.ncl", include_str!("../../../examples/scenarios/villa/house.ncl")),
];

/// Most files listed (a safety net against opening a huge folder).
const MAX_FILES: usize = 2000;

#[derive(Debug)]
pub enum WsError {
    BadPath(String),
    NotFound(String),
    Exists(String),
    NotEmpty(String),
    Io(io::Error),
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsError::BadPath(p) => write!(f, "`{p}` isn't a .ncl file or a folder inside the workspace"),
            WsError::NotFound(p) => write!(f, "{p} doesn't exist"),
            WsError::Exists(p) => write!(f, "{p} already exists"),
            WsError::NotEmpty(p) => write!(f, "{p} still has files in it: move or delete them first"),
            WsError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for WsError {
    fn from(e: io::Error) -> Self {
        WsError::Io(e)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Open (creating it if needed) a workspace folder. An empty folder gets the examples.
    pub fn open(root: impl Into<PathBuf>) -> io::Result<Workspace> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let ws = Workspace { root };
        if ws.list()?.is_empty() {
            for (name, text) in SEED {
                let path = ws.root.join(name);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, text)?;
            }
        }
        Ok(ws)
    }

    /// An existing folder, as is (no seeding).
    pub fn at(root: impl Into<PathBuf>) -> Workspace {
        Workspace { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every `.ncl` file, sorted by path.
    pub fn list(&self) -> io::Result<Vec<FileEntry>> {
        let mut out = Vec::new();
        walk(&self.root, &self.root, &mut out, 0)?;
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Every folder, empty ones included, sorted by path (`plans`, `plans/rooms`, …).
    pub fn folders(&self) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        walk_dirs(&self.root, &self.root, &mut out, 0)?;
        out.sort();
        Ok(out)
    }

    /// Make a folder (and the folders above it).
    pub fn make_folder(&self, path: &str) -> Result<(), WsError> {
        let full = self.resolve_folder(path)?;
        if full.is_file() {
            return Err(WsError::Exists(path.to_owned()));
        }
        std::fs::create_dir_all(full)?;
        Ok(())
    }

    /// Remove a folder that holds no files (empty folders inside it go with it).
    pub fn remove_folder(&self, path: &str) -> Result<(), WsError> {
        let full = self.resolve_folder(path)?;
        if !full.is_dir() {
            return Err(WsError::NotFound(path.to_owned()));
        }
        fn has_files(dir: &Path) -> io::Result<bool> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    if has_files(&entry.path())? {
                        return Ok(true);
                    }
                } else {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        if has_files(&full)? {
            return Err(WsError::NotEmpty(path.to_owned()));
        }
        std::fs::remove_dir_all(full)?;
        Ok(())
    }

    /// Every file's text, for evaluating a plate with its imports.
    pub fn sources(&self) -> io::Result<BTreeMap<String, String>> {
        self.list()?
            .into_iter()
            .map(|f| Ok((f.path.clone(), std::fs::read_to_string(self.root.join(&f.path))?)))
            .collect()
    }

    pub fn read(&self, path: &str) -> Result<String, WsError> {
        let full = self.resolve(path)?;
        std::fs::read_to_string(&full).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => WsError::NotFound(path.to_owned()),
            _ => WsError::Io(e),
        })
    }

    pub fn write(&self, path: &str, text: &str) -> Result<(), WsError> {
        let full = self.resolve(path)?;
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(full, text)?;
        Ok(())
    }

    pub fn delete(&self, path: &str) -> Result<(), WsError> {
        let full = self.resolve(path)?;
        if !full.is_file() {
            return Err(WsError::NotFound(path.to_owned()));
        }
        std::fs::remove_file(full)?;
        Ok(())
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<(), WsError> {
        let (a, b) = (self.resolve(from)?, self.resolve(to)?);
        if !a.is_file() {
            return Err(WsError::NotFound(from.to_owned()));
        }
        if b.exists() {
            return Err(WsError::Exists(to.to_owned()));
        }
        if let Some(parent) = b.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(a, b)?;
        Ok(())
    }

    /// The file a workspace path names, refusing anything that could leave the folder.
    fn resolve(&self, path: &str) -> Result<PathBuf, WsError> {
        if !path.ends_with(".ncl") {
            return Err(WsError::BadPath(path.to_owned()));
        }
        self.resolve_folder(path)
    }

    /// A folder (or file) inside the workspace, by relative path: no `..`, no dot-folders.
    fn resolve_folder(&self, path: &str) -> Result<PathBuf, WsError> {
        let bad = || WsError::BadPath(path.to_owned());
        if path.is_empty() || path.len() > 300 || path.contains('\\') {
            return Err(bad());
        }
        let mut full = self.root.clone();
        for c in Path::new(path).components() {
            match c {
                Component::Normal(part) if !part.to_string_lossy().starts_with('.') => full.push(part),
                _ => return Err(bad()),
            }
        }
        Ok(full)
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<FileEntry>, depth: usize) -> io::Result<()> {
    if depth > 8 || out.len() >= MAX_FILES {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(root, &path, out, depth + 1)?;
        } else if kind.is_file() && name.ends_with(".ncl") {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            out.push(FileEntry { path: rel, size: entry.metadata().map(|m| m.len()).unwrap_or(0) });
        }
    }
    Ok(())
}

fn walk_dirs(root: &Path, dir: &Path, out: &mut Vec<String>, depth: usize) -> io::Result<()> {
    if depth > 8 || out.len() >= MAX_FILES {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)?.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" || name == "target" || !entry.file_type()?.is_dir()
        {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);
        out.push(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
        walk_dirs(root, &path, out, depth + 1)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_can_be_made_listed_and_removed_when_empty() {
        let ws = Workspace::at(temp());
        std::fs::create_dir_all(ws.root()).unwrap();
        ws.make_folder("studies/early").unwrap();
        assert_eq!(ws.folders().unwrap(), ["studies", "studies/early"]);
        ws.write("studies/a.ncl", "{}").unwrap();
        assert!(matches!(ws.remove_folder("studies"), Err(WsError::NotEmpty(_))));
        ws.remove_folder("studies/early").unwrap();
        ws.delete("studies/a.ncl").unwrap();
        ws.remove_folder("studies").unwrap();
        assert!(ws.folders().unwrap().is_empty());
        assert!(matches!(ws.make_folder("../out"), Err(WsError::BadPath(_))));
        assert!(matches!(ws.make_folder(".hidden"), Err(WsError::BadPath(_))));
        let _ = std::fs::remove_dir_all(ws.root());
    }

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("plate-ws-{}-{}", std::process::id(), rand_suffix()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn the_seed_is_every_example() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut found = Vec::new();
        walk(&examples, &examples, &mut found, 0).unwrap();
        let mut found: Vec<String> = found.into_iter().map(|f| f.path).collect();
        let mut seeded: Vec<String> = SEED.iter().map(|(p, _)| p.to_string()).collect();
        found.sort();
        seeded.sort();
        assert_eq!(found, seeded, "list every file in examples/ in SEED");
    }

    #[test]
    fn seeds_lists_reads_writes_and_renames() {
        let ws = Workspace::open(temp()).unwrap();
        let names: Vec<String> = ws.list().unwrap().into_iter().map(|f| f.path).collect();
        assert!(
            names.contains(&"3-hotel.ncl".to_owned()) && names.contains(&"plans/guest-room.ncl".to_owned())
        );
        assert_eq!(names.len(), SEED.len(), "{names:?}");

        ws.write("plans/office.ncl", "{ spaces = {} }").unwrap();
        assert_eq!(ws.read("plans/office.ncl").unwrap(), "{ spaces = {} }");
        assert!(ws.list().unwrap().iter().any(|f| f.path == "plans/office.ncl"));

        ws.rename("plans/office.ncl", "plans/studio.ncl").unwrap();
        assert!(matches!(ws.read("plans/office.ncl"), Err(WsError::NotFound(_))));
        assert!(matches!(ws.rename("3-hotel.ncl", "plans/studio.ncl"), Err(WsError::Exists(_))));
        ws.delete("plans/studio.ncl").unwrap();
        let _ = std::fs::remove_dir_all(ws.root());
    }

    #[test]
    fn refuses_paths_outside_the_folder() {
        let ws = Workspace::open(temp()).unwrap();
        for bad in ["../x.ncl", "/etc/passwd", "a/../../x.ncl", ".git/config.ncl", "x.txt", "", "a\\b.ncl"] {
            assert!(matches!(ws.read(bad), Err(WsError::BadPath(_))), "{bad}");
            assert!(matches!(ws.write(bad, ""), Err(WsError::BadPath(_))), "{bad}");
        }
        let _ = std::fs::remove_dir_all(ws.root());
    }

    #[test]
    fn the_seeded_files_check_with_their_imports() {
        // Plates, and plans on their own (they're previewed).
        let ws = Workspace::open(temp()).unwrap();
        let files = ws.sources().unwrap();
        for name in files.keys() {
            plate_config::check_files(plate_config::Workspace { main: name, files: &files })
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
        let _ = std::fs::remove_dir_all(ws.root());
    }
}
