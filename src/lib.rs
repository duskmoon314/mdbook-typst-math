use std::path::PathBuf;

use anyhow::anyhow;
use mdbook_preprocessor::book::{Book, BookItem, Chapter};
use mdbook_preprocessor::errors::Result;
use mdbook_preprocessor::{Preprocessor, PreprocessorContext};
use pulldown_cmark::{Event, Options, Parser};
use serde::Deserialize;

mod compiler;
use compiler::Compiler;
use typst::foundations::Bytes;
use typst::text::{Font, FontInfo};

/// Options that are passed to the compile step
pub struct TypstProcessorOptions {
    /// preamble to be added before each content
    ///
    /// This is used as fallback if the following options are not set
    pub preamble: String,
    /// preamble to be added before each inline math
    pub inline_preamble: Option<String>,
    /// preamble to be added before each display math
    pub display_preamble: Option<String>,
}

/// Configuration for the typst-math preprocessor from book.toml
#[derive(Debug, Clone, Deserialize)]
struct TypstMathConfig {
    preamble: Option<String>,
    inline_preamble: Option<String>,
    display_preamble: Option<String>,
    fonts: Option<Vec<String>>,
    cache: Option<String>,
}

pub struct TypstProcessor;

impl Preprocessor for TypstProcessor {
    fn name(&self) -> &str {
        "typst-math"
    }

    fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book> {
        let config: Option<TypstMathConfig> = ctx
            .config
            .get(&format!("preprocessor.{}", self.name()))
            .ok()
            .flatten();
        let mut compiler = Compiler::new();

        // Set options
        let mut opts = TypstProcessorOptions {
            preamble: String::from("#set page(width: auto, height: auto, margin: 0.5em)"),
            inline_preamble: None,
            display_preamble: None,
        };
        if let Some(ref cfg) = config {
            if let Some(ref preamble) = cfg.preamble {
                opts.preamble = preamble.clone();
            }
            if let Some(ref inline_preamble) = cfg.inline_preamble {
                opts.inline_preamble = Some(inline_preamble.clone());
            }
            if let Some(ref display_preamble) = cfg.display_preamble {
                opts.display_preamble = Some(display_preamble.clone());
            }
        }

        let mut db = fontdb::Database::new();
        // Load fonts from the config
        if let Some(ref cfg) = config {
            if let Some(ref fonts) = cfg.fonts {
                for font_path in fonts {
                    db.load_fonts_dir(font_path);
                }
            }
        }
        // Load system fonts, lower priority
        db.load_system_fonts();

        // Add all fonts in db to the compiler
        for face in db.faces() {
            let info = db
                .with_face_data(face.id, FontInfo::new)
                .expect("Failed to load font info");
            if let Some(info) = info {
                compiler.book.push(info);
                if let Some(font) = match &face.source {
                    fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => {
                        let bytes = std::fs::read(path).expect("Failed to read font file");
                        Font::new(Bytes::new(bytes), face.index)
                    }
                    fontdb::Source::Binary(data) => {
                        Font::new(Bytes::new(data.as_ref().as_ref().to_vec()), face.index)
                    }
                } {
                    compiler.fonts.push(font);
                }
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
        if let Some(ref cfg) = config {
            if let Some(ref cache) = cfg.cache {
                compiler.cache = PathBuf::from(cache);
            }
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
        chapter: &mut Chapter,
        compiler: &Compiler,
        opts: &TypstProcessorOptions,
    ) -> Result<String> {
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

            let svg = compiler.render(block.clone()).map_err(|e| anyhow!(e))?;

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
