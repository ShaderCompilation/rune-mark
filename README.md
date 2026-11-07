# Rune Markdown scratchpad

A small native Rust prototype: rendered Markdown on the left, raw input on the
right. The panes share the window equally and scroll independently. The neutral
theme follows the desktop's light/dark setting.

The sample is embedded at build time. Edit or paste over it; the preview reads
the same editor buffer on every edit. Nothing is saved when the app closes.
The source editor is plain monospace text with wrapping, clipboard operations,
and Rune's standard undo/redo. The preview allows text selection and copying.

## Run

Use Rust **1.99.1 or newer**, with rustfmt installed. Rune is included under
`vendor/rune`. The first build needs crates.io access and a writable Cargo cache
unless the registry dependencies are already cached.

```sh
cd rune-markdown-prototype
cargo run
```

On Linux, Rune requires Wayland and xkbcommon development packages and a working
GPU driver. For example:

```sh
# Fedora
sudo dnf install libxkbcommon-devel wayland-devel

# Debian / Ubuntu
sudo apt-get install libxkbcommon-dev libwayland-dev libxkbcommon-x11-0 mesa-vulkan-drivers
```

See [Rune's Linux instructions](https://github.com/anomalyco/rune/blob/dev/crates/rune/README.md#linux)
for driver and headless setup. A software Vulkan driver can run snapshots without
a physical GPU. These commands are setup instructions; this prototype does not
install system packages.

## Dependencies and limits

`rune` and `rune-kit` use the local workspace under `vendor/rune`, imported from
`a138b97e654b8299b2a4cb518b8476b2b528c2c4`. This exact revision was taken
from [Anomaly's Hex manifest](https://github.com/anomalyco/hex/blob/dev/Cargo.toml),
which pins both crates together. The complete tracked workspace and its licenses
are preserved; Git metadata and build artifacts are excluded. Both direct Rune
dependencies and their internal workspace dependencies resolve to this copy.

The supplied `Cargo.lock` records the resolved dependency graph. Use `--locked`
for subsequent builds, and add `--offline` when registry dependencies are cached.

The preview uses [Rune's Markdown component](https://github.com/anomalyco/rune/blob/dev/crates/rune-kit/README.md#markdown)
with its own CommonMark/GFM interpretation, including tables and highlighted
code fences. Wide tables can scroll horizontally. The sample demonstrates
headings, emphasis, links, lists, quotes, tables, code, and Unicode.

This is an in-memory rendering experiment. There is no file management, autosave,
source highlighting, scroll synchronization, image loader, Mermaid, math engine,
HTML/CSS compatibility layer, or export.
Syntax outside the demonstrated subset follows Rune's behavior. Markdown
conformance, large-document performance, and platform parity have not been
measured.

## Shared editing state

`src/state.rs` owns one `TextEdit` for the document, global caret/selection, IME
composition, and undo/redo history. Separate view state records `Source` or
`LivePreview` mode and an optional active source range. Changing mode never
changes the editor; block activation only sets its caret and focus. Both preserve
document bytes and history, including pending redo steps.

`src/markdown.rs` derives block kinds and UTF-8 byte ranges from Rune Kit's cached
full-document parse. Each top-level list or quote is one block, including nested
content. Metadata refreshes on buffer changes, including undo/redo, and is reused
for selection and mode changes. A text change invalidates the active range;
retaining and rebasing an edited region belongs to the later bounded-input work.

The split view still opens the full source editor. The mode toggle,
rendered-text activation, blank-region activation, and bounded block editor are
not built yet; the state APIs are ready for them.

## Checks

```sh
cargo fmt --check
cargo check --offline --locked --all-targets
cargo test --offline --locked
cargo build --offline --locked
mkdir -p artifacts
target/debug/rune-markdown-prototype --theme light --snap artifacts/light.png --tree artifacts/light.json
target/debug/rune-markdown-prototype --theme dark --snap artifacts/dark.png --tree artifacts/dark.json
target/debug/rune-markdown-prototype --size 800x500 --snap artifacts/small.png
```

The runtime tests exercise paste-to-preview updates, clearing the preview,
retained editor focus, Unicode, and incomplete Markdown. They run the same view
and editor binding as the window, and also check current block metadata after
input. State tests cover nested containers, byte ranges, invalid offsets, and
preserving document, selection, composition, undo, and redo across view changes.
They do not substitute for native input checks.

For the window smoke test:

- Type in the middle of a paragraph; confirm immediate preview updates without
  losing the caret. Test multiline paste and Ctrl+Z / Ctrl+Shift+Z.
- Select all source and delete it; confirm the preview becomes empty. Type again.
- Paste enough paragraphs to overflow both panes. Scroll each independently and
  check the editor follows its caret while typing near the end.
- Resize the window; confirm equal pane widths, wrapped source, and usable scroll
  regions. Try a wide table and horizontal scrolling.
- Drag across preview paragraphs and table cells and copy the rendered text.
  Return to the source and continue editing.
- Check light/dark screenshots and change the desktop appearance while running.

### Validation in the implementation environment

- Formatting and offline compilation against the original pinned dependency and
  the unmodified vendored copy: passed.
- All four original runtime tests: passed offline before and after vendoring.
- Native window, screenshots, and manual input checks have not been repeated for
  these state and parser changes.

## Attribution

Rune is vendored under the MIT license; see [vendor/rune/LICENSE](vendor/rune/LICENSE).
The sample Markdown is original to this prototype.
