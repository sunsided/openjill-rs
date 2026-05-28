use std::path::{Path, PathBuf};

use egui::{Response, Sense, Ui, Vec2};

/// Default file extensions displayed by [`FileTree`] when none are configured.
///
/// These map to the Jill of the Jungle data file formats supported by the
/// `openjill-data` crate.  All values are lowercase; the comparison is always
/// case-insensitive.
pub const DEFAULT_EXTENSIONS: &[&str] = &["sha", "jn1", "dma", "vcl", "cfg"];

// ── tree model ────────────────────────────────────────────────────────────────

/// A single node in the pre-built directory tree managed by [`FileTreeState`].
enum FileTreeEntry {
    /// A directory node rendered as a collapsible header.
    Dir {
        /// Path to this directory.
        path: PathBuf,
        /// Display name (last path component).
        name: String,
        /// Children of this directory, directories before files, both sorted
        /// lexicographically by name.
        children: Vec<FileTreeEntry>,
    },
    /// A file whose extension matched the active filter.
    File {
        /// Path to this file.
        path: PathBuf,
        /// Display name (filename only).
        name: String,
    },
}

// ── state ─────────────────────────────────────────────────────────────────────

/// Persistent state for the [`FileTree`] widget.
///
/// Holds the pre-scanned directory tree and the active extension filter.
/// Create once per app or panel and pass a mutable reference to each
/// [`FileTree`] call.  Call [`FileTreeState::refresh`] to re-scan when the
/// filesystem changes.
pub struct FileTreeState {
    /// Root directory being listed.
    root: PathBuf,
    /// Lowercase extensions used to filter leaf files (no leading dot).
    extensions: Vec<String>,
    /// Pre-built tree rooted at [`FileTreeState::root`].
    tree: Vec<FileTreeEntry>,
}

impl FileTreeState {
    /// Creates a new state rooted at `root` with the [`DEFAULT_EXTENSIONS`].
    ///
    /// The root path is canonicalized so that all stored paths are absolute.
    /// If canonicalization fails (e.g. the directory does not exist yet), the
    /// original path is kept as-is.
    ///
    /// The directory tree is scanned eagerly at construction.  Call
    /// [`FileTreeState::refresh`] later to pick up filesystem changes.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let root = root.canonicalize().unwrap_or(root);
        let mut state = Self {
            root,
            extensions: DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
            tree: Vec::new(),
        };
        state.refresh();
        state
    }

    /// Replaces the extension filter with `extensions` and re-scans.
    ///
    /// Each extension may be passed with or without a leading dot and is
    /// normalised to lowercase, so `"SHA"`, `".sha"`, and `"sha"` all accept
    /// files whose extension is `sha`.
    #[must_use]
    pub fn with_extensions(mut self, extensions: &[&str]) -> Self {
        self.extensions = extensions
            .iter()
            .map(|s| s.trim_start_matches('.').to_lowercase())
            .collect();
        self.refresh();
        self
    }

    /// Re-scans the root directory and rebuilds the in-memory tree.
    ///
    /// Directories that contain no matching descendants are pruned so the
    /// rendered tree stays compact.
    pub fn refresh(&mut self) {
        self.tree = scan_dir(&self.root, &self.extensions);
    }

    /// Returns the root path this state was created with.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

// ── filesystem scan ───────────────────────────────────────────────────────────

/// Scans `dir` recursively and returns entries whose extensions match `extensions`.
///
/// Within each directory, sub-directories come before files; both groups are
/// sorted lexicographically by display name.  Directories with no matching
/// descendants are omitted.
fn scan_dir(dir: &Path, extensions: &[String]) -> Vec<FileTreeEntry> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut dirs: Vec<FileTreeEntry> = Vec::new();
    let mut files: Vec<FileTreeEntry> = Vec::new();

    for entry in read_dir.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            let children = scan_dir(&path, extensions);
            if !children.is_empty() {
                dirs.push(FileTreeEntry::Dir { path, name, children });
            }
        } else if path.is_file() && has_matching_extension(&path, extensions) {
            files.push(FileTreeEntry::File { path, name });
        }
    }

    dirs.sort_by(|a, b| entry_display_name(a).cmp(entry_display_name(b)));
    files.sort_by(|a, b| entry_display_name(a).cmp(entry_display_name(b)));
    dirs.extend(files);
    dirs
}

/// Returns `true` when `path`'s extension equals any element of `extensions`.
///
/// The comparison is case-insensitive; `extensions` must already be stored in
/// lowercase (which [`FileTreeState`] guarantees).
fn has_matching_extension(path: &Path, extensions: &[String]) -> bool {
    let ext: String = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    extensions.iter().any(|allowed| allowed == &ext)
}

/// Returns the display name stored in a [`FileTreeEntry`].
fn entry_display_name(entry: &FileTreeEntry) -> &str {
    match entry {
        FileTreeEntry::Dir { name, .. } | FileTreeEntry::File { name, .. } => name.as_str(),
    }
}

// ── widget ────────────────────────────────────────────────────────────────────

/// Output from [`FileTree::show`].
pub struct FileTreeOutput {
    /// Combined egui response from all interactions in the tree.
    pub response: Response,
    /// Path of the file that was clicked this frame, if any.
    pub clicked_path: Option<PathBuf>,
}

/// Egui widget that lists files under a root directory with extension filters.
///
/// Presents a collapsible directory tree where files matching the configured
/// extension list appear as selectable leaf labels.  Clicking a file updates
/// the `selected` path and emits it through [`FileTreeOutput::clicked_path`].
///
/// # Example
///
/// ```ignore
/// let mut state = FileTreeState::new("/path/to/data");
/// let mut selected: Option<PathBuf> = None;
///
/// // inside an egui frame:
/// egui::ScrollArea::vertical().show(ui, |ui| {
///     let output = FileTree::new(&mut state, &mut selected).show(ui);
///     if let Some(path) = output.clicked_path {
///         println!("selected: {}", path.display());
///     }
/// });
/// ```
pub struct FileTree<'a> {
    /// Pre-scanned file tree state.
    state: &'a mut FileTreeState,
    /// Currently selected absolute path; updated on click.
    selected: &'a mut Option<PathBuf>,
}

impl<'a> FileTree<'a> {
    /// Creates a file tree widget backed by `state` with mutable selection.
    pub fn new(state: &'a mut FileTreeState, selected: &'a mut Option<PathBuf>) -> Self {
        Self { state, selected }
    }

    /// Renders the tree into `ui` and returns interaction output.
    pub fn show(self, ui: &mut Ui) -> FileTreeOutput {
        let mut clicked_path = None;

        if self.state.tree.is_empty() {
            let response = ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "No matching files found under {}",
                    self.state.root.display()
                ),
            );
            return FileTreeOutput { response, clicked_path };
        }

        let mut response = ui.allocate_response(Vec2::ZERO, Sense::hover());
        for entry in &self.state.tree {
            let out = show_entry(ui, entry, self.selected);
            if clicked_path.is_none() {
                clicked_path = out.clicked_path;
            }
            response = response.union(out.response);
        }

        FileTreeOutput { response, clicked_path }
    }
}

impl egui::Widget for FileTree<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

// ── entry rendering ───────────────────────────────────────────────────────────

/// Internal result from rendering one [`FileTreeEntry`].
struct EntryOutput {
    /// Egui response for this entry (and all its children, when a directory).
    response: Response,
    /// Path of the file clicked inside this entry, if any.
    clicked_path: Option<PathBuf>,
}

/// Renders a single [`FileTreeEntry`] into `ui`, recursing for directories.
fn show_entry(ui: &mut Ui, entry: &FileTreeEntry, selected: &mut Option<PathBuf>) -> EntryOutput {
    match entry {
        FileTreeEntry::Dir { path, name, children } => {
            let cr = egui::CollapsingHeader::new(name.as_str())
                .id_salt(path.as_os_str())
                .default_open(false)
                .show(ui, |ui| {
                    let mut child_clicked: Option<PathBuf> = None;
                    let mut child_resp = ui.allocate_response(Vec2::ZERO, Sense::hover());
                    for child in children {
                        let out = show_entry(ui, child, selected);
                        if child_clicked.is_none() {
                            child_clicked = out.clicked_path;
                        }
                        child_resp = child_resp.union(out.response);
                    }
                    (child_clicked, child_resp)
                });
            // `body_returned` carries the closure's return value; `body_response`
            // is egui's own response for the body area.
            let (clicked_path, opt_child_resp) = match cr.body_returned {
                Some((clicked, resp)) => (clicked, Some(resp)),
                None => (None, None),
            };
            let mut response = cr.header_response;
            if let Some(body_resp) = cr.body_response {
                response = response.union(body_resp);
            }
            if let Some(child_resp) = opt_child_resp {
                response = response.union(child_resp);
            }
            EntryOutput { response, clicked_path }
        }
        FileTreeEntry::File { path, name } => {
            let is_selected = selected.as_deref() == Some(path.as_path());
            let mut response = ui.add(egui::Button::selectable(is_selected, name.as_str()));
            let clicked_path = if response.clicked() {
                if selected.as_deref() != Some(path.as_path()) {
                    *selected = Some(path.clone());
                    response.mark_changed();
                }
                Some(path.clone())
            } else {
                None
            };
            EntryOutput { response, clicked_path }
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{DEFAULT_EXTENSIONS, has_matching_extension, scan_dir};
    use std::path::Path;

    /// Verifies that `has_matching_extension` accepts a file whose extension
    /// exactly matches a lowercase entry in the allow-list.
    #[test]
    fn has_matching_extension_accepts_exact_lowercase_match() {
        let extensions: Vec<String> = DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect();
        assert!(has_matching_extension(Path::new("JILL1.sha"), &extensions));
    }

    /// Verifies that `has_matching_extension` accepts uppercase file extensions
    /// because the comparison lowercases the path extension before comparing.
    #[test]
    fn has_matching_extension_accepts_uppercase_extension() {
        let extensions: Vec<String> = DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect();
        assert!(has_matching_extension(Path::new("JILL1.SHA"), &extensions));
        assert!(has_matching_extension(Path::new("jill1.DMA"), &extensions));
        assert!(has_matching_extension(Path::new("jill1.JN1"), &extensions));
        assert!(has_matching_extension(Path::new("jill1.VCL"), &extensions));
        assert!(has_matching_extension(Path::new("jill1.CFG"), &extensions));
    }

    /// Verifies that `has_matching_extension` rejects files whose extension is
    /// not in the allow-list.
    #[test]
    fn has_matching_extension_rejects_unknown_extension() {
        let extensions: Vec<String> = DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect();
        assert!(!has_matching_extension(Path::new("readme.txt"), &extensions));
        assert!(!has_matching_extension(Path::new("image.png"), &extensions));
        assert!(!has_matching_extension(Path::new("noext"), &extensions));
    }

    /// Verifies that `scan_dir` returns an empty list when the target
    /// directory does not exist.
    #[test]
    fn scan_dir_returns_empty_for_nonexistent_directory() {
        let extensions: Vec<String> = DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect();
        let result = scan_dir(Path::new("/nonexistent/path/that/should/not/exist"), &extensions);
        assert!(result.is_empty());
    }

    /// Verifies that `scan_dir` returns only matching files and prunes
    /// directories that contain no matching descendants.
    #[test]
    fn scan_dir_finds_matching_files_and_prunes_empty_dirs() {
        use std::fs;

        // Use PID + nanosecond timestamp to avoid collisions in parallel test runs.
        let unique = format!(
            "openjill_file_tree_test_scan_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        );
        let base = std::env::temp_dir().join(unique);
        // Clean up any leftover state from a previous run.
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("failed to create temp dir");

        fs::write(base.join("JILL1.SHA"), b"").expect("write SHA");
        fs::write(base.join("jill1.dma"), b"").expect("write dma");
        fs::write(base.join("readme.txt"), b"").expect("write txt");

        // A sub-directory that has no matching files — should be pruned.
        let empty_sub = base.join("nosub");
        fs::create_dir_all(&empty_sub).expect("create empty sub");
        fs::write(empty_sub.join("notes.txt"), b"").expect("write notes");

        let extensions: Vec<String> = DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect();
        let entries = scan_dir(&base, &extensions);

        // Only the two Jill files should appear; txt files and the empty dir
        // are filtered out.
        assert_eq!(
            entries.len(),
            2,
            "expected exactly 2 matching files, got {}",
            entries.len()
        );

        let _ = fs::remove_dir_all(&base);
    }
}
