//! Customized typst compiler for mdbook preprocessor
//!
//! Highly inspired by the [typst-bot](https://github.com/mattfbacon/typst-bot)

use std::{
    cell::{RefCell, RefMut},
    collections::HashMap,
    io::Write,
    path::PathBuf,
};

use comemo::Prehashed;
use typst::{
    diag::{eco_format, FileError, FileResult, PackageError, PackageResult},
    eval::Tracer,
    foundations::{Bytes, Datetime},
    syntax::{package::PackageSpec, FileId, Source},
    text::{Font, FontBook},
    Library, World,
};
use typst_svg::svg;

/// Fake file
///
/// This is a fake file which wrap the real content takes from the md math block
pub struct File {
    bytes: Bytes,

    source: Option<Source>,
}

impl File {
    fn source(&mut self, id: FileId) -> FileResult<Source> {
        let source = match &self.source {
            Some(source) => source,
            None => {
                let contents =
                    std::str::from_utf8(&self.bytes).map_err(|_| FileError::InvalidUtf8)?;
                let source = Source::new(id, contents.into());
                self.source.insert(source)
            }
        };
        Ok(source.clone())
    }
}

/// Compiler
///
/// This is the compiler which has all the necessary fields except the source
pub struct Compiler {
    pub library: Prehashed<Library>,
    pub book: Prehashed<FontBook>,
    pub fonts: Vec<Font>,

    pub cache: PathBuf,
    pub files: RefCell<HashMap<FileId, File>>,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            library: Prehashed::new(Library::default()),
            book: Prehashed::new(FontBook::default()),
            fonts: Vec::new(),

            cache: PathBuf::new(),
            files: RefCell::new(HashMap::new()),
        }
    }

    pub fn wrap_source(&self, source: impl Into<String>) -> WrapSource<'_> {
        WrapSource {
            compiler: self,
            source: Source::detached(source),
            time: time::OffsetDateTime::now_local().unwrap_or(time::OffsetDateTime::now_utc()),
        }
    }

    /// Get the package directory or download if not exists
    fn package(&self, package: &PackageSpec) -> PackageResult<PathBuf> {
        let package_subdir = format!("{}/{}/{}", package.namespace, package.name, package.version);
        let path = self.cache.join(package_subdir);

        if path.exists() {
            return Ok(path);
        }

        // Download the package
        let package_url = format!(
            "https://packages.typst.org/{}/{}-{}.tar.gz",
            package.namespace, package.name, package.version
        );

        let mut response = reqwest::blocking::get(package_url).map_err(|e| {
            PackageError::NetworkFailed(Some(eco_format!(
                "Failed to download package {}: {}",
                package.name,
                e
            )))
        })?;

        let mut compressed = Vec::new();
        response.copy_to(&mut compressed).map_err(|e| {
            PackageError::NetworkFailed(Some(eco_format!(
                "Failed to save package {}: {}",
                package.name,
                e
            )))
        })?;

        let mut decompressed = Vec::new();
        let mut decoder = flate2::write::GzDecoder::new(decompressed);
        decoder.write_all(&compressed).map_err(|e| {
            PackageError::MalformedArchive(Some(eco_format!(
                "Failed to decompress package {}: {}",
                package.name,
                e
            )))
        })?;
        decoder.try_finish().map_err(|e| {
            PackageError::MalformedArchive(Some(eco_format!(
                "Failed to decompress package {}: {}",
                package.name,
                e
            )))
        })?;
        decompressed = decoder.finish().map_err(|e| {
            PackageError::MalformedArchive(Some(eco_format!(
                "Failed to decompress package {}: {}",
                package.name,
                e
            )))
        })?;

        let mut archive = tar::Archive::new(decompressed.as_slice());
        archive.unpack(&path).map_err(|e| {
            std::fs::remove_dir_all(&path).ok();
            PackageError::MalformedArchive(Some(eco_format!(
                "Failed to unpack package {}: {}",
                package.name,
                e
            )))
        })?;

        Ok(path)
    }

    fn file(&self, id: FileId) -> FileResult<RefMut<'_, File>> {
        if let Ok(file) = RefMut::filter_map(self.files.borrow_mut(), |files| files.get_mut(&id)) {
            return Ok(file);
        }

        'outer: {
            if let Some(package) = id.package() {
                let package_dir = self.package(package)?;
                let Some(path) = id.vpath().resolve(&package_dir) else {
                    break 'outer;
                };
                let contents = std::fs::read(&path).map_err(|e| FileError::from_io(e, &path))?;
                return Ok(RefMut::map(self.files.borrow_mut(), |files| {
                    files.entry(id).or_insert(File {
                        bytes: contents.into(),
                        source: None,
                    })
                }));
            }
        }

        Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
    }

    pub fn render(&self, source: impl Into<String>) -> Result<String, String> {
        let source = source.into();
        let world = self.wrap_source(source);
        let mut tracer = Tracer::default();
        let document =
            typst::compile(&world, &mut tracer).map_err(|diags| format!("{:?}", diags))?;
        // TODO: handle warnings
        // let warnings = tracer.warnings();

        let images = document
            .pages
            .iter()
            .map(|page| {
                let frame = &page.frame;
                svg(frame)
            })
            .collect::<Vec<_>>();
        let images = images.join("\n");

        Ok(images)
    }
}

/// Wrap source
///
/// This is a wrapper for the source which provides ref to the compiler
pub struct WrapSource<'a> {
    compiler: &'a Compiler,
    source: Source,
    time: time::OffsetDateTime,
}

impl World for WrapSource<'_> {
    fn library(&self) -> &Prehashed<Library> {
        &self.compiler.library
    }

    fn book(&self) -> &Prehashed<FontBook> {
        &self.compiler.book
    }

    fn main(&self) -> Source {
        self.source.clone()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            self.compiler.file(id)?.source(id)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.compiler.file(id).map(|f| f.bytes.clone())
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.compiler.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        Some(Datetime::Date(self.time.date()))
    }
}
