use super::*;
use gpui::{EntityInputHandler, UTF16Selection};
use std::ops::Range;
impl EntityInputHandler for BrowserPanel {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let units: Vec<_> = self.ime_text.encode_utf16().collect();
        let range = range.start.min(units.len())..range.end.min(units.len());
        *actual = Some(range.clone());
        String::from_utf16(&units[range]).ok()
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.ime_selection.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.ime_text.is_empty()).then(|| 0..self.ime_text.encode_utf16().count())
    }
    fn unmark_text(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.ime_text.is_empty() {
            let text = self.ime_text.clone();
            self.replace_text_in_range(None, &text, w, cx);
        }
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime_text.clear();
        self.ime_selection = 0..0;
        let Some(id) = self.active_id() else {
            return;
        };
        let host = self.host.clone();
        let text = text.to_owned();
        if let Ok(rx) = self
            .worker
            .submit_ordered(move |b| b.input(&host, &id, &text))
        {
            self.await_result(rx, |_, _, _| {}, cx);
        }
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ime_text = text.into();
        let len = text.encode_utf16().count();
        let selection = selection.unwrap_or(len..len);
        self.ime_selection = selection.clone();
        let Some(id) = self.active_id() else {
            return;
        };
        let host = self.host.clone();
        let text = text.to_owned();
        if let Ok(rx) = self.worker.submit_ordered(move |b| {
            b.composition(&host, &id, &text, selection.start, selection.end)
        }) {
            self.await_result(rx, |_, _, _| {}, cx);
        }
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(Bounds::new(self.caret, gpui::size(px(1.), px(20.))))
    }
    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.ime_selection.end)
    }
}
