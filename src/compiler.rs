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
    comemo::Track,
    diag::{
        eco_format, FileError, FileResult, PackageError, PackageResult, SourceDiagnostic, Warned,
    },
    foundations::{Bytes, Datetime, Duration},
    model::LateLinkResolver,
    syntax::{
        package::PackageSpec, DiagSpan, FileId, Lines, RootedPath, Source, VirtualPath, VirtualRoot,
    },
    text::{Font, FontBook},
    utils::LazyHash,
    Feature, Library, LibraryExt, World, WorldExt,
};
use typst_html::{html_in_bundle, tag, HtmlDocument, HtmlElement, HtmlNode, HtmlOptions};
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;
use typst_svg::{svg, SvgOptions};

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

/// HTML/MathML output for one math block.
pub struct HtmlMath {
    /// The serialized `<math>` element.
    pub fragment: String,
    /// The MathML stylesheet emitted by Typst, if any.
    pub style: Option<String>,
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
    /// Configured fonts and lazy slots in the same order as `book`.
    pub fonts: FontStore,
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
            fonts: FontStore::new(),
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

    /// Enables Typst's experimental HTML target for MathML rendering.
    pub fn enable_html(&mut self) {
        let features = [Feature::Html].into_iter().collect();
        self.library = LazyHash::new(Library::builder().with_features(features).build());
    }

    /// Wraps a source string into a [`WrapSource`] that implements [`World`].
    ///
    /// This creates a complete Typst world context for compilation,
    /// capturing the current local time for date-related functions.
    ///
    /// # Parameters
    ///
    /// - `source`: The Typst source code to compile
    /// - `filename`: Optional filename to use in diagnostics (e.g., chapter name)
    /// - `markdown_line`: The line number in the original markdown file (1-indexed)
    /// - `preamble_lines`: Number of lines in the preamble before the actual content
    pub fn wrap_source(
        &self,
        source: impl Into<String>,
        filename: Option<&str>,
        markdown_line: usize,
        preamble_lines: usize,
    ) -> WrapSource<'_> {
        let source_str = source.into();
        let source = if let Some(name) = filename {
            // Typst virtual paths use forward slashes and reject paths that
            // escape the project root. Chapter names and Windows paths can
            // contain backslashes, so normalize them before interning.
            let normalized = name.replace('\\', "/");
            let vpath = VirtualPath::new(normalized).unwrap_or_else(|_| {
                VirtualPath::new("main.typ").expect("a static Typst path must be valid")
            });
            let file_id = RootedPath::new(VirtualRoot::Project, vpath).intern();
            Source::new(file_id, source_str)
        } else {
            Source::detached(source_str)
        };

        WrapSource {
            compiler: self,
            source,
            time: time::OffsetDateTime::now_local().unwrap_or(time::OffsetDateTime::now_utc()),
            markdown_line,
            preamble_lines,
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
            PackageError::NetworkFailed(Some(eco_format!("{}: {}", package.name, e)))
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
        if let VirtualRoot::Package(package) = id.root() {
            let package_dir = self.package(package)?;
            let path = id
                .vpath()
                .realize(&package_dir)
                .map_err(|_| FileError::NotFound(id.vpath().get_without_slash().into()))?;
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

        Err(FileError::NotFound(id.vpath().get_without_slash().into()))
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
    /// # Parameters
    ///
    /// - `source`: The Typst source code to render
    /// - `filename`: Optional filename to use in diagnostics (e.g., chapter name)
    /// - `markdown_line`: The line number in the original markdown file (1-indexed)
    /// - `preamble_lines`: Number of lines in the preamble before the actual content
    ///
    /// # Errors
    ///
    /// Returns [`CompileError::Compilation`] if the Typst code fails to compile.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let compiler = Compiler::new();
    /// let svg = compiler.render("$ E = m c^2 $", Some("chapter1.md"), 42, 1)?;
    /// ```
    pub fn render(
        &self,
        source: impl Into<String>,
        filename: Option<&str>,
        markdown_line: usize,
        preamble_lines: usize,
    ) -> Result<String, CompileError> {
        let source = source.into();
        let world = self.wrap_source(source, filename, markdown_line, preamble_lines);

        let Warned { output, warnings } = typst::compile::<PagedDocument>(&world);

        match output {
            Ok(document) => {
                print_diagnostics(&world, &warnings, &[])?;
                let options = SvgOptions::default();
                let images = document
                    .pages()
                    .iter()
                    .map(|page| svg(page, &options))
                    .collect::<Vec<_>>();
                let images = images
                    .into_iter()
                    .map(|mut image| {
                        // Typst 0.15 no longer emits the legacy `typst-doc`
                        // class. Keep it for backwards-compatible styling in
                        // mdBook themes and existing user CSS.
                        if image.starts_with("<svg ") && !image.starts_with("<svg class=") {
                            image = image.replacen("<svg ", r#"<svg class="typst-doc" "#, 1);
                        }
                        image
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(images)
            }
            Err(errors) => {
                print_diagnostics(&world, &warnings, &errors)?;
                Err(CompileError::Compilation(
                    "typst compilation failed".to_string(),
                ))
            }
        }
    }

    /// Renders a math block to an HTML/MathML fragment.
    ///
    /// Typst's HTML encoder produces a complete document. We use its public
    /// DOM and bundle encoder to serialize only the generated `<math>` node,
    /// preserving MathML and any inline SVG fallback nodes without reparsing
    /// the encoded HTML string.
    pub fn render_html_math(
        &self,
        source: impl Into<String>,
        filename: Option<&str>,
        markdown_line: usize,
        preamble_lines: usize,
    ) -> Result<HtmlMath, CompileError> {
        let source = source.into();
        let world = self.wrap_source(source, filename, markdown_line, preamble_lines);
        let Warned { output, warnings } = typst::compile::<HtmlDocument>(&world);

        let document = match output {
            Ok(document) => {
                print_diagnostics(&world, &warnings, &[])?;
                document
            }
            Err(errors) => {
                print_diagnostics(&world, &warnings, &errors)?;
                return Err(CompileError::Compilation(
                    "typst HTML compilation failed".to_string(),
                ));
            }
        };

        let body = document
            .root()
            .children
            .iter()
            .find_map(|node| match node {
                HtmlNode::Element(element) if element.tag == tag::body => Some(element),
                _ => None,
            })
            .ok_or_else(|| {
                CompileError::Compilation(
                    "typst HTML output did not contain a body element".to_string(),
                )
            })?;

        // A standalone inline equation is grouped into a paragraph by the
        // HTML exporter, while a display equation is emitted directly. Walk
        // the tree recursively so user show rules that add an extra wrapper
        // do not make the fragment impossible to extract.
        let math = find_math_element(body);

        let math = math.ok_or_else(|| {
            CompileError::Compilation(
                "typst HTML output did not contain a math element".to_string(),
            )
        })?;

        let fragment = encode_html_element(&document, math)?;

        let style = document
            .root()
            .children
            .iter()
            .find_map(|node| match node {
                HtmlNode::Element(element) if element.tag == tag::head => {
                    element.children.iter().find_map(|child| match child {
                        HtmlNode::Element(style) if style.tag == tag::style => Some(style),
                        _ => None,
                    })
                }
                _ => None,
            })
            .map(|style| encode_html_element(&document, style))
            .transpose()?;

        Ok(HtmlMath { fragment, style })
    }
}

/// Finds the first MathML `<math>` element in an HTML subtree.
fn find_math_element(element: &HtmlElement) -> Option<&HtmlElement> {
    for node in &element.children {
        match node {
            HtmlNode::Element(child) if child.tag == tag::mathml::math => return Some(child),
            HtmlNode::Element(child) => {
                if let Some(math) = find_math_element(child) {
                    return Some(math);
                }
            }
            _ => {}
        }
    }
    None
}

/// Serializes one Typst HTML DOM element without reparsing the full document.
fn encode_html_element(
    document: &HtmlDocument,
    element: &HtmlElement,
) -> Result<String, CompileError> {
    let resolver = LateLinkResolver::new(None, document.introspector().as_ref());
    let encoded =
        html_in_bundle(element, &HtmlOptions::default(), resolver.track()).map_err(|errors| {
            CompileError::Compilation(format!("failed to encode HTML: {errors:?}"))
        })?;
    Ok(encoded
        .strip_prefix("<!DOCTYPE html>")
        .unwrap_or(&encoded)
        .to_string())
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
    /// The line number in the original markdown file where this block starts (1-indexed).
    markdown_line: usize,
    /// Number of lines in the preamble before the actual math content.
    preamble_lines: usize,
}

impl WrapSource<'_> {
    pub fn lookup(&self, id: FileId) -> Lines<String> {
        if let Ok(source) = self.compiler.get_source(id) {
            source.lines().clone()
        } else if let Ok(bytes) = self.compiler.get_file(id) {
            let text = std::str::from_utf8(bytes.as_slice()).expect("not valid utf-8");
            Lines::new(text.to_owned())
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
        self.compiler.fonts.font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
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
        Ok(match id.root() {
            VirtualRoot::Package(package) => {
                format!("{package}{}", vpath.get_with_slash())
            }
            VirtualRoot::Project => vpath.get_without_slash().to_owned(),
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
        let typst_line = source.byte_to_line(byte_index).ok_or_else(|| {
            codespan_reporting::files::Error::IndexTooLarge {
                given: byte_index,
                max: source.len_bytes(),
            }
        })?;

        // Adjust line number to point to the original markdown file
        if id == self.source.id() && typst_line >= self.preamble_lines {
            // Line is in the actual content (after preamble)
            // Both markdown_line and returned value are 0-indexed
            Ok(self.markdown_line - 1 + (typst_line - self.preamble_lines))
        } else {
            Ok(typst_line)
        }
    }

    fn line_range(
        &'a self,
        id: Self::FileId,
        line_index: usize,
    ) -> Result<std::ops::Range<usize>, codespan_reporting::files::Error> {
        let source = self.lookup(id);

        // Convert adjusted markdown line back to Typst line
        let typst_line = if id == self.source.id() && line_index >= self.markdown_line - 1 {
            // This is an adjusted line number, convert back to Typst line
            self.preamble_lines + (line_index - (self.markdown_line - 1))
        } else {
            line_index
        };

        source.line_to_range(typst_line).ok_or_else(|| {
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

fn label(world: &WrapSource, span: DiagSpan) -> Option<Label<FileId>> {
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
                .map(|s| eco_format!("hint: {}", s.v).into())
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

#[cfg(all(test, feature = "embed-fonts"))]
mod tests {
    use super::*;

    fn compiler_with_embedded_fonts() -> Compiler {
        let mut compiler = Compiler::new();
        let mut fonts = FontStore::new();

        fonts.extend(typst_kit::fonts::embedded());

        compiler.book = fonts.book().clone();
        compiler.fonts = fonts;
        compiler
    }

    #[test]
    fn html_math_renders_mathml() {
        let mut compiler = compiler_with_embedded_fonts();
        compiler.enable_html();

        let output = compiler
            .render_html_math("$ a_1 + frac(1, 2) $", Some("math.md"), 1, 0)
            .expect("HTML math should compile");

        assert!(output.fragment.starts_with("<math"));
        assert!(output.fragment.contains("<msub>"));
        assert!(output.fragment.contains("<mfrac>"));
        assert!(output.style.is_some());
    }

    #[test]
    fn html_math_marks_display_equations() {
        let mut compiler = compiler_with_embedded_fonts();
        compiler.enable_html();

        let output = compiler
            .render_html_math("$ x^2 $", Some("math.md"), 1, 0)
            .expect("HTML math should compile");

        assert!(
            output.fragment.starts_with("<math display=\"block\">")
                || output.fragment.starts_with("<math display=\"block\" ")
        );
        assert!(output.fragment.contains("<msup>"));
    }

    #[test]
    fn svg_render_keeps_legacy_document_class() {
        let compiler = compiler_with_embedded_fonts();
        let output = compiler
            .render("$ x^2 $", Some("math.md"), 1, 0)
            .expect("SVG math should compile");

        assert!(output.starts_with("<svg class=\"typst-doc\""));
    }

    #[test]
    fn html_math_reports_compile_errors() {
        let mut compiler = compiler_with_embedded_fonts();
        compiler.enable_html();

        let error = compiler.render_html_math("#let =", Some("math.md"), 1, 0);

        assert!(matches!(error, Err(CompileError::Compilation(_))));
    }
}
