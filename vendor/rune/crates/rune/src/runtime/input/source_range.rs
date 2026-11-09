//! Keys that would leave a bounded input (`El::source_range`), offered to the app first.
use super::*;

impl<S: 'static> Runtime<S> {
    /// Offers a bounded field's key that would act beyond its range to `on_source_range_exit`,
    /// before it applies. True when the app chose to apply it to the whole buffer.
    pub(super) fn offer_source_range_exit(&mut self, ev: &KeyEvent, layout: Option<&Layout<Ink>>, tx: &mut Tx) -> bool {
        let Some((hook, access, range, single_line)) = self.focused_input().and_then(|i| {
            Some((i.range_exit.clone()?, i.access.clone()?, i.edit.input_source_range()?, i.single_line))
        }) else {
            return false;
        };
        let exit = {
            // The classifier copies the model only for keys that can leave the range.
            let edit = access(states!(self));
            let previous = edit.input_source_range();
            edit.set_single_line(single_line);
            edit.set_input_range(Some(range));
            let exit = edit.source_range_exit(ev, layout);
            edit.set_input_range(previous);
            exit
        };
        let Some(exit) = exit else { return false };
        log_event!(self, "source range exit {exit:?}");
        hook(states!(self), exit, tx)
    }

    /// The focused field's layout for the whole model text at its current width, for a key a
    /// bounded field applies unbounded: vertical movement then follows the full document.
    pub(super) fn focused_unbounded_layout(&mut self) -> Option<Rc<Layout<Ink>>> {
        let path = self.focused.clone()?;
        let access = self.focused_input()?.access.clone()?;
        let mut edit = access(states!(self)).clone();
        edit.set_input_range(None);
        let s = self.env.scale;
        let n = self.node(&path)?;
        let (mut spec, w) = (n.text.clone()?, n.target[2] - n.target[0] - n.inset[0] - n.inset[2]);
        spec.replace_content(edit.display().0);
        // The bounded field's styling is indexed by its range; plain text keeps line metrics.
        spec.spans.clear();
        spec.strikes.clear();
        spec.blocks.clear();
        spec.soft_breaks.clear();
        Some(self.text.layout_for_spec(&spec, s, spec.wrap.then_some(w * s), line_alignment(&spec)))
    }
}
