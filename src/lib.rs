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

use std::path::PathBuf;

use anyhow::anyhow;
use mdbook_preprocessor::book::{Book, BookItem, Chapter};
use mdbook_preprocessor::errors::Result;
use mdbook_preprocessor::{Preprocessor, PreprocessorContext};
use pulldown_cmark::{Event, Options, Parser};
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
    preamble: Option<String>,
    inline_preamble: Option<String>,
    display_preamble: Option<String>,
    fonts: Option<FontsConfig>,
    cache: Option<String>,
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
                String::from("#set page(width: auto, height: auto, margin: 0.5em)")
            }),
            inline_preamble: config.inline_preamble,
            display_preamble: config.display_preamble,
        };

        let mut db = fontdb::Database::new();
        // Load fonts from the config
        if let Some(fonts) = config.fonts {
            for font_path in fonts.into_vec() {
                db.load_fonts_dir(font_path);
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
        let chapter_name = chapter.name.as_str();
        let mut typst_blocks = Vec::new();

        let mut pulldown_cmark_opts = Options::empty();
        pulldown_cmark_opts.insert(Options::ENABLE_TABLES);
        pulldown_cmark_opts.insert(Options::ENABLE_FOOTNOTES);
        pulldown_cmark_opts.insert(Options::ENABLE_STRIKETHROUGH);
        pulldown_cmark_opts.insert(Options::ENABLE_TASKLISTS);
        pulldown_cmark_opts.insert(Options::ENABLE_MATH);

        let parser = Parser::new_ext(&chapter.content, pulldown_cmark_opts);
        for (e, span) in parser.into_offset_iter() {
            if let Event::InlineMath(math_content) = e {
                typst_blocks.push((
                    span,
                    format!(
                        "{}\n${math_content}$",
                        opts.inline_preamble.as_ref().unwrap_or(&opts.preamble)
                    ),
                    true,
                ))
            } else if let Event::DisplayMath(math_content) = e {
                let math_content = math_content.trim();
                typst_blocks.push((
                    span,
                    format!(
                        "{}\n$ {math_content} $",
                        opts.display_preamble.as_ref().unwrap_or(&opts.preamble)
                    ),
                    false,
                ))
            }
        }

        let mut content = chapter.content.to_string();

        for (span, block, inline) in typst_blocks.iter().rev() {
            let pre_content = &content[0..span.start];
            let post_content = &content[span.end..];

            let svg = compiler.render(block.clone()).map_err(|e: CompileError| {
                anyhow!("Failed to render math in chapter '{}': {}", chapter_name, e)
            })?;

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
