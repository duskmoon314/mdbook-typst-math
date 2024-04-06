# mdbook-typst

mdbook preprocessor to add [typst](https://typst.app/) support

## Installation

```shell
cargo install --git https://github.com/duskmoon314/mdbook-typst
# OR
git clone https://github.com/duskmoon314/mdbook-typst.git
cargo build --release
```

## Usage

### Setup preprocessor

Add the following to your `book.toml`:

```toml
[preprocessor.typst]
# If installed via cargo build
command = "/path/to/mdbook-typst"
```

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

.typst-display>.typst-doc {
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
    y = f(x)
    $$
    world
```

Then preprocessor will convert it to:

```html
hello
<div class="typst-display">
    <svg class="typst-doc" ...></svg>
</div>
world
```

## TODO

- [x] Integrate `typst` in code instead of using `std::process::Commend`
  - [ ] Refactor the code to improve readability and maintainability
- [ ] Allow user to configure the preambles through `book.toml`