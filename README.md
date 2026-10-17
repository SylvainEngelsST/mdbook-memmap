# mdbook-memmap

Native Rust memory-map and register documentation for
[mdBook](https://rust-lang.github.io/mdBook/), using reusable YAML IP definitions
and hierarchical system maps.

**Status: checkpoint 2, native model and hierarchy resolver ready for review.**
This is not yet a functional replacement for the Python-backed preprocessor.
Active `memmap` inclusions fail explicitly until native rendering is implemented.
Books without active inclusions are preserved. No Python process or backend
command is invoked.

## Scope and Review Gates

1. Standard Rust library/binary package, mdBook protocol, configuration installer,
   unchanged fence contract, tests, and packaging.
2. Native typed YAML model, strict validation, diagnostics, and hierarchy resolver.
3. Native Markdown renderer, reference parity, and direct mdBook integration.
4. Release locks/provenance, real-book acceptance, and final packaging checks.

Each checkpoint is validated and reviewed before a Conventional Commit is made
in this repository. Standalone validation/export/IP-XACT utilities remain Python
for this phase; they are not silently replaced or removed from rds-designspec.

## Installation

The qualified build toolchain is Rust 1.99.0. From a checkout:

```sh
cargo install --path . --locked
mdbook-memmap install path/to/book
```

Registry publication is not yet performed; `cargo install mdbook-memmap` will be
documented when a release is published. Maintainers can distribute the compiled
binary directly to users who do not have Cargo or network access.
No Python runtime will be required for native book rendering.

`install` edits an existing `book.toml`, preserves comments and existing
configuration, and is idempotent. Review its default root (`"."`, the book
directory) and narrow it to the released map directories needed by the book.
No JavaScript assets are required.

```toml
[preprocessor.memmap]
command = "mdbook-memmap"
source-roots = ["../released-maps"]
renderers = ["html"]
before = ["links"]
```

The native preprocessor uses the standard `mdbook-preprocessor` library like
mdbook-wavedrom. Its library exports `Memmap` implementing `Preprocessor`; the
binary supports `--help`, `--version`, `supports html`, and `install [book-dir]`.
It supports mdBook `>=0.5.4, <0.6.0` and HTML only.
Protocol stdout contains JSON only; diagnostics use stderr.
Book configuration remains scaffolded until checkpoint 3 wires native rendering.
The library `Project` already enforces canonical source roots for source loading.

## Unchanged Fence Options

````markdown
```memmap
source: ../released-maps/systems/soc.yaml
address_space: cpu
view: summary
```

```memmap
source: ../released-maps/ips/timer.yaml
map: csr
view: registers
```
````

| Property | Usage |
|----------|-------|
| `source` | Required nonempty string. YAML path relative to the containing chapter; the native library enforces canonical paths and allowed roots. |
| `view` | Required `summary` or `registers`. |
| `map` | Exact IP-local map name; required for IP inputs and mutually exclusive with `address_space`. |
| `address_space` | Exact system address-space name; required for system inputs and mutually exclusive with `map`. |

Unknown/duplicate properties, non-string values, empty selectors, and extra fence
info tokens fail. Only top-level backtick/tilde fences with info exactly `memmap`
are active. List/blockquote inclusions fail instead of corrupting Markdown.
Longer outer Markdown fences and indented code preserve literal examples.

The existing YAML source contract remains the compatibility target: schema
version 1, `rds-v1` naming, separate IP/system definitions, software access,
`hwAccess`, volatility, reset types, and reusable hierarchy. Python remains the
reference for renderer parity, not a runtime dependency or fallback.
The old `backend`, `backend-args`, and `backend-timeout-seconds` settings are not
part of the native architecture; migration diagnostics and strict configuration
will be implemented with the integrated renderer.

## Native Model and Hierarchy API

```rust,no_run
use mdbook_memmap::project::Project;
use std::path::PathBuf;

let mut project = Project::new(&[PathBuf::from("released-maps")])?;
let view = project.resolve(
    &PathBuf::from("released-maps/systems/soc.yaml"), "cpu"
)?;
for block in view.blocks {
    for register in &block.memory_map.registers {
        println!("{}: {}", register.name, &block.base + &register.offset);
    }
}
# Ok::<(), anyhow::Error>(())
```

`loader::load_definition` parses and validates one definition without traversing
references. Use `Project::load` for the complete hierarchy or `Project::resolve`
for a selected flattened system view. Sources resolve relative to the YAML file
containing the reference. Every declared space is traversed, not only the one
selected for display. The typed model retains source file/line/column/object-path
locations, descriptions, reset values/masks, and instance roles.

The strict scalar subset matches the Python authoring contract: decimal/hex
nonnegative integers, true/false booleans, and string prose. Quoted numeric text,
floating-point syntax, nulls, and booleans are not accepted as integers.
`on`/`off` and date-like strings remain text. Decimal leading zeros do not imply
octal. Binary/octal/separator integer syntax is not recognized as an integer.
Reject duplicate keys, merge keys, aliases, anchors, explicit tags, multiple
documents, non-string mapping keys, and unsupported directives; `%YAML 1.2`
is allowed.

Local checks cover naming, unknown/missing properties, unique names, widths,
field/register bounds and overlaps, register alignment, software access/effects,
hardware-write volatility, and reset references/widths/duplicates. Unspecified
resets stay unspecified; zero masks do not imply retention; masked-out value bits
are not zeroed. Memory/reserved maps remain distinct from register maps.

Hierarchy checks cover file existence, canonical allowed roots, symlink escapes,
cycles, unique VLNV-to-file identity, target existence, complete allocation fit,
overlaps including allocation tails, absolute register alignment, and ambiguous
flattened instance/map/reset names. A tail allocation does not enlarge implemented
storage; aliases remain unsupported.

A `Project` is a per-operation snapshot. Reused IP definitions share `Arc`
identity; a fresh project is required to reread edits. Failed hierarchy loads
clear the source/identity caches. `snapshots()` exposes the exact parsed bytes for
future provenance checks; file contents are not locked against concurrent edits,
so release inputs must remain immutable during a build.

Addresses, sizes, and reset values/masks use arbitrary-precision `BigUint`;
there is no silent 64-bit truncation. Widths and bit indices use checked `u64`
arithmetic. As explicit operational limits, YAML nesting and source-reference
depth are limited to 128 levels, and width/index values above `u64::MAX` fail.
These limits fail diagnostically instead of overflowing or exhausting the stack.

The [fixtures](tests/fixtures/) are copied compatibility inputs from the accepted
rds-designspec timer/subsystem/system example. The native tests assert all four
reference addresses, shared IP identity, and the same access/reset semantics.
No Python executable is needed to run the Rust test suite.

## Development and Packaging

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo doc --locked --no-deps
cargo package --locked
```

Keep `CARGO_HOME` and `CARGO_TARGET_DIR` on project storage in quota-limited
environments. After dependencies are cached, use `--offline`. The small
[example book](examples/book/) contains literal examples and can be built with
mdBook after putting the compiled binary on `PATH`.

## License

[Mozilla Public License 2.0](LICENSE).
