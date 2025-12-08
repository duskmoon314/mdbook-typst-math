# mdbook-typst-math example

Here are some simple examples showing how `mdbook-typst-math` renders math blocks with
`typst`.

## Inline and Block Math


```markdown
This is an inline example $Q = rho A v + C$
```

This is an inline example $Q = rho A v + C$

---

```markdown
This is a block example
$$
Q = rho A v + C
$$
```

This is a block example
$$
Q = rho A v + C
$$

---

## Using Typst Packages

You can use Typst packages by importing them in the `preamble` configuration.
For example, to use the [physica](https://typst.app/universe/package/physica) package:

```toml
[preprocessor.typst-math]
cache = ".typst-cache"
preamble = """
#set page(width: auto, height: auto, margin: 0.5em)
#import "@preview/physica:0.9.7": *
"""
```

Then you can use physica functions in your math blocks:

```markdown
Derivative: $dv(f, x)$, Partial derivative: $pdv(f, x, y)$
```

Derivative: $dv(f, x)$, Partial derivative: $pdv(f, x, y)$

```markdown
$$
grad f = vu(x) pdv(f, x) + vu(y) pdv(f, y) + vu(z) pdv(f, z)
$$
```

$$
grad f = vu(x) pdv(f, x) + vu(y) pdv(f, y) + vu(z) pdv(f, z)
$$

## Escaping the Math Mode

Actually, you can escape the math mode thanks to Typst's `#[]` syntax.

```markdown
$$
#[
    #cetz.canvas({
        import cetz.draw: *
        circle((0, 0))
        line((-1, -1), (1, 1))
        line((-1, 1), (1, -1))
    })
]
$$
```

$$
#[
    #cetz.canvas({
        import cetz.draw: *
        circle((0, 0))
        line((-1, -1), (1, 1))
        line((-1, 1), (1, -1))
    })
]
$$

> Since the code is still wrapped in a math block, the output might not be as expected. In addition, empty lines inside `#[]` may cause issues in markdown parsing. **Use this feature with caution!**

## Typst's warnings and errors

Typst's warnings and errors will be shown in the console output when building the book.

For example, the following block will generate a warning:

```markdown
$$
#[
    #set text(font: "nonexistent-font")
]
$$
```

$$
#[
    #set text(font: "nonexistent-font")
]
$$

You will see a warning like this in the console:

```console
$ mdbook build
 INFO Book building has started
 WARN Typst: warning: unknown font family: nonexistent-font
  ┌─ main.typ:7:20
  │
7 │     #set text(font: "nonexistent-font")
  │                     ^^^^^^^^^^^^^^^^^^


 INFO Running the html backend
 INFO HTML book written to `/path/to/mdbook-typst-math/example/book`
```