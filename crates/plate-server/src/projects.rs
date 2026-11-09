//! Projects: the folders at the top of the workspace. Each is a [`Workspace`] of its own, closed
//! under imports: a plate only ever sees the files of its project, so editing one project can't
//! change a layout in another. There's no manifest: a folder is a project.
//!
//! The examples ship inside the binary. Opening one copies it into the workspace, so the copy is
//! the user's to change and new versions of the app can bring better examples.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::workspace::{FileEntry, Workspace, WsError, MAX_FILES};

/// Marks a workspace laid out as projects. Without it, what's there is from an older version.
const MARKER: &str = ".plate-projects";
/// Deleted projects wait here, so a delete can be undone.
const TRASH: &str = ".trash";
/// How many deleted projects are kept.
const TRASH_KEPT: usize = 20;
/// Where files from before projects are moved to.
const MIGRATED: &str = "my-files";
/// Most bytes a project zip may unpack to.
const MAX_UNZIPPED: u64 = 64 * 1024 * 1024;
/// Longest project name.
const MAX_NAME: usize = 64;

pub struct Example {
    /// Its folder, and the project's name when it's opened.
    pub name: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    /// The plate to show first.
    pub main: &'static str,
    pub files: &'static [(&'static str, &'static str)],
}

macro_rules! example_files {
    ($project:literal: $($path:literal),* $(,)?) => {
        &[$(($path, include_str!(concat!("../../../examples/", $project, "/", $path)))),*]
    };
}

/// The examples, in the order they're best read. A test checks they list every file in examples/.
pub const EXAMPLES: &[Example] = &[
    Example {
        name: "home",
        title: "Home",
        blurb: "Start here: a studio, a furnished apartment, a condo with angled walls, a villa on its plot",
        main: "studio.ncl",
        files: example_files!("home":
            "apartment.ncl", "condo.ncl", "studio.ncl", "villa.ncl",
            "plans/bathroom.ncl", "plans/bedroom.ncl", "plans/house.ncl", "plans/kitchen.ncl",
            "plans/living.ncl",
        ),
    },
    Example {
        name: "commercial",
        title: "Commercial",
        blurb: "A hotel five plans deep, an annex grown from a spine, a tower office floor",
        main: "hotel.ncl",
        files: example_files!("commercial":
            "annex.ncl", "hotel.ncl", "office.ncl",
            "hotel/bathroom.ncl", "hotel/cafe.ncl", "hotel/guest-room.ncl", "hotel/public.ncl",
            "hotel/wing.ncl",
            "office/core.ncl", "office/meeting.ncl", "office/pantry.ncl", "office/workplace.ncl",
        ),
    },
    Example {
        name: "duplex",
        title: "Duplex",
        blurb: "Built for real: wall assemblies, sized circuits, heat, plumbing, in feet and inches",
        main: "duplex.ncl",
        files: example_files!("duplex":
            "duplex.ncl", "one-bedroom.ncl", "open-plan.ncl", "plans/flat.ncl", "plans/open-flat.ncl",
            "build/assemblies/basic.ncl", "build/assemblies/wood-frame.ncl",
            "build/codes/cec.ncl", "build/codes/nec.ncl",
            "build/media/nm-b.ncl", "build/media/nmd90.ncl",
            "build/services/heat.ncl", "build/services/lighting.ncl", "build/services/plumbing.ncl",
            "build/services/power.ncl",
        ),
    },
];

impl Example {
    pub fn named(name: &str) -> Option<&'static Example> {
        EXAMPLES.iter().find(|e| e.name == name)
    }

    pub fn sources(&self) -> BTreeMap<String, String> {
        self.files.iter().map(|(p, t)| ((*p).to_owned(), (*t).to_owned())).collect()
    }
}

/// A project, as the switcher lists it.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    pub name: String,
    pub files: usize,
    pub plates: usize,
    /// The plate to show when nothing says otherwise.
    pub main: Option<String>,
    /// When a file in it last changed (milliseconds since the epoch).
    pub modified: u64,
}

#[derive(Debug, Clone)]
pub struct Projects {
    root: PathBuf,
}

impl Projects {
    /// Open (creating it if needed) a workspace folder. Files from before projects move into a
    /// project of their own, once, with their relative imports intact.
    pub fn open(root: impl Into<PathBuf>) -> io::Result<Projects> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let projects = Projects { root };
        if !projects.root.join(MARKER).exists() {
            projects.migrate()?;
            std::fs::write(projects.root.join(MARKER), "")?;
        }
        Ok(projects)
    }

    fn migrate(&self) -> io::Result<()> {
        let mut loose = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if skipped(&name) {
                continue;
            }
            let kind = entry.file_type()?;
            let holds_plates = kind.is_dir() && !Workspace::at(entry.path()).list()?.is_empty();
            if holds_plates || (kind.is_file() && name.ends_with(".ncl")) {
                loose.push(entry.path());
            }
        }
        if loose.is_empty() {
            return Ok(());
        }
        let target = self.root.join(self.free_name(MIGRATED));
        std::fs::create_dir_all(&target)?;
        for path in loose {
            let name = path.file_name().expect("a directory entry has a name");
            std::fs::rename(&path, target.join(name))?;
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every project, by name. A folder holding things but no `.ncl` file isn't one.
    pub fn list(&self) -> io::Result<Vec<ProjectInfo>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if skipped(&name) || !entry.file_type()?.is_dir() {
                continue;
            }
            let ws = Workspace::at(entry.path());
            let files = ws.list()?;
            if files.is_empty() && std::fs::read_dir(entry.path())?.next().is_some() {
                continue;
            }
            out.push(info(name, &ws, &files)?);
        }
        out.sort_by_key(|a| a.name.to_lowercase());
        Ok(out)
    }

    /// One project's folder.
    pub fn get(&self, name: &str) -> Result<Workspace, WsError> {
        check_name(name)?;
        let dir = self.root.join(name);
        if !dir.is_dir() {
            return Err(WsError::NotFound(name.to_owned()));
        }
        Ok(Workspace::at(dir))
    }

    pub fn info(&self, name: &str) -> Result<ProjectInfo, WsError> {
        let ws = self.get(name)?;
        let files = ws.list()?;
        Ok(info(name.to_owned(), &ws, &files)?)
    }

    /// A new, empty project.
    pub fn create(&self, name: &str) -> Result<(), WsError> {
        check_name(name)?;
        let dir = self.root.join(name);
        if dir.exists() {
            return Err(WsError::Exists(name.to_owned()));
        }
        std::fs::create_dir_all(dir)?;
        Ok(())
    }

    /// Copy an example into the workspace, or find the copy made before. The project's name.
    pub fn open_example(&self, example: &Example) -> Result<String, WsError> {
        if self.root.join(example.name).is_dir() {
            return Ok(example.name.to_owned());
        }
        self.create(example.name)?;
        let ws = self.get(example.name)?;
        for (path, text) in example.files {
            ws.write(path, text)?;
        }
        Ok(example.name.to_owned())
    }

    /// Copy a project, under a name that's free. The copy's name.
    pub fn duplicate(&self, name: &str, wanted: &str) -> Result<String, WsError> {
        let from = self.get(name)?;
        check_name(wanted)?;
        let copy = self.free_name(wanted);
        self.create(&copy)?;
        let to = self.get(&copy)?;
        for folder in from.folders()? {
            to.make_folder(&folder)?;
        }
        for (path, text) in from.sources()? {
            to.write(&path, &text)?;
        }
        Ok(copy)
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<(), WsError> {
        self.get(from)?;
        check_name(to)?;
        // Changing only the case of a name is a real rename on a case-insensitive disk.
        if self.root.join(to).exists() && !from.eq_ignore_ascii_case(to) {
            return Err(WsError::Exists(to.to_owned()));
        }
        std::fs::rename(self.root.join(from), self.root.join(to))?;
        Ok(())
    }

    /// Move a project to the trash. What to restore it by.
    pub fn trash(&self, name: &str) -> Result<String, WsError> {
        self.get(name)?;
        let bin = self.root.join(TRASH);
        std::fs::create_dir_all(&bin)?;
        let stamp = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let id = format!("{stamp}-{name}");
        std::fs::rename(self.root.join(name), bin.join(&id))?;
        self.prune_trash(&bin)?;
        Ok(id)
    }

    fn prune_trash(&self, bin: &Path) -> io::Result<()> {
        let mut ids: Vec<(u128, PathBuf)> = std::fs::read_dir(bin)?
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                Some((name.split('-').next()?.parse().ok()?, e.path()))
            })
            .collect();
        ids.sort();
        let extra = ids.len().saturating_sub(TRASH_KEPT);
        for (_, path) in &ids[..extra] {
            std::fs::remove_dir_all(path)?;
        }
        Ok(())
    }

    /// Bring a project back from the trash, under its name if that's still free. Its name.
    pub fn restore(&self, id: &str) -> Result<String, WsError> {
        check_name(id)?;
        let from = self.root.join(TRASH).join(id);
        if !from.is_dir() {
            return Err(WsError::NotFound(id.to_owned()));
        }
        let wanted = id.split_once('-').map_or(id, |(_, name)| name);
        let name = self.free_name(wanted);
        std::fs::rename(from, self.root.join(&name))?;
        Ok(name)
    }

    /// `wanted`, or `wanted-2`, `wanted-3`, … whichever isn't taken.
    pub fn free_name(&self, wanted: &str) -> String {
        if !self.root.join(wanted).exists() {
            return wanted.to_owned();
        }
        (2..).map(|i| format!("{wanted}-{i}")).find(|n| !self.root.join(n).exists()).expect("a free name")
    }

    /// A project as a zip of its `.ncl` files, inside a folder named after it.
    pub fn export_zip(&self, name: &str) -> Result<Vec<u8>, WsError> {
        let ws = self.get(name)?;
        let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (path, text) in ws.sources()? {
            zip.start_file(format!("{name}/{path}"), options).map_err(zip_error)?;
            zip.write_all(text.as_bytes())?;
        }
        Ok(zip.finish().map_err(zip_error)?.into_inner())
    }

    /// Make a project from a zip, under a free name. Only `.ncl` files are taken, and each goes
    /// through the workspace's path checks. The project's name.
    pub fn import_zip(&self, wanted: &str, bytes: &[u8]) -> Result<String, WsError> {
        check_name(wanted)?;
        let mut archive = zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(zip_error)?;
        let mut files: Vec<(String, String)> = Vec::new();
        let mut total = 0u64;
        for i in 0..archive.len() {
            let entry = archive.by_index(i).map_err(zip_error)?;
            let path = entry.name().map_err(zip_error)?.replace('\\', "/");
            if entry.is_dir() || !path.ends_with(".ncl") || path.starts_with("__MACOSX/") {
                continue;
            }
            total += entry.size();
            if total > MAX_UNZIPPED || files.len() >= MAX_FILES {
                return Err(WsError::BadName("that zip is too large".to_owned()));
            }
            let mut text = String::new();
            entry.take(MAX_UNZIPPED).read_to_string(&mut text)?;
            files.push((path, text));
        }
        if files.is_empty() {
            return Err(WsError::BadName("that zip has no .ncl files in it".to_owned()));
        }
        // A zip of a folder: the folder is the project, not a folder in it.
        let top = files[0].0.split('/').next().unwrap_or_default().to_owned();
        let prefix = format!("{top}/");
        let strip = top != ".." && files.iter().all(|(p, _)| p.starts_with(&prefix));
        let name = self.free_name(wanted);
        self.create(&name)?;
        let ws = self.get(&name)?;
        let written = files
            .iter()
            .try_for_each(|(path, text)| ws.write(if strip { &path[prefix.len()..] } else { path }, text));
        if let Err(e) = written {
            let _ = std::fs::remove_dir_all(self.root.join(&name));
            return Err(e);
        }
        Ok(name)
    }
}

/// The plate a project opens on: the one named after the project, else the first near the top.
pub fn main_plate(project: &str, files: &BTreeMap<String, String>) -> Option<String> {
    let named = format!("{project}.ncl");
    if files.get(&named).is_some_and(|text| is_plate(text)) {
        return Some(named);
    }
    files
        .iter()
        .filter(|(_, text)| is_plate(text))
        .min_by_key(|(path, _)| (path.matches('/').count(), (*path).clone()))
        .map(|(path, _)| path.clone())
}

/// Does this file have a `start`? The same test the editor's explorer uses.
fn is_plate(text: &str) -> bool {
    text.lines().any(|line| {
        line.trim_start().strip_prefix("start").is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

fn info(name: String, ws: &Workspace, files: &[FileEntry]) -> io::Result<ProjectInfo> {
    let sources = ws.sources()?;
    let modified = files
        .iter()
        .filter_map(|f| std::fs::metadata(ws.root().join(&f.path)).and_then(|m| m.modified()).ok())
        .filter_map(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .max()
        .unwrap_or(0);
    Ok(ProjectInfo {
        files: files.len(),
        plates: sources.values().filter(|t| is_plate(t)).count(),
        main: main_plate(&name, &sources),
        modified,
        name,
    })
}

fn skipped(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "target"
}

/// One folder name: nothing that could leave the workspace or trouble a file system.
fn check_name(name: &str) -> Result<(), WsError> {
    const RESERVED: &[&str] = &["con", "prn", "aux", "nul"];
    let bad = name.is_empty()
        || name.len() > MAX_NAME
        || name.starts_with('.')
        || name.ends_with(['.', ' '])
        || name.chars().any(|c| !(c.is_alphanumeric() || " -_.()".contains(c)))
        || skipped(name)
        || RESERVED.contains(&name.to_lowercase().as_str());
    if bad {
        return Err(WsError::BadName(format!(
            "`{name}` can't be a project name: use letters, numbers, spaces and - _ . ( )"
        )));
    }
    Ok(())
}

fn zip_error(e: zip::result::ZipError) -> WsError {
    WsError::BadName(format!("that isn't a zip this can read: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let stamp = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("plate-projects-{}-{stamp}", std::process::id()))
    }

    fn names(p: &Projects) -> Vec<String> {
        p.list().unwrap().into_iter().map(|i| i.name).collect()
    }

    #[test]
    fn the_examples_are_every_file_in_examples() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut found: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&examples).unwrap() {
            let dir = entry.unwrap();
            let name = dir.file_name().to_string_lossy().into_owned();
            found.extend(
                Workspace::at(dir.path()).list().unwrap().into_iter().map(|f| format!("{name}/{}", f.path)),
            );
        }
        let mut listed: Vec<String> =
            EXAMPLES.iter().flat_map(|e| e.files.iter().map(|(p, _)| format!("{}/{p}", e.name))).collect();
        found.sort();
        listed.sort();
        assert_eq!(found, listed, "list every file in examples/ in EXAMPLES");
        assert!(EXAMPLES.len() <= 3, "keep the examples few: three projects at most");
    }

    #[test]
    fn each_example_is_closed_under_imports_and_its_main_is_a_plate() {
        for example in EXAMPLES {
            let files = example.sources();
            assert!(is_plate(&files[example.main]), "{}", example.main);
            // Only the project's own files are given: an import reaching out of it fails.
            for name in files.keys() {
                plate_config::check_files(plate_config::Workspace { main: name, files: &files })
                    .unwrap_or_else(|e| panic!("{}/{name}: {e:?}", example.name));
            }
        }
    }

    #[test]
    fn opening_an_example_copies_it_once() {
        let p = Projects::open(temp()).unwrap();
        assert!(names(&p).is_empty(), "a new workspace starts empty");
        let home = Example::named("home").unwrap();
        assert_eq!(p.open_example(home).unwrap(), "home");
        let ws = p.get("home").unwrap();
        ws.write("studio.ncl", "{ changed = true }").unwrap();
        assert_eq!(p.open_example(home).unwrap(), "home");
        assert_eq!(ws.read("studio.ncl").unwrap(), "{ changed = true }", "the copy is the user's");
        assert_eq!(p.info("home").unwrap().files, home.files.len());
        let _ = std::fs::remove_dir_all(p.root());
    }

    #[test]
    fn files_from_before_projects_move_into_one_project_keeping_their_imports() {
        let root = temp();
        let old = Workspace::at(&root);
        std::fs::create_dir_all(&root).unwrap();
        old.write("a.ncl", "let p = import \"plans/p.ncl\" in {\n  start = 1 }").unwrap();
        old.write("plans/p.ncl", "{}").unwrap();
        std::fs::create_dir_all(root.join("not-plates")).unwrap();
        std::fs::write(root.join("not-plates/readme.txt"), "keep me").unwrap();
        std::fs::write(root.join("notes.txt"), "keep me").unwrap();

        let p = Projects::open(&root).unwrap();
        assert_eq!(names(&p), ["my-files"]);
        let moved = p.get("my-files").unwrap();
        assert!(moved.read("plans/p.ncl").is_ok());
        assert_eq!(p.info("my-files").unwrap().main.as_deref(), Some("a.ncl"));
        assert!(root.join("notes.txt").exists() && root.join("not-plates/readme.txt").exists());

        // Only once: a project made later stays where it is.
        p.create("later").unwrap();
        p.get("later").unwrap().write("b.ncl", "{}").unwrap();
        let p = Projects::open(&root).unwrap();
        assert_eq!(names(&p), ["later", "my-files"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn projects_can_be_renamed_duplicated_trashed_and_restored() {
        let p = Projects::open(temp()).unwrap();
        p.create("a").unwrap();
        let a = p.get("a").unwrap();
        a.write("x.ncl", "{}").unwrap();
        a.make_folder("empty").unwrap();
        assert!(matches!(p.create("a"), Err(WsError::Exists(_))));

        assert_eq!(p.duplicate("a", "a").unwrap(), "a-2");
        assert_eq!(p.get("a-2").unwrap().folders().unwrap(), ["empty"]);
        p.rename("a-2", "b").unwrap();
        assert_eq!(names(&p), ["a", "b"]);
        assert!(matches!(p.rename("a", "b"), Err(WsError::Exists(_))));

        let id = p.trash("a").unwrap();
        assert_eq!(names(&p), ["b"], "the trash isn't a project");
        p.create("a").unwrap();
        assert_eq!(p.restore(&id).unwrap(), "a-2", "its name was taken meanwhile");
        assert!(p.get("a-2").unwrap().read("x.ncl").is_ok());
        assert!(matches!(p.restore(&id), Err(WsError::NotFound(_))));
        let _ = std::fs::remove_dir_all(p.root());
    }

    #[test]
    fn names_that_could_leave_the_workspace_are_refused() {
        let p = Projects::open(temp()).unwrap();
        for bad in ["", "..", ".hidden", "a/b", "a\\b", "../x", "con", "trailing.", "node_modules", "a:b"] {
            assert!(matches!(p.create(bad), Err(WsError::BadName(_))), "{bad}");
            assert!(matches!(p.get(bad), Err(WsError::BadName(_))), "{bad}");
        }
        let _ = std::fs::remove_dir_all(p.root());
    }

    #[test]
    fn a_project_survives_a_trip_through_a_zip() {
        let p = Projects::open(temp()).unwrap();
        p.create("a").unwrap();
        let a = p.get("a").unwrap();
        a.write("x.ncl", "{ one = 1 }").unwrap();
        a.write("plans/y.ncl", "{ two = 2 }").unwrap();
        let bytes = p.export_zip("a").unwrap();
        assert_eq!(p.import_zip("a", &bytes).unwrap(), "a-2");
        let b = p.get("a-2").unwrap();
        assert_eq!(b.read("plans/y.ncl").unwrap(), "{ two = 2 }", "the zip's top folder is the project");
        assert!(matches!(p.import_zip("c", b"not a zip"), Err(WsError::BadName(_))));
        let _ = std::fs::remove_dir_all(p.root());
    }

    #[test]
    fn a_zip_cannot_write_outside_its_project() {
        let p = Projects::open(temp()).unwrap();
        let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        zip.start_file("../evil.ncl", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"{}").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(p.import_zip("z", &bytes).is_err());
        assert!(!p.root().parent().unwrap().join("evil.ncl").exists());
        assert!(names(&p).is_empty(), "nothing half made is left behind");
        let _ = std::fs::remove_dir_all(p.root());
    }

    #[test]
    fn the_main_plate_is_the_one_named_after_the_project_else_the_nearest_the_top() {
        let files = |list: &[(&str, &str)]| -> BTreeMap<String, String> {
            list.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
        };
        let plate = "{ start = 1 }\n{\n  start = 2 }";
        let found = files(&[("a/deep.ncl", plate), ("z.ncl", plate), ("b.ncl", "{}")]);
        assert_eq!(main_plate("p", &found).as_deref(), Some("z.ncl"));
        let found = files(&[("a.ncl", plate), ("p.ncl", plate)]);
        assert_eq!(main_plate("p", &found).as_deref(), Some("p.ncl"));
        assert_eq!(main_plate("p", &files(&[("a.ncl", "{}")])), None);
    }
}
