# mdbook-typst-math

[![Crates.io Version](https://img.shields.io/crates/v/mdbook-typst-math)](https://crates.io/crates/mdbook-typst-math)
[![docs.rs](https://img.shields.io/docsrs/mdbook-typst-math)](https://docs.rs/mdbook-typst-math)

**mdbook-typst-math** is an [mdBook](https://github.com/rust-lang/mdBook) preprocessor that uses [Typst](https://typst.app/) to render mathematical expressions.

## Requirements

- **mdBook 0.5.x or later** - This preprocessor uses the `mdbook-preprocessor` crate which requires mdBook 0.5.x
- Rust (for building from source)

## Installation

### Cargo

You can install the latest released version from crates.io:

```shell
cargo install mdbook-typst-math
```

Or install the latest version from GitHub:

```shell
cargo install --git https://github.com/duskmoon314/mdbook-typst-math
```

Or build from source:

```shell
git clone https://github.com/duskmoon314/mdbook-typst-math.git
cd mdbook-typst-math
cargo build --release
```

### Pre-built binaries

You can download pre-built binaries from the [releases](https://github.com/duskmoon314/mdbook-typst-math/releases).

## Usage

### Setup preprocessor

Add the following to your `book.toml`:

```toml
[preprocessor.typst-math]
```

If `mdbook-typst-math` is not in your `PATH`, you need to specify its location:

```toml
[preprocessor.typst-math]
command = "path/to/mdbook-typst-math"
```

The path is usually `~/.cargo/bin/mdbook-typst-math` if you installed it using `cargo`.

### Control the style

Add css to control the style of the typst block:

```css
/* css/typst.css as an example */
.typst-inline {
  display: inline flex;
  vertical-align: bottom;
}

.typst-display {
  display: block flex;
  justify-content: center;
}

.typst-display > .typst-doc {
  transform: scale(1.5);
}
```

Add the following to your `book.toml`:

```toml
[output.html]
additional-css = ["css/typst.css"]
```

### Theme support (Dark mode)

By default, this preprocessor generates SVGs with transparent backgrounds and
replaces black text color with `currentColor`. This allows the math to adapt
to different color themes.

To make the math text color adapt to the current theme automatically, add the following CSS:

```css
/* Use the theme's foreground color for math */
.typst-doc {
  color: var(--fg);
}
```

If you prefer to set specific colors for dark themes (coal, navy, ayu), you can use:

```css
/* Set specific text color for dark themes */
html.coal .typst-doc,
html.navy .typst-doc,
html.ayu .typst-doc {
  color: #b3b3b3;
}
```

### What this preprocessor does

This preprocessor will convert all math blocks to a `<div>` with the class
`typst-inline`/`typst-display` (depends on the type of math blocks) and a
`<svg>` with the class `typst-doc` inside.

Say you have the following code block in your markdown:

```markdown
  hello
  $$
  y = f(x)
  $$
  world
```

This preprocessor will first change it to:

```diff
  hello
- $$
- y = f(x)
- $$
+ #set page(width:auto, height:auto, margin:0.5em, fill:none)
+ $ y = f(x) $
  world
```

The math content is wrapped in `typst`'s `$ ... $` to indicate it is a math block, and a preamble is added before it to set the page size and margin.

Then preprocessor will leverage `typst` to render the math block and change it to:

```html
hello
<div class="typst-display">
  <svg class="typst-doc" ...></svg>
</div>
world
```

$$
y = f(x)
$$

### Rendering Typst with fenced code blocks

In addition to math blocks, you can render arbitrary Typst content using fenced code blocks:

````markdown
```typst,render
#set text(fill: blue)
*Hello from Typst!*
```
````

This allows you to use Typst's full capabilities beyond math mode, including:
- Diagrams with packages like cetz
- Tables and layouts
- Formatted text with custom styling
- Any other Typst features

Code blocks are also rendered using the `display_preamble` (or `preamble` if not set) and wrapped in `<div class="typst-display">`.

### Using Typst Packages

This preprocessor supports Typst packages from [Typst Universe](https://typst.app/universe).
Packages are automatically downloaded and cached when first used.

To use a package like [physica](https://typst.app/universe/package/physica), add the import to your preamble:

```toml
[preprocessor.typst-math]
cache = ".typst-cache"
preamble = """
#set page(width:auto, height:auto, margin:0.5em)
#import "@preview/physica:0.9.7": *
"""
```

Then you can use the package features in your math blocks:

```markdown
The derivative is $dv(f,x)$ and the partial derivative is $pdv(f,x,y)$.

$$
grad f = vu(x) pdv(f,x) + vu(y) pdv(f,y)
$$
```

$$
grad f = vu(x) pdv(f,x) + vu(y) pdv(f,y)
$$

> **Note:** Make sure to set the `cache` option to specify where downloaded packages should be stored. You may want to add this directory to your `.gitignore`.
>
> You may also want to re-use the same cache directory as your Typst installation by setting `cache` to:
> - `$XDG_CACHE_HOME/typst/packages` or `~/.cache/typst/packages` on Linux
> - `~/Library/Caches/typst/packages` on macOS
> - `%LOCALAPPDATA%\typst\packages` on Windows

### Configuration

Currently, only following configurations are supported. Here we use an example to show how to set them:

````toml
[preprocessor.typst]

# Additional fonts to load
#
# Two types are supported: a string or an array of strings
#
# Usually, you don't need to set this since the default build of preprocessor
# will load system fonts and typst embedded fonts.
fonts = ["/path/to/FiraMath-Regular.otf"] # or "/path/to/FiraMath-Regular.otf"

# Preamble to be added before the typst code
#
# The default preamble is:
# ```
# #set page(width:auto, height:auto, margin:0.5em, fill:none)
# ```
#
# The `fill: none` makes the background transparent, which allows the
# rendered math to adapt to different color themes (light/dark mode).
# If you want a specific background color, you can set it like:
# `fill: white` or `fill: rgb("#ffffff")` for white background.
#
# NOTE: When you customize `preamble`, the default value is completely
# overwritten. If you don't specify `fill`, Typst's default (white
# background) will be used. To keep transparent background, explicitly
# set `fill: none` in your custom preamble.
preamble = """
#set page(width:auto, height:auto, margin:0.5em, fill:white)
#set text(size: 12pt)
#show math.equation: set text(font: "Fira Math")
"""

# Preamble to be added before the typst code for inline math
#
# If not set, then `preamble` will be used.
#
# Usually, this is not needed. But if you want to use different settings for
# inline math and display math, you can set this.
inline_preamble = """
#set page(width:auto, height:auto, margin:0.5em, fill:white)
#set text(size: 12pt)
#show math.equation: set text(font: "Fira Math")
"""

# Preamble to be added before the typst code for display math
#
# If not set, then `preamble` will be used.
#
# Usually, this is not needed. But if you want to use different settings for
# inline math and display math, you can set this.
display_preamble = """
#set page(width:auto, height:auto, margin:0.5em, fill:white)
#set text(size: 14pt)
#show math.equation: set text(font: "Fira Math")
"""

# Cache directory for downloaded packages
#
# If you want to use Typst packages (e.g., physica), you should set this.
# The packages will be downloaded from packages.typst.org and cached here.
cache = ".typst-cache"

# Color mode for SVG output
#
# - "auto" (default): Replace black (#000000) with `currentColor` in SVG,
#   allowing CSS to control text color for theme support (light/dark mode).
# - "static": Keep colors as-is from Typst output. Use this if you want to
#   preserve exact colors or use a fixed background color.
color_mode = "auto"

# Code block language tag for rendering Typst code blocks
#
# By default, code blocks with the language tag `typst,render` are rendered.
# You can customize this to use a different tag.
code_tag = "typst,render"
````

## Contributing

Contributions are welcome! Please open issues or pull requests with:

- Bug reports
- Feature requests
- Documentation improvements
- Any other contributions

If you use this preprocessor in your mdBook projects, please consider sharing your experience or examples.