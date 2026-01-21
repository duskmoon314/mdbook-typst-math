//! mdbook-typst-math - An mdbook preprocessor to render math using Typst
//!
//! This crate provides a preprocessor for mdbook that converts LaTeX-style
//! math blocks into SVG images rendered by Typst.
//!
//! # Usage
//!
//! Add the preprocessor to your `book.toml`:
//!
//! ```toml
//! [preprocessor.typst-math]
//! ```
//!
//! # Configuration
//!
//! The preprocessor supports the following configuration options:
//!
//! - `preamble`: Typst code to prepend to all math blocks
//! - `inline_preamble`: Typst code to prepend to inline math blocks
//! - `display_preamble`: Typst code to prepend to display math blocks
//! - `fonts`: List of font directories to load
//! - `cache`: Directory for caching downloaded packages
//! - `color_mode`: Color mode for SVG output (`auto` or `static`)
//! - `code_tag`: Language tag for code blocks to render as Typst (default: `typst,render`)
//! - `enable_math`: Enable rendering of math blocks (default: `true`)
//! - `enable_code`: Enable rendering of Typst code blocks (default: `true`)

use std::path::PathBuf;

use anyhow::anyhow;
use mdbook_preprocessor::book::{Book, BookItem, Chapter};
use mdbook_preprocessor::errors::Result;
use mdbook_preprocessor::{Preprocessor, PreprocessorContext};
use serde::Deserialize;

mod compiler;
use compiler::{CompileError, Compiler};
use typst::foundations::Bytes;
use typst::text::{Font, FontInfo};

/// Options that control how Typst renders math blocks.
///
/// These options allow customization of the Typst preamble used for
/// inline and display math rendering.
pub struct TypstProcessorOptions {
    /// Default preamble added before each math block.
    ///
    /// This is used as a fallback if `inline_preamble` or `display_preamble`
    /// is not set. The default value sets up an auto-sized page with minimal margins.
    pub preamble: String,
    /// Optional preamble specifically for inline math (`$...$`).
    ///
    /// If `None`, the default `preamble` is used instead.
    pub inline_preamble: Option<String>,
    /// Optional preamble specifically for display math (`$$...$$`).
    ///
    /// If `None`, the default `preamble` is used instead.
    pub display_preamble: Option<String>,
    /// Color mode for SVG output.
    ///
    /// When set to `Auto`, black color (`#000000`) in SVG will be replaced
    /// with `currentColor`, allowing CSS to control the text color for
    /// theme support (light/dark mode).
    pub color_mode: ColorMode,
    /// Language tag for code blocks to render as Typst.
    pub code_tag: String,
    /// Enable rendering of math blocks (inline and display math).
    pub enable_math: bool,
    /// Enable rendering of Typst code blocks.
    pub enable_code: bool,
}

/// Color mode for SVG output.
///
/// This controls how the preprocessor handles colors in the generated SVG.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// Replace black (`#000000`) with `currentColor` for CSS theme support.
    ///
    /// This is the default mode, which allows the SVG text color to adapt
    /// to light/dark themes via CSS.
    #[default]
    Auto,
    /// Keep colors as-is from Typst output.
    ///
    /// Use this mode if you want to preserve exact colors specified in Typst,
    /// or if you're using a fixed background color.
    Static,
}

/// Represents font configuration that accepts either a single string or an array.
///
/// This allows users to specify fonts in `book.toml` as either:
/// - `fonts = "path/to/fonts"`
/// - `fonts = ["path1", "path2"]`
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum FontsConfig {
    Single(String),
    Multiple(Vec<String>),
}

impl FontsConfig {
    fn into_vec(self) -> Vec<String> {
        match self {
            FontsConfig::Single(s) => vec![s],
            FontsConfig::Multiple(v) => v,
        }
    }
}

/// Configuration for the typst-math preprocessor from book.toml
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct TypstMathConfig {
    /// The preamble to prepend to all math blocks.
    preamble: Option<String>,

    /// Optional preamble for inline math blocks.
    inline_preamble: Option<String>,

    /// Optional preamble for display math blocks.
    display_preamble: Option<String>,

    /// Custom fonts to load
    fonts: Option<FontsConfig>,

    /// Cache directory for downloaded packages
    cache: Option<String>,
    #[serde(default)]
    color_mode: ColorMode,

    /// Language tag for code blocks to render as Typst.
    /// Defaults to "typst,render" if not specified.
    code_tag: Option<String>,

    /// Enable rendering of math blocks (inline and display math).
    /// Defaults to true if not specified.
    enable_math: Option<bool>,

    /// Enable rendering of Typst code blocks.
    /// Defaults to true if not specified.
    enable_code: Option<bool>,
}

/// The main preprocessor that converts math blocks to Typst-rendered SVGs.
///
/// This preprocessor scans markdown content for inline math (`$...$`) and
/// display math (`$$...$$`) blocks, renders them using Typst, and replaces
/// them with SVG images wrapped in appropriate HTML elements.
///
/// # Example
///
/// ```ignore
/// use mdbook_typst_math::TypstProcessor;
/// use mdbook_preprocessor::Preprocessor;
///
/// let processor = TypstProcessor;
/// assert_eq!(processor.name(), "typst-math");
/// ```
pub struct TypstProcessor;

impl Preprocessor for TypstProcessor {
    fn name(&self) -> &str {
        "typst-math"
    }

    fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book> {
        let config: TypstMathConfig = ctx
            .config
            .get(&format!("preprocessor.{}", self.name()))
            .ok()
            .flatten()
            .unwrap_or_default();
        let mut compiler = Compiler::new();

        // Set options from config
        let opts = TypstProcessorOptions {
            preamble: config.preamble.unwrap_or_else(|| {
                String::from("#set page(width: auto, height: auto, margin: 0.5em, fill: none)")
            }),
            inline_preamble: config.inline_preamble,
            display_preamble: config.display_preamble,
            color_mode: config.color_mode,
            code_tag: config
                .code_tag
                .unwrap_or_else(|| String::from("typst,render")),
            enable_math: config.enable_math.unwrap_or(true),
            enable_code: config.enable_code.unwrap_or(true),
        };

        let mut db = fontdb::Database::new();
        // Load fonts from the config
        if let Some(fonts) = config.fonts {
            for font_path in fonts.into_vec() {
                let path = std::path::Path::new(&font_path);
                if path.is_file() {
                    // Load single font file
                    if let Err(e) = db.load_font_file(&font_path) {
                        eprintln!("Warning: Failed to load font file {:?}: {}", font_path, e);
                    }
                } else if path.is_dir() {
                    // Load all fonts from directory
                    db.load_fonts_dir(&font_path);
                } else {
                    eprintln!("Warning: Font path does not exist: {:?}", font_path);
                }
            }
        }
        // Load system fonts, lower priority
        db.load_system_fonts();

        // Add all fonts in db to the compiler
        for face in db.faces() {
            let Some(info) = db.with_face_data(face.id, FontInfo::new).flatten() else {
                eprintln!(
                    "Warning: Failed to load font info for {:?}, skipping",
                    face.source
                );
                continue;
            };
            compiler.book.push(info);
            let font = match &face.source {
                fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => {
                    match std::fs::read(path) {
                        Ok(bytes) => Font::new(Bytes::new(bytes), face.index),
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to read font file {:?}: {}, skipping",
                                path, e
                            );
                            continue;
                        }
                    }
                }
                fontdb::Source::Binary(data) => {
                    Font::new(Bytes::new(data.as_ref().as_ref().to_vec()), face.index)
                }
            };
            if let Some(font) = font {
                compiler.fonts.push(font);
            }
        }

        #[cfg(feature = "embed-fonts")]
        {
            // Load typst embedded fonts, lowest priority
            for data in typst_assets::fonts() {
                let buffer = Bytes::new(data);
                for font in Font::iter(buffer) {
                    compiler.book.push(font.info().clone());
                    compiler.fonts.push(font);
                }
            }
        }

        // Set the cache dir
        if let Some(ref cache) = config.cache {
            compiler.cache = PathBuf::from(cache);
        }

        // record if any errors occurred
        let mut res = None;

        book.for_each_mut(|item| {
            if let Some(Err(_)) = res {
                return;
            }

            if let BookItem::Chapter(ref mut chapter) = *item {
                res = Some(self.convert_typst(chapter, &compiler, &opts).map(|c| {
                    chapter.content = c;
                }))
            }
        });

        res.unwrap_or(Ok(())).map(|_| book)
    }

    fn supports_renderer(&self, renderer: &str) -> Result<bool> {
        Ok(renderer == "html")
    }
}

impl TypstProcessor {
    fn convert_typst(
        &self,
        chapter: &Chapter,
        compiler: &Compiler,
        opts: &TypstProcessorOptions,
    ) -> Result<String> {
        use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

        // Construct filename from chapter name and source path
        let filename = if let Some(ref path) = chapter.source_path {
            format!("{} {}", chapter.name, path.display())
        } else {
            chapter.name.clone()
        };
        let mut typst_blocks = Vec::new();

        let mut pulldown_cmark_opts = Options::empty();
        pulldown_cmark_opts.insert(Options::ENABLE_TABLES);
        pulldown_cmark_opts.insert(Options::ENABLE_FOOTNOTES);
        pulldown_cmark_opts.insert(Options::ENABLE_STRIKETHROUGH);
        pulldown_cmark_opts.insert(Options::ENABLE_TASKLISTS);
        pulldown_cmark_opts.insert(Options::ENABLE_MATH);

        let mut in_typst_code_block = false;
        let mut code_block_start: Option<std::ops::Range<usize>> = None;
        let mut code_block_content = String::new();

        let parser = Parser::new_ext(&chapter.content, pulldown_cmark_opts);
        for (e, span) in parser.into_offset_iter() {
            match e {
                Event::InlineMath(math_content) if opts.enable_math => {
                    let preamble = opts.inline_preamble.as_ref().unwrap_or(&opts.preamble);
                    typst_blocks.push((
                        span.clone(),
                        format!("{}\n${math_content}$", preamble),
                        true,
                        preamble.lines().count(), // preamble line count
                    ));
                }
                Event::DisplayMath(math_content) if opts.enable_math => {
                    let math_content = math_content.trim();
                    let preamble = opts.display_preamble.as_ref().unwrap_or(&opts.preamble);
                    typst_blocks.push((
                        span.clone(),
                        format!("{}\n$ {math_content} $", preamble),
                        false,
                        preamble.lines().count(), // preamble line count
                    ));
                }
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) if opts.enable_code => {
                    if lang.as_ref() == opts.code_tag.as_str() {
                        in_typst_code_block = true;
                        code_block_start = Some(span.clone());
                        code_block_content.clear();
                    }
                }
                Event::Text(text) if in_typst_code_block && opts.enable_code => {
                    code_block_content.push_str(&text);
                }
                Event::End(TagEnd::CodeBlock) if in_typst_code_block && opts.enable_code => {
                    if let Some(start_span) = code_block_start.take() {
                        let preamble = opts.display_preamble.as_ref().unwrap_or(&opts.preamble);
                        let full_span = start_span.start..span.end;

                        typst_blocks.push((
                            full_span,
                            format!("{}\n{}", preamble, code_block_content.trim()),
                            false, // Display mode
                            preamble.lines().count(),
                        ));
                    }
                    in_typst_code_block = false;
                    code_block_content.clear();
                }
                _ => {}
            }
        }

        let mut content = chapter.content.to_string();

        for (span, block, inline, preamble_lines) in typst_blocks.iter().rev() {
            let pre_content = &content[0..span.start];
            let post_content = &content[span.end..];

            // Calculate the line number in the original markdown
            let markdown_line = chapter.content[..span.start].lines().count() + 1;

            let mut svg = compiler
                .render(
                    block.clone(),
                    Some(&filename),
                    markdown_line,
                    *preamble_lines,
                )
                .map_err(|e: CompileError| {
                    anyhow!("Failed to render math in chapter '{}': {}", filename, e)
                })?;

            // Apply color mode transformation
            if opts.color_mode == ColorMode::Auto {
                svg = svg.replace(r##"fill="#000000""##, r#"fill="currentColor""#);
                svg = svg.replace(r##"stroke="#000000""##, r#"stroke="currentColor""#);
            }

            content = match inline {
                true => format!(
                    "{}<span class=\"typst-inline\">{}</span>{}",
                    pre_content, svg, post_content
                ),
                false => format!(
                    "{}<div class=\"typst-display\">{}</div>{}",
                    pre_content, svg, post_content
                ),
            };
        }

        Ok(content)
    }
}
