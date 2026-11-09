# Rune Markdown scratchpad

A small native Rust prototype for editing Markdown in place. The window opens in
**Live Preview**: one scrolling document where the block you click becomes
editable in place, keeping its rendered look. A toolbar toggle switches between
three views:

- **Live Preview** (default): rendered blocks; the active block stays styled and
  shows only the Markdown syntax the caret reaches.
- **Source**: the whole document as plain source, in the same scroll container.
- **Split**: the source on the left and a selectable rendered preview on the
  right, each scrolling on its own.

Ctrl+E (⌘E on macOS) switches between Live Preview and Source, returning to the
last source view chosen (Source or Split). The neutral theme follows the
desktop's light/dark setting.

**Zoom** scales the whole window, toolbar included, from 50% to 300%:
Ctrl/⌘ + (or =) zooms in, Ctrl/⌘ − zooms out and Ctrl/⌘ 0 resets. The control
at the right of the toolbar does the same; press the percentage to reset. Text
reflows to the zoomed width and stays sharp. `src/zoom.rs` lays the window out
in `window / zoom` points and draws it with Rune's `El::scale` from the top-left
corner. Zoom doesn't touch the document, caret, history or view mode, and isn't
saved.

The sample is embedded at build time. Edit or paste over it; the preview reads
the same editor buffer on every edit. Nothing is saved when the app closes.
The Source view is plain monospace text with wrapping, clipboard operations,
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
document bytes and history, including pending redo steps. Source mode has no
active range; returning to Live Preview opens the region at the caret.

`src/markdown.rs` derives block kinds and UTF-8 byte ranges from Rune Kit's cached
full-document parse. Each top-level list or quote is one block, including nested
content. Metadata refreshes on buffer changes, including undo/redo, and is reused
for selection and mode changes. Edits, undo, and redo rebase the active range;
new blocks created inside it remain open, and parsed containers that merge with
it expand the range. Losing focus closes it. An active composition defers every
region and mode change until it commits or cancels.

The earlier split layout remains as a third view, with the source moved to the
left.

## Views and scrolling

`src/app.rs` builds the window: the toolbar, then either one `document-scroll`
container (Live Preview and Source) or the split panes. `SourceLayout` in
`State` records whether Source mode shows the source alone or split; the mode
itself stays `LivePreview` or `Source`.

- **Switching** never changes the text, caret, focus or history. Going back to
  Live Preview opens the block at the caret; a selection spanning several blocks
  collapses to its focus end. Switches wait for an open composition.
- **Toggle segments** keep the editor focused when pressed, so an open block
  stays open. They aren't keyboard-focusable; use Ctrl/Cmd+E from the keyboard.
- **Select All** in Live Preview, with or without an open block, switches to
  Source and selects the document.
- **Scroll stability.** Opening a block by clicking keeps its caret line under
  the pointer. Switching views and moving into an adjacent block with the arrows
  keep the caret line at the same height on screen. These use the vendored
  `Tx::keep_caret_at_press` and `Tx::keep_caret_on_screen`, and land without
  transition animations.

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

## Live Preview interactions

`editor::document` renders Live Preview from the full parse, with the active region
replaced by the shared input (`editor::source_input`). `State` handles each
interaction without creating per-block buffers or history:

- **Clicks.** Clicking rendered text, a link, a task marker or a blank line
  activates its region, reveals its Markdown and puts the caret at the mapped
  source position. Links do not open and tasks do not toggle.
- **Editing.** Typing, Enter, paste and deletion edit the shared buffer at once.
  Enter always inserts a newline. The input shows the region without its trailing
  line breaks, so it has no empty last line. A caret just after a region's final
  newline belongs to the next region: Enter at the end of a block renders the block
  and continues on the new blank line.
- **Arrows.** Up on the first line, Down on the last, Left at the start or Right at
  the end moves into the adjacent region, which then activates. Up and Down keep the
  caret's place on screen: it lands at the same x in the next region's styled line
  (a heading's larger type, hidden syntax), and keeps that column through blank
  lines. Home and End go to the ends of the displayed line. Shift-selection stays
  within the block.
- **Source switches.** Shift-navigation beyond the block, Select All and deletions
  across its boundary (Backspace at its start, Delete at its end) switch to Source
  mode, then apply once to the whole document.
- **Undo and redo** stay document-wide. If the restored selection is outside the
  active region, the mode switches to Source.
- **Composition.** While an input method composes, keys stay within the block and
  clicks wait. A deferred click applies after the composition commits (rebased past
  the committed text) or cancels.

### Styled editing and syntax reveal

The active region keeps its blocks' look while edited: heading type, inline bold,
italic, code chips, links and strikethrough, highlighted code inside its frame,
list marker columns with hanging indents, and quote rules. `src/reveal.rs` decides
which syntax shows for the caret or selection; everything else stays hidden.
Revealing or hiding changes no text, selection or history.

- Rune's `SourceProjection` (`El::source_projection`) maps the bounded source
  range to what the field draws: hidden syntax, decoded entities, generated text
  (inline-code padding) and per-line layout (`LineBox`: indents, quote rules,
  gutter bullets/numbers/task boxes, code frames). Layout, clicks, drag selection,
  caret and selection geometry, Left/Right, Up/Down and accessibility all go
  through it. Where hidden bytes collapse, a click lands on the side the syntax
  prefers (an opening delimiter's content side); Left/Right stop on the near side,
  which reveals the syntax before the caret enters it.
- Rune Kit's `markdown::editable` builds the projection and spans from the same
  cached parse and style rules as the renderer, including the fence language and
  highlighter. A lone heading or code block uses its own base type and frame.
- `State::editor` stays the only buffer, selection, composition and history. The
  projection is rebuilt on every render from the reveal policy; composition keeps
  the caret, so the syntax around it stays as it was until it commits or cancels.

- Revealing is recomputed on every render from the caret and selection: clicks,
  arrows, Home/End, drags, Shift-selection, edits, undo/redo and a finished
  composition. Moving the caret never reparses the Markdown or reruns the code
  highlighter; both are cached by source text (`tests/reveal_navigation.rs` checks
  that the cached results are reused).
- Navigation follows the displayed lines (see **Arrows** above). Moving between
  regions, Rune moves the caret to the line just beyond the region, then places it
  at the original x in the next region's own layout, repeating while landing reveals
  or hides syntax on that line.
- When reveal reshapes the region (a code fence appearing above the caret, a marker
  taking space), keys, clicks and drags inside it keep the caret where the previous
  layout drew it, scrolling the document to compensate. Text that genuinely reflows
  sideways is not held. A drag keeps its source anchor while the layout changes.
- Breaking syntax (deleting a delimiter, an unclosed fence) reparses the document:
  the broken construct shows as editable source and other blocks keep their style.

Limits: vertical spacing inside a multi-block region (list item gaps, paragraph
margins in quotes) is not reproduced, so an activated list or quote is slightly
tighter than rendered. Soft line breaks inside a paragraph show as line breaks.
Nested code in lists or quotes is framed but not highlighted. A list's gutter task
box is drawn as a plain box. Tables, rules and diagrams still show their whole
source when active. A list's source range can run on through reference definitions
that follow it, so these show as source while that list is open. Up and Down inside
one region don't keep a column across a short line; only moves between regions do.

Rune reports keys that would leave the bounded input through
`El::on_source_range_exit`, before they apply. Rune decides what would leave the
range using the field's layout; the app decides what happens next.

## Checks

```sh
cargo fmt --check
CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 cargo check --offline --locked --all-targets
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_INCREMENTAL=false cargo test --offline --locked
CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 cargo build --offline --locked
mkdir -p artifacts
target/debug/rune-markdown-prototype --theme light --snap artifacts/light.png --tree artifacts/light.json
target/debug/rune-markdown-prototype --scene fixture-code --theme dark --size 900x1700 --snap artifacts/code.png
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
Live Preview tests drive the same document view through clicks on rendered text,
links and task markers; typing, Enter, paste and deletion; adjacent-block arrows
with column retention; Source switches for Shift-navigation, Select All and boundary
deletions; undo/redo; and deferred clicks around composition commit and cancel.
They do not substitute for native input checks.

Projection tests (`tests/projection.rs`) cover hidden ranges, collapsed-boundary
sides, replacements, decorations, caret stops and UTF-8. Editable-presentation tests
(`tests/editable_presentation.rs`) cover heading metrics and marker reveal, arrow
reveal without history entries, pointer hits through hidden syntax, wrapped
navigation, composition inside revealed syntax, accessibility, entities and
escapes, code fences in their frame, and list and quote markers.

The `--scene` names (`src/scenes.rs`) open `tests/fixtures/live_preview.md` with
the caret on one construct each: `fixture` (unfocused), `fixture-heading`,
`fixture-inline`, `fixture-link`, `fixture-code`, `fixture-quote`,
`fixture-nested-quote`, `fixture-list`, `fixture-ordered`, `fixture-task` and
`fixture-list-code`.

Reveal-navigation tests (`tests/reveal_navigation.rs`) drive the full window:
Home/End on wrapped and list lines, Up/Down between styled regions and through blank
lines, caret holds when a fence reveals above the caret and when clicking inside an
open region, drag and Shift selections with source clipboard, deleting a revealed
delimiter with undo/redo, every reveal rule, and parse/highlight reuse. Each checks
that reveal adds no history.

Mode tests (`tests/modes.rs`) drive the full window: the Live Preview default and
toggle, Ctrl/Cmd+E with and without focus, history across switches, selection
collapse, Select All, the Split layout's left-hand source, and caret heights
across clicks, switches and adjacent-block moves.

For the window smoke test:

- Click a paragraph halfway down a long document; confirm the caret appears under
  the pointer and nothing jumps. Press Ctrl+E twice; the caret stays put.
- In Split, type in the middle of a paragraph; confirm immediate preview updates
  without losing the caret. Test multiline paste and Ctrl+Z / Ctrl+Shift+Z.
- Select all source and delete it; confirm the preview becomes empty. Type again.
- Paste enough paragraphs to overflow both panes. Scroll each independently and
  check the editor follows its caret while typing near the end.
- Resize the window; confirm equal pane widths, wrapped source, and usable scroll
  regions. Try a wide table and horizontal scrolling in each view.
- Drag across preview paragraphs and table cells and copy the rendered text.
  Return to the source and continue editing.
- Check light/dark screenshots and change the desktop appearance while running.

### Validation in the implementation environment

- Formatting and offline compilation against the original pinned dependency and
  the unmodified vendored copy: passed.
- All four original runtime tests: passed offline before and after vendoring.
- Native window, screenshots, and manual input checks have not been repeated for
  these state, parser and interaction changes.

## Attribution

Rune is vendored under the MIT license; see [vendor/rune/LICENSE](vendor/rune/LICENSE).
The sample Markdown is original to this prototype.
