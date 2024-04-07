use std::path::PathBuf;

use anyhow::anyhow;
use mdbook::book::{Book, Chapter};
use mdbook::errors::Result;
use mdbook::preprocess::{Preprocessor, PreprocessorContext};
use mdbook::BookItem;
use pulldown_cmark::{Event, Options, Parser};

mod compiler;
use compiler::Compiler;
use typst::foundations::Bytes;
use typst::text::{Font, FontInfo};

pub struct TypstProcessor;

impl Preprocessor for TypstProcessor {
    fn name(&self) -> &str {
        "typst"
    }

    fn run(&self, ctx: &PreprocessorContext, mut book: Book) -> Result<Book> {
        let config = ctx.config.get_preprocessor(self.name());
        let mut compiler = Compiler::new();

        let mut db = fontdb::Database::new();
        // Load fonts from the config
        if let Some(fonts) = config.and_then(|c| c.get("fonts")) {
            if let Some(fonts) = fonts.as_array() {
                for font in fonts {
                    let font = font.as_str().unwrap();
                    db.load_fonts_dir(font);
                }
            };
            if let Some(font) = fonts.as_str() {
                db.load_fonts_dir(font);
            };
        }
        // Load system fonts, lower priority
        db.load_system_fonts();

        // Add all fonts to the compiler
        for face in db.faces() {
            let info = db
                .with_face_data(face.id, FontInfo::new)
                .expect("Failed to load font info");
            if let Some(info) = info {
                compiler.book.update(|book| book.push(info));
                if let Some(font) = match &face.source {
                    fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => {
                        let bytes = std::fs::read(path).expect("Failed to read font file");
                        Font::new(Bytes::from(bytes), face.index)
                    }
                    fontdb::Source::Binary(data) => {
                        Font::new(Bytes::from(data.as_ref().as_ref()), face.index)
                    }
                } {
                    compiler.fonts.push(font);
                }
            }
        }

        // Set the cache dir
        if let Some(cache) = config.and_then(|c| c.get("cache")) {
            compiler.cache = cache
                .as_str()
                .map(PathBuf::from)
                .expect("cache dir must be a string");
        }

        // record if any errors occurred
        let mut res = None;

        book.for_each_mut(|item| {
            if let Some(Err(_)) = res {
                return;
            }

            if let BookItem::Chapter(ref mut chapter) = *item {
                res = Some(self.convert_typst(chapter, &compiler).map(|c| {
                    chapter.content = c;
                }))
            }
        });

        res.unwrap_or(Ok(())).map(|_| book)
    }

    fn supports_renderer(&self, renderer: &str) -> bool {
        renderer == "html"
    }
}

impl TypstProcessor {
    fn convert_typst(&self, chapter: &mut Chapter, compiler: &Compiler) -> Result<String> {
        let mut typst_blocks = Vec::new();

        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_FOOTNOTES);
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TASKLISTS);
        opts.insert(Options::ENABLE_MATH);

        let parser = Parser::new_ext(&chapter.content, opts);
        for (e, span) in parser.into_offset_iter() {
            if let Event::InlineMath(math_content) = e {
                typst_blocks.push((
                    span,
                    format!("#set page(width: auto, height: auto, margin: 0.5em)\n{math_content}"),
                    true,
                ))
            } else if let Event::DisplayMath(math_content) = e {
                let math_content = math_content.trim();
                typst_blocks.push((
                    span,
                    format!("#set page(width: auto, height: auto, margin: 0.5em)\n{math_content}"),
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
