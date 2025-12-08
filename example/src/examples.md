# Examples

Here are some examples demonstrating how to use `mdbook-typst-math`.

## Basic Usage with Inline and Display Math

```markdown
Pythagorean theorem states that $a^2 + b^2 = c^2$ for a right triangle.
```

Pythagorean theorem states that $a^2 + b^2 = c^2$ for a right triangle.

```markdown
Fourier transform is given by:
$$
accent(f, "^")(omega) = integral_(-infinity)^infinity f(t) e^(-i omega t) d t
$$
```

Fourier transform is given by:
$$
accent(f, "^")(omega) = integral_(-infinity)^infinity f(t) e^(-i omega t) d t
$$

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

```markdown
Maxwell's equations are given by:
$$
div vb(E) = rho / epsilon_0 \
div vb(B) = 0 \
curl vb(E) = - pdv(vb(B), t) \
curl vb(B) = mu_0 vb(J) + mu_0 epsilon_0 pdv(vb(E), t)
$$
```

Maxwell's equations are given by:
$$
div vb(E) = rho / epsilon_0 \
div vb(B) = 0 \
curl vb(E) = - pdv(vb(B), t) \
curl vb(B) = mu_0 vb(J) + mu_0 epsilon_0 pdv(vb(E), t)
$$

## Escaping the Math Mode

Actually, you can escape the math mode using `#[]` syntax.

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