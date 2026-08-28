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
//! - `fonts`: List of font files or directories to load
//! - `include_system_fonts`: Search system font directories (default: `true`)
//! - `cache`: Directory for caching downloaded packages
//! - `color_mode`: Color mode for SVG output (`auto` or `static`)
//! - `code_tag`: Language tag for code blocks to render as Typst (default: `typst,render`)
//! - `enable_math`: Enable rendering of math blocks (default: `true`)
//! - `enable_code`: Enable rendering of Typst code blocks (default: `true`)
//! - `html_math`: Emit math as MathML using Typst's experimental HTML target
//!   (default: `false`)

use std::path::PathBuf;

use anyhow::anyhow;
use mdbook_preprocessor::book::{Book, BookItem, Chapter};
use mdbook_preprocessor::errors::Result;
use mdbook_preprocessor::{Preprocessor, PreprocessorContext};
use serde::Deserialize;

mod compiler;
use compiler::{CompileError, Compiler};
use typst::foundations::Bytes;
use typst::text::Font;
use typst_kit::fonts::FontStore;

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
    /// Render math blocks as HTML/MathML using Typst's experimental HTML target.
    pub html_math: bool,
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

    /// Custom font files or directories to load.
    fonts: Option<FontsConfig>,

    /// Whether to search system font directories. Defaults to true.
    include_system_fonts: Option<bool>,

    /// Whether to render math blocks as HTML/MathML using Typst's experimental
    /// HTML target. Defaults to false.
    html_math: Option<bool>,

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
        let html_math = config.html_math.unwrap_or(false);
        if html_math {
            compiler.enable_html();
        }

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
            html_math,
        };

        let mut font_store = FontStore::new();

        // Keep configured paths in priority order. Explicit files are expected
        // to be few and remain eagerly parsed; directories use lazy sources.
        if let Some(fonts) = config.fonts {
            for font_path in fonts.into_vec() {
                let path = std::path::Path::new(&font_path);
                if path.is_file() {
                    match std::fs::read(path) {
                        Ok(data) => {
                            let mut loaded = false;
                            for font in Font::iter(Bytes::new(data)) {
                                loaded = true;
                                font_store.push((font.clone(), font.info().clone()));
                            }
                            if !loaded {
                                eprintln!("Warning: Failed to parse font file {:?}", font_path);
                            }
                        }
                        Err(e) => {
                            eprintln!("Warning: Failed to load font file {:?}: {}", font_path, e);
                        }
                    }
                } else if path.is_dir() {
                    font_store.extend(typst_kit::fonts::scan(path));
                } else {
                    eprintln!("Warning: Font path does not exist: {:?}", font_path);
                }
            }
        }

        // System and embedded fonts have lower priority than configured paths.
        if config.include_system_fonts.unwrap_or(true) {
            font_store.extend(typst_kit::fonts::system());
        }
        #[cfg(feature = "embed-fonts")]
        font_store.extend(typst_kit::fonts::embedded());

        // Move the configured store into the compiler after adding all
        // sources. The book is cloned because `Compiler` keeps its public
        // metadata field while `FontStore` owns the lazy slots.
        compiler.book = font_store.book().clone();
        compiler.fonts = font_store;

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
                        true,                     // Math blocks can use the HTML/MathML target.
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
                        true,                     // Math blocks can use the HTML/MathML target.
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
                            false, // Typst code blocks continue to use SVG.
                        ));
                    }
                    in_typst_code_block = false;
                    code_block_content.clear();
                }
                _ => {}
            }
        }

        let mut content = chapter.content.to_string();
        let mut html_style = None;

        for (span, block, inline, preamble_lines, is_math) in typst_blocks.iter().rev() {
            let pre_content = &content[0..span.start];
            let post_content = &content[span.end..];

            // Calculate the line number in the original markdown
            let markdown_line = chapter.content[..span.start].lines().count() + 1;

            let rendered = if opts.html_math && *is_math {
                let html = compiler
                    .render_html_math(
                        block.clone(),
                        Some(&filename),
                        markdown_line,
                        *preamble_lines,
                    )
                    .map_err(|e: CompileError| {
                        anyhow!("Failed to render math in chapter '{}': {}", filename, e)
                    })?;
                if html_style.is_none() {
                    html_style = html.style;
                }
                html.fragment
            } else {
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
                svg
            };

            content = match inline {
                true => format!(
                    "{}<span class=\"typst-inline\">{}</span>{}",
                    pre_content, rendered, post_content
                ),
                false => format!(
                    "{}<div class=\"typst-display\">{}</div>{}",
                    pre_content, rendered, post_content
                ),
            };
        }

        if let Some(style) = html_style {
            content = format!("{style}\n{content}");
        }

        Ok(content)
    }
}

#[cfg(all(test, feature = "embed-fonts"))]
mod tests {
    use super::*;
    use mdbook_preprocessor::book::Chapter;
    use std::str::FromStr;

    fn compiler_with_embedded_fonts() -> Compiler {
        let mut compiler = Compiler::new();
        let mut fonts = FontStore::new();

        fonts.extend(typst_kit::fonts::embedded());

        compiler.book = fonts.book().clone();
        compiler.fonts = fonts;
        compiler
    }

    fn options(html_math: bool) -> TypstProcessorOptions {
        TypstProcessorOptions {
            preamble: String::new(),
            inline_preamble: None,
            display_preamble: None,
            color_mode: ColorMode::Auto,
            code_tag: "typst,render".to_string(),
            enable_math: true,
            enable_code: true,
            html_math,
        }
    }

    #[test]
    fn html_math_only_changes_math_blocks() {
        let mut compiler = compiler_with_embedded_fonts();
        compiler.enable_html();
        let chapter = Chapter::new(
            "Test",
            "Inline $x^2$\n\n```typst,render\n#set text(fill: red)\nHello\n```".to_string(),
            "test.md",
            vec![],
        );

        let output = TypstProcessor
            .convert_typst(&chapter, &compiler, &options(true))
            .expect("chapter should compile");

        assert!(output.contains("<span class=\"typst-inline\"><math>"));
        assert!(output.contains("<div class=\"typst-display\"><svg class=\"typst-doc\""));
        assert_eq!(output.matches("<style>").count(), 1);
    }

    #[test]
    fn html_math_disabled_keeps_svg_output() {
        let compiler = compiler_with_embedded_fonts();
        let chapter = Chapter::new("Test", "$x^2$".to_string(), "test.md", vec![]);

        let output = TypstProcessor
            .convert_typst(&chapter, &compiler, &options(false))
            .expect("chapter should compile");

        assert!(!output.contains("<math"));
        assert!(output.contains("<svg class=\"typst-doc\""));
    }

    #[test]
    fn html_math_config_accepts_snake_case() {
        let config = mdbook_preprocessor::config::Config::from_str(
            "[book]\ntitle = \"test\"\n\n[preprocessor.typst-math]\nhtml_math = true\n",
        )
        .expect("configuration should parse");
        let config: TypstMathConfig = config
            .get("preprocessor.typst-math")
            .expect("preprocessor config should deserialize")
            .expect("preprocessor config should be present");

        assert_eq!(config.html_math, Some(true));
    }
}
