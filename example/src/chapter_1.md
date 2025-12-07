# mdbook-typst-math example

Here is some simple examples showing how `mdbook-typst-math` renders math blocks with
`typst`.

---


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
