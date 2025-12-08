//! Customized Typst compiler for mdbook preprocessor.
//!
//! This module provides a [`Compiler`] that wraps Typst's compilation functionality,
//! handling font loading, package management, and source compilation.
//!
//! Highly inspired by the [typst-bot](https://github.com/mattfbacon/typst-bot).

use std::{collections::HashMap, fmt, io::Write, path::PathBuf, sync::RwLock};

use codespan_reporting::{
    diagnostic::{Diagnostic, Label},
    term,
};
use tracing::{error, warn};
use typst::{
    diag::{
        eco_format, FileError, FileResult, PackageError, PackageResult, SourceDiagnostic, Warned,
    },
    foundations::{Bytes, Datetime},
    layout::PagedDocument,
    syntax::{package::PackageSpec, FileId, Lines, Source, Span},
    text::{Font, FontBook},
    utils::LazyHash,
    Library, LibraryExt, World, WorldExt,
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

        let Warned { output, warnings } = typst::compile::<PagedDocument>(&world);

        match output {
            Ok(document) => {
                print_diagnostics(&world, &warnings, &[])?;
                let images = document.pages.iter().map(svg).collect::<Vec<_>>();
                let images = images.join("\n");
                Ok(images)
            }
            Err(errors) => {
                print_diagnostics(&world, &warnings, &errors)?;
                Err(CompileError::Compilation(format!(
                    "typst compilation failed"
                )))
            }
        }
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

impl WrapSource<'_> {
    pub fn lookup(&self, id: FileId) -> Lines<String> {
        if let Ok(source) = self.compiler.get_source(id) {
            source.lines().clone()
        } else if let Ok(bytes) = self.compiler.get_file(id) {
            Lines::try_from(&bytes).expect("not valid utf-8")
        } else {
            self.source.lines().clone()
        }
    }
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

// Mostly copied from typst: https://github.com/typst/typst/blob/7fb4aa0aec314bb8ef99b8096d8d65a8e63b17e6/crates/typst-cli/src/compile.rs#L680
impl<'a> codespan_reporting::files::Files<'a> for WrapSource<'a> {
    type FileId = FileId;
    type Name = String;
    type Source = Lines<String>;

    fn name(&'a self, id: Self::FileId) -> Result<Self::Name, codespan_reporting::files::Error> {
        let vpath = id.vpath();
        Ok(if let Some(package) = id.package() {
            format!("{package}{}", vpath.as_rooted_path().display())
        } else {
            format!("{}", vpath.as_rootless_path().display())
        })
    }

    fn source(
        &'a self,
        id: Self::FileId,
    ) -> Result<Self::Source, codespan_reporting::files::Error> {
        Ok(self.lookup(id))
    }

    fn line_index(
        &'a self,
        id: Self::FileId,
        byte_index: usize,
    ) -> Result<usize, codespan_reporting::files::Error> {
        let source = self.lookup(id);
        source.byte_to_line(byte_index).ok_or_else(|| {
            codespan_reporting::files::Error::IndexTooLarge {
                given: byte_index,
                max: source.len_bytes(),
            }
        })
    }

    fn line_range(
        &'a self,
        id: Self::FileId,
        line_index: usize,
    ) -> Result<std::ops::Range<usize>, codespan_reporting::files::Error> {
        let source = self.lookup(id);
        source.line_to_range(line_index).ok_or_else(|| {
            codespan_reporting::files::Error::LineTooLarge {
                given: line_index,
                max: source.len_lines(),
            }
        })
    }

    fn column_number(
        &'a self,
        id: Self::FileId,
        _line_index: usize,
        byte_index: usize,
    ) -> Result<usize, codespan_reporting::files::Error> {
        let source = self.lookup(id);
        source.byte_to_column(byte_index).ok_or_else(|| {
            let max = source.len_bytes();
            if byte_index <= max {
                codespan_reporting::files::Error::InvalidCharBoundary { given: byte_index }
            } else {
                codespan_reporting::files::Error::IndexTooLarge {
                    given: byte_index,
                    max,
                }
            }
        })
    }
}

fn label(world: &WrapSource, span: Span) -> Option<Label<FileId>> {
    Some(Label::primary(span.id()?, world.range(span)?))
}

pub fn print_diagnostics(
    world: &WrapSource,
    warnings: &[SourceDiagnostic],
    errors: &[SourceDiagnostic],
) -> Result<(), CompileError> {
    for diagnostic in warnings.iter().chain(errors) {
        let diag = match diagnostic.severity {
            typst::diag::Severity::Error => Diagnostic::error(),
            typst::diag::Severity::Warning => Diagnostic::warning(),
        }
        .with_message(diagnostic.message.clone())
        .with_notes(
            diagnostic
                .hints
                .iter()
                .map(|s| (eco_format!("hint: {s}")).into())
                .collect(),
        )
        .with_labels(label(world, diagnostic.span).into_iter().collect());

        let diag = term::emit_into_string(&term::Config::default(), world, &diag)
            .map_err(|e| CompileError::Compilation(format! {"Failed to format diagnostic: {e}"}))?;
        match diagnostic.severity {
            typst::diag::Severity::Error => error!("Typst: {diag}"),
            typst::diag::Severity::Warning => warn!("Typst: {diag}"),
        }
    }

    Ok(())
}
