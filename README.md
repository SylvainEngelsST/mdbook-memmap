# mdbook-memmap

Native Rust memory-map and register documentation for
[mdBook](https://rust-lang.github.io/mdBook/), using reusable YAML IP definitions
and hierarchical system maps.

**Status: checkpoint 3, native rendering and mdBook integration ready for review.**
Active `memmap` inclusions now render directly in Rust. Books without inclusions
are preserved. No Python runtime, package installation, or backend command is
needed by end users.

Release-lock/provenance compatibility remains checkpoint 4. This build rejects
those settings rather than silently producing an unqualified release book.

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
No Python runtime is required for native book rendering.

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
`source-roots` is required for active inclusions and must be a nonempty array of
directories relative to the book root (absolute paths also work). Both direct
sources and transitive references must stay inside these canonical roots.
Only accessed source files are loaded; source directories must not change during
a build. Chapter paths, including symlinks, are resolved to their actual source
directory before resolving the fence's `source`.

The configuration is strict: unknown keys/types fail. Standard mdBook keys
`command`, `renderers`, `before`, `after`, and `optional = false` are accepted.
`optional = true` is rejected because validation failures must abort the build.

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
When migrating a Python-backed book, remove `backend`, `backend-args`, and
`backend-timeout-seconds`; each causes an explicit migration diagnostic. The
fences do not change. `release-lock` and `provenance` deliberately fail until
checkpoint 4; keep using the qualified release tooling for locked builds.

## Native Markdown Rendering

```rust,no_run
use mdbook_memmap::{
    fences::View,
    markdown::{render, Source},
    project::Project,
};
use std::path::PathBuf;

let root = PathBuf::from("released-maps").canonicalize()?;
let mut project = Project::new(std::slice::from_ref(&root))?;
let ip = project.load(&root.join("ips/timer.yaml"))?;
let markdown = render(Source::Ip(&ip), Some("csr"), View::Registers, "")?;
let system = project.resolve(&root.join("systems/soc.yaml"), "cpu")?;
let overview = render(Source::System(&system), None, View::Summary, "memmap-1-1")?;
# Ok::<(), anyhow::Error>(())
```

Library rendering expects validated definitions from the loader or resolver;
do not mutate inputs to violate their invariants. IP rendering can select one
map or all maps. System rendering uses a resolved address space; a local map
selector is invalid there. Detailed register views reject memory/reserved maps.

The renderer preserves authored Markdown descriptions, escapes table cells,
distinguishes implemented and allocated extents, fills reserved field gaps,
and retains partial/missing reset semantics. Table fields are sorted by bit
position while their descriptions stay in authored order. Arbitrary-precision
addresses and reset values are never truncated. Each mdBook inclusion receives
a unique `memmap-CHAPTER-FENCE` namespace, including nested chapters and repeated
views; library callers can supply their own lowercase ASCII namespace.

Fourteen checked-in outputs from the independent Python reference exporter are
compared byte-for-byte in Rust tests, including rich descriptions, memory and
reserved maps, the last 32-bit address, and addresses/resets wider than 64 bits.
The legacy `Generated by rds-memmap` comment is intentionally preserved for
compatibility; it does **not** indicate a Python/backend invocation.

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
Reset bit strings in detailed Markdown have a maximum field width of 65,536
bits; wider displayed resets fail with `[markdown-width]` rather than allocating
unbounded output or silently truncating. Summary rendering is unaffected.

The timer/subsystem/system [fixtures](tests/fixtures/) are copied compatibility
inputs from the accepted rds-designspec example. Additional rendering fixtures
exercise wide values, table escaping, and partial resets. The native tests assert
all four reference addresses, shared IP identity, and the same access/reset
semantics. No Python executable is needed to run the Rust test suite.

## Development and Packaging

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --doc
cargo doc --locked --no-deps
cargo package --locked
```

Keep `CARGO_HOME` and `CARGO_TARGET_DIR` on project storage in quota-limited
environments. After dependencies are cached, use `--offline`. The small
[example book](examples/book/) contains literal fences and active IP/system
summary/detail inclusions. Build it after putting the compiled binary on `PATH`:

```sh
cargo build --locked
PATH="$PWD/target/debug:$PATH" mdbook build examples/book
MDBOOK="$(command -v mdbook)" cargo test --locked --test book -- --ignored
```

The explicitly enabled book test requires mdBook 0.5.4. It builds twice with
only the native binary directory on the child process's `PATH`, checks identical
HTML bytes, all four system register addresses, 18 tables, 23 unique memory-map
anchors, and their links. CI runs this test as well as the Python-free Rust suite.
If using `CARGO_TARGET_DIR`, adjust the binary path in the build command.

Only maintainers regenerating the parity goldens need the Python reference:

```sh
python3 tests/reference/generate.py /path/to/qualified/bin/rds-memmap
```

Goldens were generated from the accepted rds-designspec implementation
(`897fb47`). Do not regenerate them using the renderer under test.

## License

[Mozilla Public License 2.0](LICENSE).
