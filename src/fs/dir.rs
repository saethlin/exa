use std::io::{self, Result as IOResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::borrow::Cow;

use fs::File;
use fs::feature::ignore::IgnoreCache;


/// A **Dir** provides a cached list of the file paths in a directory that's
/// being listed.
///
/// This object gets passed to the Files themselves, in order for them to
/// check the existence of surrounding files, then highlight themselves
/// accordingly. (See `File#get_source_files`)
pub struct Dir {

    /// A vector of the files that have been read from this directory.
    contents: Vec<PathBuf>,

    /// The path that was read.
    pub path: PathBuf,
}

impl Dir {

    /// Create a new Dir object filled with all the files in the directory
    /// pointed to by the given path. Fails if the directory can’t be read, or
    /// isn’t actually a directory, or if there’s an IO error that occurs at
    /// any point.
    ///
    /// The `read_dir` iterator doesn’t actually yield the `.` and `..`
    /// entries, so if the user wants to see them, we’ll have to add them
    /// ourselves after the files have been read.
    pub fn read_dir(path: PathBuf) -> IOResult<Dir> {
        info!("Reading directory {:?}", &path);

        let contents = fs::read_dir(&path)?
                                             .map(|result| result.map(|entry| entry.path()))
                                             .collect::<Result<_,_>>()?;

        Ok(Dir { contents, path })
    }

    /// Produce an iterator of IO results of trying to read all the files in
    /// this directory.
    pub fn files<'dir, 'ig>(
        &'dir self,
        dot_filter: DotFilter,
        ignore: Option<&'ig IgnoreCache>,
        output: &mut Vec<File<'dir>>
    ) -> Vec<Arc<io::Error>>  {
        use rayon::prelude::*;
        if let Some(i) = ignore { i.discover_underneath(&self.path); }

        // File::new calls std::fs::File::metadata, which on linux calls lstat. On some
        // filesystems this can be very slow, but there's no async filesystem API
        // so all we can do to hide the latency is use system threads.

        let mut paths = Vec::with_capacity(self.contents.len());
        for file_to_scan in self.contents.iter().filter_map(|path| {
            let dotfiles = dot_filter.shows_dotfiles();
                let name =  File::filename(path);
                if !dotfiles && name.starts_with('.') {
                    None
                } else if ignore.map(|i| i.is_ignored(path)).unwrap_or(false) {
                    None
                } else {
                    Some(path)
                }
            }) {
            paths.push(file_to_scan);
        }

        output.reserve(paths.len() + 2);

        paths.par_iter().map(|path| {
            File::new(Cow::Borrowed(*path), self, None).unwrap()
        }).collect_into_vec(output);

        let mut dots = dot_filter.dots();
        // Append special directories
        if let Dots::DotNext = dots {
            dots = Dots::DotDotNext;
            File::new(Cow::Borrowed(&self.path), self, Some(".")).map(|f| output.push(f)).unwrap();
        }
        if let Dots::DotDotNext = dots {
            File::new(Cow::Owned(self.path.join("..")), self, Some("..")).map(|f| output.push(f)).unwrap();
        }

        Vec::new() // TODO: Actually report errors here somehow

    }

    /// Whether this directory contains a file with the given path.
    pub fn contains(&self, path: &Path) -> bool {
        self.contents.iter().any(|p| p.as_path() == path)
    }

    /// Append a path onto the path specified by this directory.
    pub fn join(&self, child: &Path) -> PathBuf {
        self.path.join(child)
    }
}

/*
/// Iterator over reading the contents of a directory as `File` objects.
pub struct Files<'dir, 'ig> {

    /// The internal iterator over the paths that have been read already.
    inner: std::iter::Zip<std::slice::Iter<'dir, PathBuf>, std::vec::IntoIter<
        (Result<fs::Metadata, Arc<io::Error>>, Option<Result<fs::Metadata, Arc<io::Error>>>)>>,

    /// The directory that begat those paths.
    dir: &'dir Dir,

    /// Whether to include dotfiles in the list.
    dotfiles: bool,

    /// Whether the `.` or `..` directories should be produced first, before
    /// any files have been listed.
    dots: Dots,

    ignore: Option<&'ig IgnoreCache>,
}

impl<'dir, 'ig> Files<'dir, 'ig> {
    fn parent(&self) -> PathBuf {
        // We can’t use `Path#parent` here because all it does is remove the
        // last path component, which is no good for us if the path is
        // relative. For example, while the parent of `/testcases/files` is
        // `/testcases`, the parent of `.` is an empty path. Adding `..` on
        // the end is the only way to get to the *actual* parent directory.
        self.dir.path.join("..")
    }

    /// Go through the directory until we encounter a file we can list (which
    /// varies depending on the dotfile visibility flag)
    fn next_visible_file(&mut self) -> Option<Result<File<'dir>, (PathBuf, io::Error)>> {
        loop {
            if let Some((path, (metadata, target_metadata))) = self.inner.next() {
                let filename = File::filename(&path);
                if !self.dotfiles && filename.starts_with('.') { continue }

                if let Some(i) = self.ignore {
                    if i.is_ignored(&path) { continue }
                }

                let target_metadata = target_metadata.map(|m| m.map_err(|e| Arc::try_unwrap(e).unwrap()));

                return Some(metadata.map(|meta|
                    File {
                        name: filename,
                        ext: File::ext(path),
                        path: path.into(),
                        metadata: meta,
                        parent_dir: Some(self.dir),
                        target_metadata,
                    }
                ).map_err(|e| {
                    (path.clone(), Arc::try_unwrap(e).unwrap())
                }))
            }
            else {
                return None
            }
        }
    }
}
*/

/// The dot directories that need to be listed before actual files, if any.
/// If these aren’t being printed, then `FilesNext` is used to skip them.
enum Dots {

    /// List the `.` directory next.
    DotNext,

    /// List the `..` directory next.
    DotDotNext,

    /// Forget about the dot directories and just list files.
    FilesNext,
}

/*
impl<'dir, 'ig> Iterator for Files<'dir, 'ig> {
    type Item = Result<File<'dir>, (PathBuf, io::Error)>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Dots::DotNext = self.dots {
            self.dots = Dots::DotDotNext;
            Some(File::new(Cow::Borrowed(&self.dir.path), self.dir, Some(String::from(".")))
                      .map_err(|e| (Path::new(".").to_path_buf(), e)))
        }
        else if let Dots::DotDotNext = self.dots {
            self.dots = Dots::FilesNext;
            Some(File::new(self.parent().into(), self.dir, Some(String::from("..")))
                      .map_err(|e| (self.parent(), e)))
        }
        else {
            self.next_visible_file()
        }
    }
}
*/

/// Usually files in Unix use a leading dot to be hidden or visible, but two
/// entries in particular are "extra-hidden": `.` and `..`, which only become
/// visible after an extra `-a` option.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum DotFilter {

    /// Shows files, dotfiles, and `.` and `..`.
    DotfilesAndDots,

    /// Show files and dotfiles, but hide `.` and `..`.
    Dotfiles,

    /// Just show files, hiding anything beginning with a dot.
    JustFiles,
}

impl Default for DotFilter {
    fn default() -> DotFilter {
        DotFilter::JustFiles
    }
}

impl DotFilter {

    /// Whether this filter should show dotfiles in a listing.
    fn shows_dotfiles(self) -> bool {
        match self {
            DotFilter::JustFiles       => false,
            DotFilter::Dotfiles        => true,
            DotFilter::DotfilesAndDots => true,
        }
    }

    /// Whether this filter should add dot directories to a listing.
    fn dots(self) -> Dots {
        match self {
            DotFilter::JustFiles       => Dots::FilesNext,
            DotFilter::Dotfiles        => Dots::FilesNext,
            DotFilter::DotfilesAndDots => Dots::DotNext,
        }
    }
}
