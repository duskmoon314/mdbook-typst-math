use anyhow::anyhow;
use mdbook::book::{Book, Chapter};
use mdbook::errors::Result;
use mdbook::preprocess::{Preprocessor, PreprocessorContext};
use mdbook::BookItem;
use pulldown_cmark::{Event, Options, Parser};

mod compiler;
use compiler::Compiler;
use typst::foundations::Bytes;
use typst::text::Font;

pub struct TypstProcessor {
    compiler: Compiler,
}

impl TypstProcessor {
    pub fn new() -> Self {
        // Read the default font dir
        // TODO: handle all OSes
        let fonts = glob::glob("/usr/share/fonts/**/*.ttf")
            .unwrap()
            .map(Result::unwrap)
            .flat_map(|path| {
                let bytes = std::fs::read(&path).unwrap();
                let buffer = Bytes::from(bytes);
                let face_count = ttf_parser::fonts_in_collection(&buffer).unwrap_or(1);
                (0..face_count).map(move |face| {
                    Font::new(buffer.clone(), face)
                        .unwrap_or_else(|| panic!("Failed to load font {:?} face {}", path, face))
                })
            })
            .collect();

        Self {
            compiler: Compiler::new(fonts),
        }
    }
}

impl Preprocessor for TypstProcessor {
    fn name(&self) -> &str {
        "typst"
    }

    fn run(&self, _ctx: &PreprocessorContext, mut book: Book) -> Result<Book> {
        // TODO: use config of the preprocessor

        // record if any errors occurred
        let mut res = None;

        book.for_each_mut(|item| {
            if let Some(Err(_)) = res {
                return;
            }

            if let BookItem::Chapter(ref mut chapter) = *item {
                res = Some(self.convert_typst(chapter).map(|c| {
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
    fn convert_typst(&self, chapter: &mut Chapter) -> Result<String> {
        let mut typst_blocks = Vec::new();

        // let mut typst_content = String::new();
        // let mut in_typst_block = false;
        // let mut new_code_span_start = true;
        // let mut code_span = 0..0;

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

            let svg = self
                .compiler
                .render(block.clone())
                .map_err(|e| anyhow!(e))?;

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
