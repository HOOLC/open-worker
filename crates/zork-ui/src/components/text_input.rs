//! Native multiline prompt input for zork-gui.
//!
//! The input-handler and element structure are adapted from the Apache-2.0
//! `gpui-unofficial` input example. Its behavior contract is also informed by
//! Longbridge `gpui-component` Input/Textarea (Apache-2.0), but this component
//! deliberately stays on zork-gui's existing GPUI runtime and visual tokens.

use std::{ops::Range, sync::Arc};

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgba, size, App, Bounds, ClipboardItem,
    Context, CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, LineLayout, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, SharedString, Style, TextRun,
    UTF16Selection, UnderlineStyle, Window, WrappedLine,
};
use unicode_segmentation::UnicodeSegmentation;

actions!(
    composer_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        Submit,
        InsertNewline,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
    ]
);

/// Register bindings only while a `ComposerInput` key context is active.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("ComposerInput")),
        KeyBinding::new("delete", Delete, Some("ComposerInput")),
        KeyBinding::new("left", Left, Some("ComposerInput")),
        KeyBinding::new("right", Right, Some("ComposerInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("ComposerInput")),
        KeyBinding::new("shift-right", SelectRight, Some("ComposerInput")),
        KeyBinding::new("home", Home, Some("ComposerInput")),
        KeyBinding::new("end", End, Some("ComposerInput")),
        KeyBinding::new("shift-home", SelectHome, Some("ComposerInput")),
        KeyBinding::new("shift-end", SelectEnd, Some("ComposerInput")),
        KeyBinding::new("enter", Submit, Some("ComposerInput")),
        KeyBinding::new("shift-enter", InsertNewline, Some("ComposerInput")),
        KeyBinding::new(
            "ctrl-cmd-space",
            ShowCharacterPalette,
            Some("ComposerInput"),
        ),
    ]);

    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-a", SelectAll, Some("ComposerInput")),
        KeyBinding::new("cmd-v", Paste, Some("ComposerInput")),
        KeyBinding::new("cmd-c", Copy, Some("ComposerInput")),
        KeyBinding::new("cmd-x", Cut, Some("ComposerInput")),
        KeyBinding::new("cmd-left", Home, Some("ComposerInput")),
        KeyBinding::new("cmd-right", End, Some("ComposerInput")),
        KeyBinding::new("cmd-shift-left", SelectHome, Some("ComposerInput")),
        KeyBinding::new("cmd-shift-right", SelectEnd, Some("ComposerInput")),
    ]);

    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-a", SelectAll, Some("ComposerInput")),
        KeyBinding::new("ctrl-v", Paste, Some("ComposerInput")),
        KeyBinding::new("ctrl-c", Copy, Some("ComposerInput")),
        KeyBinding::new("ctrl-x", Cut, Some("ComposerInput")),
        KeyBinding::new("ctrl-home", Home, Some("ComposerInput")),
        KeyBinding::new("ctrl-end", End, Some("ComposerInput")),
        KeyBinding::new("ctrl-shift-home", SelectHome, Some("ComposerInput")),
        KeyBinding::new("ctrl-shift-end", SelectEnd, Some("ComposerInput")),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOutcome {
    Unchanged,
    Changed,
    Submit,
}

/// A platform-independent UTF-8 edit model used by the GPUI entity and unit
/// regressions. Byte selections are always clamped to character boundaries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBuffer {
    value: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
}

impl TextBuffer {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let end = value.len();
        Self {
            value,
            selected_range: end..end,
            selection_reversed: false,
            marked_range: None,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn selection(&self) -> Range<usize> {
        self.selected_range.clone()
    }

    pub fn selection_reversed(&self) -> bool {
        self.selection_reversed
    }

    pub fn marked_range(&self) -> Option<Range<usize>> {
        self.marked_range.clone()
    }

    pub fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn anchor(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        }
    }

    fn clamp_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.value.len());
        while offset > 0 && !self.value.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    pub fn set_selection(&mut self, range: Range<usize>) {
        let start = self.clamp_boundary(range.start);
        let end = self.clamp_boundary(range.end);
        self.selected_range = start.min(end)..start.max(end);
        self.selection_reversed = start > end;
    }

    pub fn move_to(&mut self, offset: usize) {
        let offset = self.clamp_boundary(offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
    }

    pub fn select_to(&mut self, offset: usize) {
        let anchor = self.anchor();
        let offset = self.clamp_boundary(offset);
        self.selected_range = anchor.min(offset)..anchor.max(offset);
        self.selection_reversed = offset < anchor;
    }

    pub fn move_left(&mut self, extend: bool) -> EditOutcome {
        let target = if extend || self.selected_range.is_empty() {
            previous_boundary(&self.value, self.cursor())
        } else {
            self.selected_range.start
        };
        if extend {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
        EditOutcome::Unchanged
    }

    pub fn move_right(&mut self, extend: bool) -> EditOutcome {
        let target = if extend || self.selected_range.is_empty() {
            next_boundary(&self.value, self.cursor())
        } else {
            self.selected_range.end
        };
        if extend {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
        EditOutcome::Unchanged
    }

    pub fn move_home(&mut self, extend: bool) -> EditOutcome {
        if extend {
            self.select_to(0);
        } else {
            self.move_to(0);
        }
        EditOutcome::Unchanged
    }

    pub fn move_end(&mut self, extend: bool) -> EditOutcome {
        let end = self.value.len();
        if extend {
            self.select_to(end);
        } else {
            self.move_to(end);
        }
        EditOutcome::Unchanged
    }

    pub fn delete_backward(&mut self) -> EditOutcome {
        if self.selected_range.is_empty() {
            let cursor = self.cursor();
            let previous = previous_boundary(&self.value, cursor);
            if previous == cursor {
                return EditOutcome::Unchanged;
            }
            self.selected_range = previous..cursor;
        }
        self.replace_selection("")
    }

    pub fn delete_forward(&mut self) -> EditOutcome {
        if self.selected_range.is_empty() {
            let cursor = self.cursor();
            let next = next_boundary(&self.value, cursor);
            if next == cursor {
                return EditOutcome::Unchanged;
            }
            self.selected_range = cursor..next;
        }
        self.replace_selection("")
    }

    pub fn select_all(&mut self) {
        self.selected_range = 0..self.value.len();
        self.selection_reversed = false;
    }

    pub fn insert(&mut self, text: &str) -> EditOutcome {
        self.replace_selection(text)
    }

    pub fn replace_selection(&mut self, text: &str) -> EditOutcome {
        let range = self
            .marked_range
            .clone()
            .unwrap_or_else(|| self.selected_range.clone());
        self.replace_byte_range(range, text, false, None)
    }

    pub fn handle_enter(&mut self, shift: bool) -> EditOutcome {
        if shift {
            self.replace_selection("\n")
        } else {
            EditOutcome::Submit
        }
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        let end = self.value.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
    }

    pub fn clear(&mut self) {
        self.set_value(String::new());
    }

    pub fn selection_utf16(&self) -> Range<usize> {
        range_to_utf16(&self.value, &self.selected_range)
    }

    pub fn set_selection_utf16(&mut self, range: Range<usize>) {
        self.set_selection(range_from_utf16(&self.value, &range));
    }

    pub fn marked_range_utf16(&self) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| range_to_utf16(&self.value, range))
    }

    pub fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        range_from_utf16(&self.value, range)
    }

    pub fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        range_to_utf16(&self.value, range)
    }

    pub fn replace_utf16_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
    ) -> EditOutcome {
        let range = range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&self.value, range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.replace_byte_range(range, text, false, None)
    }

    pub fn replace_and_mark_utf16_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_in_insert_utf16: Option<Range<usize>>,
    ) -> EditOutcome {
        let range = range_utf16
            .as_ref()
            .map(|range| range_from_utf16(&self.value, range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.replace_byte_range(range, text, true, selected_in_insert_utf16)
    }

    pub fn unmark(&mut self) {
        self.marked_range = None;
    }

    fn replace_byte_range(
        &mut self,
        range: Range<usize>,
        text: &str,
        mark: bool,
        selected_in_insert_utf16: Option<Range<usize>>,
    ) -> EditOutcome {
        let start = self.clamp_boundary(range.start);
        let end = self.clamp_boundary(range.end).max(start);
        if start == end && text.is_empty() {
            self.marked_range = None;
            return EditOutcome::Unchanged;
        }

        self.value.replace_range(start..end, text);
        self.marked_range = (mark && !text.is_empty()).then_some(start..start + text.len());
        if let Some(relative_utf16) = selected_in_insert_utf16 {
            let relative = range_from_utf16(text, &relative_utf16);
            self.selected_range = start + relative.start..start + relative.end;
        } else {
            let cursor = start + text.len();
            self.selected_range = cursor..cursor;
        }
        self.selection_reversed = false;
        EditOutcome::Changed
    }
}

fn previous_boundary(content: &str, offset: usize) -> usize {
    content
        .grapheme_indices(true)
        .rev()
        .find_map(|(index, _)| (index < offset).then_some(index))
        .unwrap_or(0)
}

fn next_boundary(content: &str, offset: usize) -> usize {
    content
        .grapheme_indices(true)
        .find_map(|(index, _)| (index > offset).then_some(index))
        .unwrap_or(content.len())
}

fn offset_from_utf16(content: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for character in content.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += character.len_utf16();
        utf8_offset += character.len_utf8();
    }
    utf8_offset
}

fn offset_to_utf16(content: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;
    for character in content.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += character.len_utf8();
        utf16_offset += character.len_utf16();
    }
    utf16_offset
}

fn range_to_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
    offset_to_utf16(content, range.start)..offset_to_utf16(content, range.end)
}

fn range_from_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
    offset_from_utf16(content, range.start)..offset_from_utf16(content, range.end)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComposerSubmit;
/// Non-text clipboard payload; storage and delivery belong to the host.
pub struct ComposerFilesPasted(pub ClipboardItem);
/// Emitted only for text edits; selection and cursor changes remain local.
pub struct ComposerEdited;
/// Text presentation changed, including an uncommitted IME composition.
pub struct ComposerLayoutChanged;

#[derive(Clone)]
struct VisualRow {
    text_start: usize,
    local_start: usize,
    local_end: usize,
    y: Pixels,
    x_offset: Pixels,
    layout: Arc<LineLayout>,
}

impl VisualRow {
    fn global_start(&self) -> usize {
        self.text_start + self.local_start
    }

    fn global_end(&self) -> usize {
        self.text_start + self.local_end
    }

    fn x_for_global_index(&self, index: usize) -> Pixels {
        let local = index
            .saturating_sub(self.text_start)
            .clamp(self.local_start, self.local_end);
        self.layout.x_for_index(local) - self.x_offset
    }
}

/// One bullet per grapheme, with explicit source/display byte mappings.
/// The edit buffer and IME keep source offsets; only painted geometry is masked.
#[derive(Clone, Debug)]
struct MaskOffsets(Vec<(usize, usize)>);
impl MaskOffsets {
    fn new(value: &str) -> (String, Self) {
        let mut text = String::new();
        let mut offsets = Vec::new();
        for (index, _) in value.grapheme_indices(true) {
            offsets.push((index, text.len()));
            text.push('•');
        }
        offsets.push((value.len(), text.len()));
        (text, Self(offsets))
    }
    fn display(&self, source: usize) -> usize {
        self.0
            .iter()
            .rev()
            .find(|(s, _)| *s <= source)
            .map(|(_, d)| *d)
            .unwrap_or(0)
    }
    fn source(&self, display: usize) -> usize {
        self.0
            .iter()
            .rev()
            .find(|(_, d)| *d <= display)
            .map(|(s, _)| *s)
            .unwrap_or(0)
    }
}

#[derive(Clone)]
struct LayoutSnapshot {
    bounds: Bounds<Pixels>,
    line_height: Pixels,
    scroll_y: Pixels,
    max_scroll_y: Pixels,
    scroll_x: Pixels,
    rows: Vec<VisualRow>,
    mask: Option<MaskOffsets>,
}

pub struct ComposerInput {
    secret: bool,
    single_line: bool,
    focus_handle: FocusHandle,
    placeholder: SharedString,
    buffer: TextBuffer,
    last_layout: Option<LayoutSnapshot>,
    manual_scroll: Option<(usize, Pixels)>,
    is_selecting: bool,
}

impl ComposerInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            secret: false,
            single_line: false,
            focus_handle: cx.focus_handle(),
            placeholder: placeholder.into(),
            buffer: TextBuffer::new(String::new()),
            last_layout: None,
            manual_scroll: None,
            is_selecting: false,
        }
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let placeholder = placeholder.into();
        if self.placeholder != placeholder {
            self.placeholder = placeholder;
            self.last_layout = None;
            self.manual_scroll = None;
            cx.notify();
        }
    }
    /// Address fields keep a single visual row and scroll the caret into view.
    /// Multiline composers retain their existing wrapping and input behavior.
    pub fn single_line(mut self) -> Self {
        self.single_line = true;
        self
    }

    pub fn value(&self) -> &str {
        self.buffer.value()
    }

    pub fn set_secret(&mut self, secret: bool, cx: &mut Context<Self>) {
        self.secret = secret;
        self.last_layout = None;
        self.manual_scroll = None;
        cx.notify();
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        let value = value.into();
        if self.buffer.value() == value {
            return;
        }
        self.buffer.set_value(value);
        self.last_layout = None;
        self.manual_scroll = None;
        cx.emit(ComposerLayoutChanged);
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if !self.buffer.value().is_empty() {
            self.buffer.clear();
            self.changed(cx);
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.last_layout = None;
        self.manual_scroll = None;
        cx.emit(ComposerEdited);
        cx.emit(ComposerLayoutChanged);
        cx.notify();
    }

    fn index_for_mouse_position(&self, position: gpui::Point<Pixels>) -> usize {
        let Some(layout) = self.last_layout.as_ref() else {
            return 0;
        };
        let local_y = position.y - layout.bounds.top() + layout.scroll_y;
        let row = if local_y <= Pixels::ZERO {
            layout.rows.first()
        } else {
            layout
                .rows
                .iter()
                .find(|row| local_y < row.y + layout.line_height)
                .or_else(|| layout.rows.last())
        };
        let Some(row) = row else {
            return 0;
        };
        let x = position.x - layout.bounds.left() + row.x_offset;
        let local = row
            .layout
            .closest_index_for_x(x)
            .clamp(row.local_start, row.local_end);
        let display = row.text_start + local;
        layout
            .mask
            .as_ref()
            .map(|m| m.source(display))
            .unwrap_or(display)
    }

    fn point_for_offset(&self, offset: usize) -> Option<gpui::Point<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let offset = layout
            .mask
            .as_ref()
            .map(|m| m.display(offset))
            .unwrap_or(offset);
        let row = layout
            .rows
            .iter()
            .rev()
            .find(|row| offset >= row.global_start() && offset <= row.global_end())
            .or_else(|| layout.rows.last())?;
        Some(point(
            layout.bounds.left() + row.x_for_global_index(offset),
            layout.bounds.top() + row.y - layout.scroll_y,
        ))
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.delete_backward() == EditOutcome::Changed {
            self.changed(cx);
        } else {
            window.play_system_bell();
        }
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.delete_forward() == EditOutcome::Changed {
            self.changed(cx);
        } else {
            window.play_system_bell();
        }
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left(false);
        cx.notify();
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right(false);
        cx.notify();
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left(true);
        cx.notify();
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right(true);
        cx.notify();
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_all();
        cx.notify();
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_home(false);
        cx.notify();
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_end(false);
        cx.notify();
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_home(true);
        cx.notify();
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_end(true);
        cx.notify();
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(ComposerSubmit);
    }

    fn insert_newline(&mut self, _: &InsertNewline, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.handle_enter(true);
        self.changed(cx);
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            if item.entries().iter().any(|entry| {
                matches!(
                    entry,
                    gpui::ClipboardEntry::Image(_) | gpui::ClipboardEntry::ExternalPaths(_)
                )
            }) {
                cx.emit(ComposerFilesPasted(item));
            } else if let Some(text) = item.text() {
                self.buffer.insert(&text);
                self.changed(cx);
            }
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.buffer.selection();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.buffer.value()[range].to_owned(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let range = self.buffer.selection();
        if !range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.buffer.value()[range].to_owned(),
            ));
            self.buffer.replace_selection("");
            self.changed(cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.buffer.select_to(offset);
        } else {
            self.buffer.move_to(offset);
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let offset = self.index_for_mouse_position(event.position);
            self.buffer.select_to(offset);
            cx.notify();
        }
    }
}

impl EventEmitter<ComposerSubmit> for ComposerInput {}
impl EventEmitter<ComposerFilesPasted> for ComposerInput {}
impl EventEmitter<ComposerEdited> for ComposerInput {}
impl EventEmitter<ComposerLayoutChanged> for ComposerInput {}

impl Focusable for ComposerInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for ComposerInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.buffer.range_from_utf16(&range_utf16);
        actual_range.replace(self.buffer.range_to_utf16(&range));
        Some(self.buffer.value()[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.selection_utf16(),
            reversed: self.buffer.selection_reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.buffer.marked_range_utf16()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let was_marked = self.buffer.marked_range.is_some();
        self.buffer.unmark();
        if was_marked {
            cx.emit(ComposerEdited);
        }
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.buffer.replace_utf16_range(range_utf16, new_text) == EditOutcome::Changed {
            self.changed(cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .buffer
            .replace_and_mark_utf16_range(range_utf16, new_text, new_selected_range_utf16)
            == EditOutcome::Changed
        {
            self.last_layout = None;
            self.manual_scroll = None;
            cx.emit(ComposerLayoutChanged);
            cx.notify();
        }
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.buffer.range_from_utf16(&range_utf16);
        let start = self.point_for_offset(range.start)?;
        let end = self.point_for_offset(range.end)?;
        let same_row = start.y == end.y;
        let left = if same_row {
            start.x.min(end.x)
        } else {
            bounds.left()
        };
        let right = if same_row {
            (start.x.max(end.x) + px(1.)).min(bounds.right())
        } else {
            bounds.right()
        };
        Some(Bounds::from_corners(
            point(
                left.max(bounds.left()),
                start.y.min(end.y).max(bounds.top()),
            ),
            point(
                right.max(left + px(1.)),
                (start.y.max(end.y) + self.last_layout.as_ref()?.line_height).min(bounds.bottom()),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let index = self.index_for_mouse_position(point);
        Some(offset_to_utf16(self.buffer.value(), index))
    }
}

struct PaintedLine {
    line: WrappedLine,
    y: Pixels,
}

struct ComposerTextElement {
    input: Entity<ComposerInput>,
}

struct PrepaintState {
    lines: Vec<PaintedLine>,
    selection: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
    snapshot: Option<LayoutSnapshot>,
}

impl IntoElement for ComposerTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ComposerTextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let single_line = input.single_line;
        let (content, mask) = if input.secret {
            let (content, map) = MaskOffsets::new(input.buffer.value());
            (content, Some(map))
        } else {
            (input.buffer.value().to_owned(), None)
        };
        let painted_offset = |offset| mask.as_ref().map(|m| m.display(offset)).unwrap_or(offset);
        let selected = input.buffer.selection();
        let selected_range = painted_offset(selected.start)..painted_offset(selected.end);
        let marked_range = input
            .buffer
            .marked_range()
            .map(|r| painted_offset(r.start)..painted_offset(r.end));
        let cursor_offset = painted_offset(input.buffer.cursor());
        let placeholder = input.placeholder.clone();
        let focused = input.focus_handle.is_focused(window);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let wrap_width = bounds.size.width.max(px(1.));
        let placeholder_mode = content.is_empty();
        let mut display = if placeholder_mode {
            placeholder.to_string()
        } else {
            content.clone()
        };
        if single_line {
            display = display.replace(['\n', '\r'], " ");
        }

        let mut painted_lines = Vec::new();
        let mut rows = Vec::new();
        let mut y = Pixels::ZERO;
        let line_count = display.split('\n').count();
        let mut text_start = 0;

        for (line_index, line_text) in display.split('\n').enumerate() {
            let line_shared = SharedString::from(line_text.to_owned());
            let color = if placeholder_mode {
                gpui::rgb(crate::design::CUE_UI.palette.subtle).into()
            } else {
                style.color
            };
            let base = TextRun {
                len: line_text.len(),
                font: style.font(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = if !placeholder_mode {
                marked_range
                    .as_ref()
                    .and_then(|marked| {
                        let start = marked.start.max(text_start).saturating_sub(text_start);
                        let end = marked
                            .end
                            .min(text_start + line_text.len())
                            .saturating_sub(text_start);
                        (start < end).then_some((start, end))
                    })
                    .map(|(start, end)| {
                        [
                            TextRun {
                                len: start,
                                ..base.clone()
                            },
                            TextRun {
                                len: end - start,
                                underline: Some(UnderlineStyle {
                                    color: Some(color),
                                    thickness: px(1.),
                                    wavy: false,
                                }),
                                ..base.clone()
                            },
                            TextRun {
                                len: line_text.len() - end,
                                ..base.clone()
                            },
                        ]
                        .into_iter()
                        .filter(|run| run.len > 0)
                        .collect::<Vec<_>>()
                    })
                    .unwrap_or_else(|| vec![base.clone()])
            } else {
                vec![base.clone()]
            };
            let mut shaped = window
                .text_system()
                .shape_text(
                    line_shared,
                    font_size,
                    &runs,
                    (!single_line).then_some(wrap_width),
                    None,
                )
                .expect("composer text shaping must succeed");
            let line = shaped
                .pop()
                .expect("shape_text returns one layout for an explicit line");

            let mut local_start = 0;
            let mut local_ends = line
                .wrap_boundaries()
                .iter()
                .map(|boundary| line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index)
                .collect::<Vec<_>>();
            local_ends.push(line.len());
            for local_end in local_ends {
                rows.push(VisualRow {
                    text_start: if placeholder_mode { 0 } else { text_start },
                    local_start,
                    local_end,
                    y,
                    x_offset: line.unwrapped_layout.x_for_index(local_start),
                    layout: line.unwrapped_layout.clone(),
                });
                y += line_height;
                local_start = local_end;
            }
            let first_row_y = y - line.size(line_height).height;
            painted_lines.push(PaintedLine {
                line,
                y: first_row_y,
            });
            if !placeholder_mode && line_index + 1 < line_count {
                text_start += line_text.len() + 1;
            }
        }

        let scroll_x = if single_line && focused && !placeholder_mode {
            rows.first()
                .map(|row| {
                    (row.x_for_global_index(cursor_offset) - wrap_width + px(2.)).max(Pixels::ZERO)
                })
                .unwrap_or(Pixels::ZERO)
        } else {
            Pixels::ZERO
        };
        if scroll_x != Pixels::ZERO {
            for row in &mut rows {
                row.x_offset += scroll_x;
            }
        }
        let cursor_row = rows
            .iter()
            .rev()
            .find(|row| cursor_offset >= row.global_start() && cursor_offset <= row.global_end())
            .or_else(|| rows.last());
        let max_scroll = (y - bounds.size.height).max(Pixels::ZERO);
        let cursor_scroll = cursor_row
            .map(|row| (row.y + line_height - bounds.size.height).max(Pixels::ZERO))
            .unwrap_or(Pixels::ZERO)
            .min(max_scroll);
        let scroll_y = input
            .manual_scroll
            .filter(|(cursor, _)| *cursor == input.buffer.cursor())
            .map(|(_, offset)| offset.max(Pixels::ZERO).min(max_scroll))
            .unwrap_or(cursor_scroll);

        let mut selection = Vec::new();
        if !placeholder_mode && !selected_range.is_empty() {
            for row in &rows {
                let start = selected_range.start.max(row.global_start());
                let end = selected_range.end.min(row.global_end());
                if start < end {
                    selection.push(fill(
                        Bounds::from_corners(
                            point(
                                bounds.left() + row.x_for_global_index(start),
                                bounds.top() + row.y - scroll_y,
                            ),
                            point(
                                bounds.left() + row.x_for_global_index(end),
                                bounds.top() + row.y - scroll_y + line_height,
                            ),
                        ),
                        rgba(0x339CFF30),
                    ));
                }
            }
        }

        let cursor = if focused && selected_range.is_empty() {
            cursor_row.map(|row| {
                fill(
                    Bounds::new(
                        point(
                            bounds.left() + row.x_for_global_index(cursor_offset),
                            bounds.top() + row.y - scroll_y,
                        ),
                        size(px(1.5), line_height),
                    ),
                    style.color,
                )
            })
        } else {
            None
        };

        PrepaintState {
            lines: painted_lines,
            selection,
            cursor,
            snapshot: Some(LayoutSnapshot {
                bounds,
                line_height,
                scroll_y,
                max_scroll_y: max_scroll,
                scroll_x,
                rows,
                mask,
            }),
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for quad in prepaint.selection.drain(..) {
            window.paint_quad(quad);
        }
        let scroll_y = prepaint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.scroll_y)
            .unwrap_or(Pixels::ZERO);
        let line_height = window.line_height();
        let scroll_x = prepaint
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.scroll_x)
            .unwrap_or(Pixels::ZERO);
        for painted in &prepaint.lines {
            let origin = point(
                bounds.left() - scroll_x,
                bounds.top() + painted.y - scroll_y,
            );
            painted
                .line
                .paint(
                    origin,
                    line_height,
                    gpui::TextAlign::Left,
                    Some(bounds),
                    window,
                    cx,
                )
                .expect("composer text painting must succeed");
        }
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
        if let Some(snapshot) = prepaint.snapshot.take() {
            self.input.update(cx, |input, _cx| {
                if input
                    .manual_scroll
                    .is_some_and(|(cursor, _)| cursor != input.buffer.cursor())
                {
                    input.manual_scroll = None;
                }
                input.last_layout = Some(snapshot);
            });
        }
    }
}

impl Render for ComposerInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .key_context("ComposerInput")
            .track_focus(&self.focus_handle)
            .tab_stop(true)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::insert_newline))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(|input, event: &gpui::ScrollWheelEvent, _, cx| {
                if input.single_line {
                    return;
                }
                let Some(layout) = &input.last_layout else {
                    return;
                };
                if layout.max_scroll_y <= Pixels::ZERO {
                    return;
                }
                let delta = event.delta.pixel_delta(layout.line_height).y;
                let offset = (layout.scroll_y - delta)
                    .max(Pixels::ZERO)
                    .min(layout.max_scroll_y);
                input.manual_scroll = Some((input.buffer.cursor(), offset));
                cx.stop_propagation();
                cx.notify();
            }))
            .child(ComposerTextElement { input: cx.entity() })
    }
}

#[cfg(test)]
mod password_mask_tests {
    use super::*;
    #[test]
    fn password_bullets_preserve_unicode_edit_and_hit_test_offsets() {
        let value = "a密👩‍💻é";
        let (paint, map) = MaskOffsets::new(value);
        assert_eq!(paint, "••••");
        for (source, _) in value.grapheme_indices(true) {
            assert_eq!(map.source(map.display(source)), source);
        }
        assert_eq!(map.display(value.len()), paint.len());
        assert_eq!(map.source(paint.len()), value.len());
        let mut buffer = TextBuffer::new(value);
        buffer.set_selection(map.source(6)..map.source(9));
        buffer.replace_selection("X");
        assert_eq!(buffer.value(), "a密Xé");
    }
}

#[cfg(all(test, feature = "headless-bench"))]
mod edit_event_tests {
    use super::*;
    use gpui::TestAppContext;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn cursor_and_preedit_stay_local_but_ime_commit_emits_once() {
        let mut app = TestAppContext::single();
        let input = app.add_window(|_, cx| ComposerInput::new("", cx));
        let count = Rc::new(Cell::new(0));
        input
            .update(&mut app, |_, _, cx| {
                let count = count.clone();
                cx.subscribe(&cx.entity(), move |_, _, _: &ComposerEdited, _| {
                    count.set(count.get() + 1)
                })
                .detach();
            })
            .unwrap();
        input
            .update(&mut app, |v, w, cx| {
                v.set_value("AB", cx);
                v.left(&Left, w, cx);
                let selection = v.buffer.selection_utf16();
                v.set_value("AB", cx);
                assert_eq!(
                    v.buffer.selection_utf16(),
                    selection,
                    "snapshot reset the caret"
                );
                v.set_secret(true, cx);
                v.unmark_text(w, cx);
                v.replace_and_mark_text_in_range(None, "ni", Some(2..2), w, cx);
                v.replace_and_mark_text_in_range(None, "你", Some(1..1), w, cx);
            })
            .unwrap();
        app.run_until_parked();
        assert_eq!(
            count.get(),
            0,
            "presentation or preedit escaped as a business edit"
        );
        input
            .update(&mut app, |v, w, cx| v.unmark_text(w, cx))
            .unwrap();
        app.run_until_parked();
        assert_eq!(count.get(), 1);
        input
            .update(&mut app, |v, w, cx| {
                assert_eq!(v.value(), "A你B");
                v.replace_and_mark_text_in_range(None, "好", Some(1..1), w, cx);
                v.replace_text_in_range(None, "好", w, cx);
            })
            .unwrap();
        app.run_until_parked();
        assert_eq!(
            count.get(),
            2,
            "same-text replacement must commit the marked input"
        );
    }
}
