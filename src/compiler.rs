//! Customized typst compiler for mdbook preprocessor
//!
//! Highly inspired by the [typst-bot](https://github.com/mattfbacon/typst-bot)

use std::{collections::HashMap, io::Write, path::PathBuf, sync::RwLock};

use typst::{
    diag::{eco_format, FileError, FileResult, PackageError, PackageResult},
    foundations::{Bytes, Datetime},
    layout::PagedDocument,
    syntax::{package::PackageSpec, FileId, Source},
    text::{Font, FontBook},
    utils::LazyHash,
    Library, LibraryExt, World,
};
use typst_svg::svg;

/// Cached file with bytes and optional parsed source
struct CachedFile {
    bytes: Bytes,
    source: Option<Source>,
}

/// Compiler
///
/// This is the compiler which has all the necessary fields except the source
pub struct Compiler {
    pub library: LazyHash<Library>,
    pub book: LazyHash<FontBook>,
    pub fonts: Vec<Font>,

    pub cache: PathBuf,
    files: RwLock<HashMap<FileId, CachedFile>>,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(FontBook::default()),
            fonts: Vec::new(),

            cache: PathBuf::new(),
            files: RwLock::new(HashMap::new()),
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

        let decompressed = Vec::new();
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
        let decompressed = decoder.finish().map_err(|e| {
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

    fn get_file(&self, id: FileId) -> FileResult<Bytes> {
        // Check if file is already cached
        {
            let files = self.files.read().unwrap();
            if let Some(file) = files.get(&id) {
                return Ok(file.bytes.clone());
            }
        }

        // File not cached, try to load it
        if let Some(package) = id.package() {
            let package_dir = self.package(package)?;
            let Some(path) = id.vpath().resolve(&package_dir) else {
                return Err(FileError::NotFound(id.vpath().as_rootless_path().into()));
            };
            let contents = std::fs::read(&path).map_err(|e| FileError::from_io(e, &path))?;
            let bytes = Bytes::new(contents);

            let mut files = self.files.write().unwrap();
            files.insert(
                id,
                CachedFile {
                    bytes: bytes.clone(),
                    source: None,
                },
            );
            return Ok(bytes);
        }

        Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
    }

    fn get_source(&self, id: FileId) -> FileResult<Source> {
        // Check if source is already cached
        {
            let files = self.files.read().unwrap();
            if let Some(file) = files.get(&id) {
                if let Some(source) = &file.source {
                    return Ok(source.clone());
                }
            }
        }

        // Get the bytes first
        let bytes = self.get_file(id)?;

        // Parse the source
        let contents = std::str::from_utf8(bytes.as_slice()).map_err(|_| FileError::InvalidUtf8)?;
        let source = Source::new(id, contents.into());

        // Cache the source
        {
            let mut files = self.files.write().unwrap();
            if let Some(file) = files.get_mut(&id) {
                file.source = Some(source.clone());
            }
        }

        Ok(source)
    }

    pub fn render(&self, source: impl Into<String>) -> Result<String, String> {
        let source = source.into();
        let world = self.wrap_source(source);
        let result = typst::compile::<PagedDocument>(&world);

        // TODO: handle warnings from result.warnings

        let document = result.output.map_err(|diags| format!("{:?}", diags))?;

        let images = document.pages.iter().map(svg).collect::<Vec<_>>();
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
    fn library(&self) -> &LazyHash<Library> {
        &self.compiler.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.compiler.book
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            self.compiler.get_source(id)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.compiler.get_file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.compiler.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        Some(Datetime::Date(self.time.date()))
    }
}
