use mdbook::book::{Book, Chapter};
use mdbook::errors::Result;
use mdbook::preprocess::{Preprocessor, PreprocessorContext};
use mdbook::BookItem;
use pulldown_cmark::{CodeBlockKind::*, Event, Options, Parser, Tag};
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

pub struct Typst;

impl Preprocessor for Typst {
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

impl Typst {
    fn convert_typst(&self, chapter: &mut Chapter) -> Result<String> {
        let mut typst_blocks = Vec::new();

        let mut typst_content = String::new();
        let mut in_typst_block = false;
        let mut new_code_span_start = true;
        let mut code_span = 0..0;

        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_FOOTNOTES);
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TASKLISTS);

        let parser = Parser::new_ext(&chapter.content, opts);
        for (e, span) in parser.into_offset_iter() {
            if let Event::Start(Tag::CodeBlock(Fenced(code))) = e.clone() {
                if &*code == "typst" {
                    in_typst_block = true;
                    typst_content.clear();
                }
                continue;
            }

            if !in_typst_block {
                continue;
            }

            // Get text
            if let Event::Text(_) = e {
                if new_code_span_start {
                    code_span = span;
                    new_code_span_start = false;
                } else {
                    code_span.end = span.end;
                }

                continue;
            }

            if let Event::End(Tag::CodeBlock(Fenced(code))) = e {
                assert_eq!(&*code, "typst");
                in_typst_block = false;

                let typst_content = &chapter.content[code_span.clone()];
                // let typst_content = format!(
                //     "#set page(width:auto, height:auto, margin:1em)\n{}",
                //     typst_content
                // );
                typst_blocks.push((span, typst_content));
                new_code_span_start = true;
            }
        }

        let mut content = chapter.content.to_string();

        for (span, block) in typst_blocks.iter().rev() {
            let pre_content = &content[0..span.start];
            let post_content = &content[span.end..];

            let mut temp_src = NamedTempFile::new()?;
            write!(
                temp_src,
                "#set page(width:auto, height:auto, margin:0.5em)\n{}",
                block
            )?;

            let temp_dst = NamedTempFile::new()?;
            Command::new("typst")
                .args([
                    "compile",
                    temp_src.path().to_str().unwrap(),
                    "-f",
                    "svg",
                    temp_dst.path().to_str().unwrap(),
                ])
                .output()?;

            let svg = std::fs::read_to_string(temp_dst.path())?;

            content = format!(
                "{}<div class=\"typst-wrapper\">{}</div>{}",
                pre_content, svg, post_content
            );
        }

        Ok(content)
    }
}
