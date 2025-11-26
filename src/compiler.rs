//! Customized Typst compiler for mdbook preprocessor.
//!
//! This module provides a [`Compiler`] that wraps Typst's compilation functionality,
//! handling font loading, package management, and source compilation.
//!
//! Highly inspired by the [typst-bot](https://github.com/mattfbacon/typst-bot).

use std::{collections::HashMap, fmt, io::Write, path::PathBuf, sync::RwLock};

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

/// Errors that can occur during Typst compilation.
#[derive(Debug)]
pub enum CompileError {
    /// Typst compilation failed with diagnostics.
    ///
    /// Contains a formatted string of the compilation errors.
    Compilation(String),
    /// Internal lock was poisoned.
    ///
    /// This should not happen in normal operation and indicates a panic
    /// occurred while holding a lock.
    #[allow(dead_code)]
    LockPoisoned,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::Compilation(msg) => write!(f, "Typst compilation error: {}", msg),
            CompileError::LockPoisoned => write!(f, "Internal error: lock poisoned"),
        }
    }
}

impl std::error::Error for CompileError {}

/// Cached file with bytes and optional parsed source.
struct CachedFile {
    bytes: Bytes,
    source: Option<Source>,
}

/// The Typst compiler context.
///
/// This struct holds all the state needed to compile Typst documents:
/// - Standard library and font book
/// - Loaded fonts
/// - File cache for packages and sources
///
/// # Example
///
/// ```ignore
/// let mut compiler = Compiler::new();
/// // Configure fonts and cache as needed
/// let svg = compiler.render("$ x^2 + y^2 = z^2 $")?;
/// ```
pub struct Compiler {
    /// The Typst standard library.
    pub library: LazyHash<Library>,
    /// Font metadata book for font selection.
    pub book: LazyHash<FontBook>,
    /// Loaded font data.
    pub fonts: Vec<Font>,
    /// Cache directory for downloaded packages.
    pub cache: PathBuf,
    /// Internal file cache for sources and binary files.
    files: RwLock<HashMap<FileId, CachedFile>>,
}

impl Default for Compiler {
    fn default() -> Self {
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(FontBook::default()),
            fonts: Vec::new(),
            cache: PathBuf::new(),
            files: RwLock::new(HashMap::new()),
        }
    }
}

impl Compiler {
    /// Creates a new compiler with default settings.
    ///
    /// The compiler starts with an empty font book and no loaded fonts.
    /// You should add fonts before rendering.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps a source string into a [`WrapSource`] that implements [`World`].
    ///
    /// This creates a complete Typst world context for compilation,
    /// capturing the current local time for date-related functions.
    pub fn wrap_source(&self, source: impl Into<String>) -> WrapSource<'_> {
        WrapSource {
            compiler: self,
            source: Source::detached(source),
            time: time::OffsetDateTime::now_local().unwrap_or(time::OffsetDateTime::now_utc()),
        }
    }

    /// Gets the package directory, downloading it if it doesn't exist.
    ///
    /// Packages are downloaded from `packages.typst.org` and extracted
    /// to the cache directory.
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

    /// Gets the raw bytes of a file, loading and caching if necessary.
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

    /// Gets a parsed source file, loading and caching if necessary.
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

    /// Renders Typst source code to SVG.
    ///
    /// Compiles the given Typst source and returns the rendered pages
    /// as concatenated SVG strings.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError::Compilation`] if the Typst code fails to compile.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let compiler = Compiler::new();
    /// let svg = compiler.render("$ E = m c^2 $")?;
    /// ```
    pub fn render(&self, source: impl Into<String>) -> Result<String, CompileError> {
        let source = source.into();
        let world = self.wrap_source(source);
        let result = typst::compile::<PagedDocument>(&world);

        // Log warnings if any
        for warning in &result.warnings {
            eprintln!("Typst warning: {:?}", warning);
        }

        let document = result
            .output
            .map_err(|diags| CompileError::Compilation(format!("{:?}", diags)))?;

        let images = document.pages.iter().map(svg).collect::<Vec<_>>();
        let images = images.join("\n");

        Ok(images)
    }
}

/// A wrapper that provides a complete Typst [`World`] for compilation.
///
/// This struct combines a [`Compiler`] reference with a specific source
/// document and timestamp, implementing all the traits needed for Typst
/// compilation.
pub struct WrapSource<'a> {
    /// Reference to the compiler providing fonts and file access.
    compiler: &'a Compiler,
    /// The main source document to compile.
    source: Source,
    /// The time to use for date-related Typst functions.
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
