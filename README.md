# mdbook-typst-math

An mdbook preprocessor to use [typst](https://typst.app/) to render math.

## Installation

```shell
cargo install --git https://github.com/duskmoon314/mdbook-typst-math
# OR
git clone https://github.com/duskmoon314/mdbook-typst-math.git
cargo build --release
```

## Usage

### Setup preprocessor

Add the following to your `book.toml`:

```toml
[preprocessor.typst]
command = "/path/to/mdbook-typst"
```

The path is usually `~/.cargo/bin/mdbook-typst` if you installed it using `cargo`.

Other configurations see the following section: [Configuration](#configuration).

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
    $$
+   #set page(width:auto, height:auto, margin:0.5em)
+   $ y = f(x) $
-   y = f(x)
    $$
    world
```

The above is a valid `typst` code. The dollar signs `$` and whitespaces are used to let typst knows it is a math block instead of an inline math.

Then preprocessor will leverage `typst` to render the math block and change it to:

```html
hello
<div class="typst-display">
  <svg class="typst-doc" ...></svg>
</div>
world
```

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
fonts = ["Fira Math"] # or "Fira Math"

# Preamble to be added before the typst code
#
# The default preamble is:
# ```
# #set page(width:auto, height:auto, margin:0.5em)
# ```
preamble = """
#set page(width:auto, height:auto, margin:0.5em)
#set text(size: 12pt)
#show math.equation: set text(font: "Fira Math")
"""

# Preamble to be added before the typst code for inline math
#
# If not set, the `preamble` will be used.
#
# Usually, this is not needed. But if you want to use different settings for
# inline math and display math, you can set this.
inline_preamble = """
#set page(width:auto, height:auto, margin:0.5em)
#set text(size: 12pt)
#show math.equation: set text(font: "Fira Math")
"""

# Preamble to be added before the typst code for display math
#
# If not set, the `preamble` will be used.
#
# Usually, this is not needed. But if you want to use different settings for
# inline math and display math, you can set this.
display_preamble = """
#set page(width:auto, height:auto, margin:0.5em)
#set text(size: 14pt)
#show math.equation: set text(font: "Fira Math")
"""
````

## TODO

- [x] Integrate `typst` in code instead of using `std::process::Commend`
  - [ ] Refactor the code to improve readability and maintainability
- [x] Allow user to configure the preambles through `book.toml`
