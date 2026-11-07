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
for selection and mode changes. Edits, undo, and redo rebase the active range;
new blocks created inside it remain open, and parsed containers that merge with
it expand the range. Moving the caret or selection outside it, or losing focus,
closes it. An active composition keeps it open until commit or cancellation.

The default screen remains the split view with a full source editor. Mode controls,
rendered-text activation, adjacent-block navigation, and switching to Source for
cross-boundary commands are not built yet.

## Source mappings and renderer hooks

Rune Kit now exposes `markdown::source_regions` and `source_region_at` alongside
`block_source_ranges`. Regions retain top-level containers, blank lines, hidden
source such as reference definitions and HTML, and a zero-length EOF insertion
position. `State::regions()` and `activate_block_at()` use these editable regions.

`markdown::rendered_texts(source)` returns text runs with `text()` and
`source_position(rendered_byte)`. Both input and output positions use UTF-8 byte
offsets. Entity spellings stay atomic, Unicode positions stay on character
boundaries, and inline-code padding maps to adjoining source boundaries.

`Markdown::on_source_click` reports a `SourceClick { source_range, position }`
using Rune's shaped text layout. It replaces link/task activation and rendered
selection while installed; blank and hidden source lines gain click targets.
Decorations without mapped text report the outer block's start.

`Markdown::replace_source_range(range, editor_view)` replaces the touched regions
with one supplied view, retaining its key beneath a `source-editor` holder.
Surrounding blocks use the complete document parse, so reference links remain
resolved even while their definition is being edited. Ranges expand to whole
regions; invalid offsets are ignored. These editing hooks settle streaming input.
The reusable `editor::source_input` uses Rune's `El::source_range(range)` in
Live Preview mode when a region is active, and the full input in Source mode.
Both bind the same `State::editor` through `on_edit`.

`El::source_range` maps global UTF-8 offsets onto a local layout for pointer and
keyboard selection, caret/IME geometry, rich decorations, clipboard operations,
browser composition context, and accessibility values and actions. Replacements
and Select All are bounded to the view; navigation may leave it. Undo and redo
remain document-wide and restore global snapshot selections.

During edit callbacks, `TextEdit::input_source_range()` supplies the exact rebased
range, including edits in repeated text. `State` retains it in view state and
expands it to cover merged parse regions. `TextEdit::rebase_source_range` provides
a UTF-8-safe common-prefix/suffix fallback for external buffer revisions. The
mapping is transient on the editor during dispatch and is absent from its history;
no block owns an editing buffer or undo stack.

## Checks

```sh
cargo fmt --check
CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 cargo check --offline --locked --all-targets
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_INCREMENTAL=false cargo test --offline --locked
CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 cargo build --offline --locked
mkdir -p artifacts
target/debug/rune-markdown-prototype --theme light --snap artifacts/light.png --tree artifacts/light.json
target/debug/rune-markdown-prototype --theme dark --snap artifacts/dark.png --tree artifacts/dark.json
target/debug/rune-markdown-prototype --size 800x500 --snap artifacts/small.png
```

Builds are memory-heavy: keep `CARGO_BUILD_JOBS` low and debug information off,
as above. A test-name filter still compiles a whole library test binary; the
application integration tests avoid the much larger Rune library test binary.

The runtime tests exercise paste-to-preview updates, clearing the preview,
retained editor focus, Unicode, and incomplete Markdown. They run the same view
and editor binding as the window, and also check current block metadata after
input. State tests cover nested containers, byte ranges, invalid offsets, and
preserving document, selection, composition, undo, and redo across view changes.
Bounded-input tests also cover local layout with global Unicode offsets, wrapped
navigation, pointer selection, clipboard, IME and accessibility, range rebasing,
block splitting/merging, and repeated-text undo/redo over the shared history.
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
