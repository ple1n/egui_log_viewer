use std::collections::VecDeque;
use std::hash::Hash;
use std::ops::RangeInclusive;

use egui::{scroll_area::ScrollSource, Ui, Vec2};

const ROW_GAP: f32 = 4.0;
const ROW_CONTENT_SPACING: Vec2 = Vec2::new(8.0, 3.0);

pub trait LogRows {
    type Row;

    fn len(&self) -> usize;
    fn get(&self, index: usize) -> Option<&Self::Row>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> LogRows for [T] {
    type Row = T;

    fn len(&self) -> usize {
        <[T]>::len(self)
    }

    fn get(&self, index: usize) -> Option<&Self::Row> {
        <[T]>::get(self, index)
    }
}

impl<T> LogRows for Vec<T> {
    type Row = T;

    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn get(&self, index: usize) -> Option<&Self::Row> {
        self.as_slice().get(index)
    }
}

impl<T> LogRows for VecDeque<T> {
    type Row = T;

    fn len(&self) -> usize {
        VecDeque::len(self)
    }

    fn get(&self, index: usize) -> Option<&Self::Row> {
        VecDeque::get(self, index)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Selection {
    anchor: Option<usize>,
    end: Option<usize>,
    dragging: bool,
}

impl Selection {
    fn begin(&mut self, index: usize, extend: bool) {
        if !extend || self.anchor.is_none() {
            self.anchor = Some(index);
        }
        self.end = Some(index);
        self.dragging = true;
    }

    fn extend_to(&mut self, index: usize) {
        if self.dragging {
            self.end = Some(index);
        }
    }

    fn finish(&mut self) {
        self.dragging = false;
    }

    fn range(self) -> Option<RangeInclusive<usize>> {
        let anchor = self.anchor?;
        let end = self.end?;
        Some(anchor.min(end)..=anchor.max(end))
    }
}

#[derive(Clone, Debug, Default)]
struct HeightIndex {
    values: Vec<f32>,
    tree: Vec<f32>,
    content_heights: Vec<f32>,
    content_offsets: Vec<f32>,
}

impl HeightIndex {
    fn len(&self) -> usize {
        self.values.len()
    }

    fn total(&self) -> f32 {
        self.prefix_sum(self.len())
    }

    fn value(&self, index: usize) -> f32 {
        self.values[index]
    }

    fn content_height(&self, index: usize) -> f32 {
        self.content_heights[index]
    }

    fn content_offset(&self, index: usize) -> f32 {
        self.content_offsets[index]
    }

    fn prefix_sum(&self, end: usize) -> f32 {
        let mut end = end.min(self.len());
        let mut sum = 0.0;
        while end > 0 {
            sum += self.tree[end - 1];
            end &= end - 1;
        }
        sum
    }

    fn resize(&mut self, len: usize, initial_height: f32, initial_content_height: f32) {
        if len < self.len() {
            self.values.truncate(len);
            self.content_heights.truncate(len);
            self.content_offsets.truncate(len);
            self.rebuild();
        } else {
            while self.len() < len {
                let one_based = self.len() + 1;
                let low_bit = one_based & one_based.wrapping_neg();
                let covered_start = one_based - low_bit;
                let node_value = self.prefix_sum(one_based - 1) - self.prefix_sum(covered_start)
                    + initial_height;
                self.values.push(initial_height);
                self.tree.push(node_value);
                self.content_heights.push(initial_content_height);
                self.content_offsets.push(0.0);
            }
        }
    }

    fn set(
        &mut self,
        index: usize,
        slot_height: f32,
        content_height: f32,
        content_offset: f32,
    ) -> bool {
        let old_slot_height = self.values[index];
        let changed = (old_slot_height - slot_height).abs() >= 0.5
            || (self.content_heights[index] - content_height).abs() >= 0.5
            || (self.content_offsets[index] - content_offset).abs() >= 0.5;
        if !changed {
            return false;
        }

        let delta = slot_height - old_slot_height;
        if delta.abs() >= 0.5 {
            self.values[index] = slot_height;
            let mut node = index + 1;
            while node <= self.len() {
                self.tree[node - 1] += delta;
                node += node & node.wrapping_neg();
            }
        }
        self.content_heights[index] = content_height;
        self.content_offsets[index] = content_offset;
        true
    }

    fn row_at(&self, offset: f32) -> usize {
        let mut index = 0;
        let mut sum = 0.0;
        let mut bit = 1;
        while bit << 1 <= self.len() {
            bit <<= 1;
        }

        while bit > 0 {
            let next = index + bit;
            if next <= self.len() && sum + self.tree[next - 1] <= offset {
                index = next;
                sum += self.tree[next - 1];
            }
            bit >>= 1;
        }
        index.min(self.len().saturating_sub(1))
    }

    fn rebuild(&mut self) {
        self.tree.clone_from(&self.values);
        for node in 1..=self.len() {
            let parent = node + (node & node.wrapping_neg());
            if parent <= self.len() {
                self.tree[parent - 1] += self.tree[node - 1];
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ViewerState {
    selection: Selection,
    heights: HeightIndex,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogViewerOutput {
    pub selected_rows: Option<RangeInclusive<usize>>,
}

pub struct LogViewer;

impl LogViewer {
    /// Shows virtualized rows with row-range selection.
    ///
    /// Label text selection is disabled inside the row callback's UI by
    /// default, so drag gestures select rows rather than the text in them.
    /// The callback UI uses compact spacing (8 points horizontally, 3
    /// vertically), with a 4-point gap between rows.
    /// The row text callback supplies the plain-text representation used by
    /// the widget's Ctrl+C handling for the selected range.
    pub fn show_rows<R, IdSource>(
        ui: &mut Ui,
        id_source: IdSource,
        rows: &R,
        row_height: f32,
        max_height: f32,
        mut row_to_text: impl FnMut(&R::Row) -> String,
        mut add_row: impl FnMut(&mut Ui, usize, &R::Row, bool),
    ) -> LogViewerOutput
    where
        R: LogRows + ?Sized,
        IdSource: Hash + std::fmt::Debug,
    {
        let viewer_id = ui.make_persistent_id(id_source);
        let scroll_id = viewer_id.with("scroll");
        let mut state = ui
            .ctx()
            .data(|data| data.get_temp::<ViewerState>(viewer_id))
            .unwrap_or_default();
        let count = rows.len();
        let estimated_height = row_height.max(ui.text_style_height(&egui::TextStyle::Body));
        state
            .heights
            .resize(count, estimated_height + ROW_GAP, estimated_height);

        if state
            .selection
            .range()
            .is_some_and(|range| *range.end() >= count)
        {
            state.selection = Selection::default();
        }

        egui::ScrollArea::both()
            .id_salt(scroll_id)
            .max_height(max_height)
            .auto_shrink([false, false])
            .scroll_source(ScrollSource::SCROLL_BAR | ScrollSource::MOUSE_WHEEL)
            .show_viewport(ui, |ui, viewport| {
                ui.style_mut().interaction.selectable_labels = false;
                ui.set_height(state.heights.total());
                if count == 0 {
                    return;
                }

                let first_visible = state.heights.row_at(viewport.min.y.max(0.0));
                let last_visible = state.heights.row_at(viewport.max.y.max(viewport.min.y));
                let clip_rect = ui.clip_rect();
                let focus_response = ui.interact(
                    clip_rect,
                    viewer_id,
                    egui::Sense::focusable_noninteractive(),
                );
                let pointer = ui.input(|input| {
                    (
                        input.pointer.interact_pos(),
                        input.pointer.button_pressed(egui::PointerButton::Primary),
                        input.pointer.button_down(egui::PointerButton::Primary),
                        input.pointer.button_released(egui::PointerButton::Primary),
                        input.modifiers.shift,
                        input.unstable_dt,
                        input.pointer.delta(),
                    )
                });

                let origin_y = ui.max_rect().top();
                let pointer_row = pointer.0.and_then(|pointer_pos| {
                    if pointer_pos.x < clip_rect.left() || pointer_pos.x > clip_rect.right() {
                        return None;
                    }
                    let first_top = origin_y
                        + state.heights.prefix_sum(first_visible)
                        + state.heights.content_offset(first_visible);
                    let last_bottom = origin_y
                        + state.heights.prefix_sum(last_visible)
                        + state.heights.content_offset(last_visible)
                        + state.heights.content_height(last_visible);
                    if pointer_pos.y < first_top - ROW_GAP * 0.5
                        || pointer_pos.y > last_bottom + ROW_GAP * 0.5
                    {
                        return None;
                    }

                    (first_visible..=last_visible)
                        .map(|row_index| {
                            let row_top = origin_y
                                + state.heights.prefix_sum(row_index)
                                + state.heights.content_offset(row_index);
                            let row_bottom = row_top + state.heights.content_height(row_index);
                            let distance = if pointer_pos.y < row_top {
                                row_top - pointer_pos.y
                            } else if pointer_pos.y > row_bottom {
                                pointer_pos.y - row_bottom
                            } else {
                                0.0
                            };
                            (row_index, distance)
                        })
                        .min_by(|(_, left), (_, right)| left.total_cmp(right))
                        .map(|(row_index, _)| row_index)
                });

                if pointer.1 {
                    if let Some(row_index) = pointer_row {
                        state.selection.begin(row_index, pointer.4);
                        focus_response.request_focus();
                    }
                } else if state.selection.dragging && pointer.2 {
                    if pointer.6 != Vec2::ZERO {
                        if let Some(row_index) = pointer_row {
                            state.selection.extend_to(row_index);
                        }
                    } else if let Some(pointer_pos) = pointer.0 {
                        if pointer_pos.y < clip_rect.top() {
                            state.selection.extend_to(first_visible);
                            let distance = clip_rect.top() - pointer_pos.y;
                            let speed = (40.0 + distance * 8.0).min(1200.0);
                            ui.scroll_with_delta(Vec2::new(
                                0.0,
                                speed * pointer.5.clamp(0.0, 0.05),
                            ));
                        } else if pointer_pos.y > clip_rect.bottom() {
                            state.selection.extend_to(last_visible);
                            let distance = pointer_pos.y - clip_rect.bottom();
                            let speed = (40.0 + distance * 8.0).min(1200.0);
                            ui.scroll_with_delta(Vec2::new(
                                0.0,
                                -speed * pointer.5.clamp(0.0, 0.05),
                            ));
                        }
                    }
                }

                if pointer.3 || !pointer.2 {
                    state.selection.finish();
                }

                if let Some(selection_range) = state.selection.range() {
                    let selected_start = (*selection_range.start()).max(first_visible);
                    let selected_end = (*selection_range.end()).min(last_visible);
                    if selected_start <= selected_end {
                        let top = origin_y
                            + state.heights.prefix_sum(selected_start)
                            + state.heights.content_offset(selected_start);
                        let bottom = origin_y
                            + state.heights.prefix_sum(selected_end)
                            + state.heights.content_offset(selected_end)
                            + state.heights.content_height(selected_end);
                        let range_rect = egui::Rect::from_min_max(
                            egui::pos2(ui.max_rect().left(), top),
                            egui::pos2(ui.max_rect().right(), bottom),
                        );
                        let selection_visuals = &ui.visuals().selection;
                        let background = egui::Color32::from_rgba_unmultiplied(
                            selection_visuals.bg_fill.r(),
                            selection_visuals.bg_fill.g(),
                            selection_visuals.bg_fill.b(),
                            42,
                        );
                        let outline = egui::Color32::from_rgba_unmultiplied(
                            selection_visuals.stroke.color.r(),
                            selection_visuals.stroke.color.g(),
                            selection_visuals.stroke.color.b(),
                            90,
                        );
                        let rounding = egui::CornerRadius::same(4);
                        ui.painter().rect_filled(range_rect, rounding, background);
                        ui.painter().rect_stroke(
                            range_rect,
                            rounding,
                            egui::Stroke::new(1.0, outline),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(
                                range_rect.min,
                                egui::pos2(range_rect.left() + 3.0, range_rect.bottom()),
                            ),
                            rounding,
                            selection_visuals.stroke.color,
                        );
                    }
                }

                for row_index in first_visible..=last_visible {
                    let row_top = origin_y + state.heights.prefix_sum(row_index);
                    let content_offset = state.heights.content_offset(row_index);
                    let content_height = state.heights.content_height(row_index);
                    let row_rect = egui::Rect::from_min_size(
                        egui::pos2(ui.max_rect().left(), row_top + content_offset),
                        Vec2::new(ui.available_width(), content_height),
                    );
                    let selected = state
                        .selection
                        .range()
                        .is_some_and(|range| range.contains(&row_index));
                    if let Some(row) = rows.get(row_index) {
                        let measured_rect = ui
                            .scope_builder(egui::UiBuilder::new().max_rect(row_rect), |row_ui| {
                                row_ui.spacing_mut().item_spacing = ROW_CONTENT_SPACING;
                                add_row(row_ui, row_index, row, selected);
                                row_ui.min_rect()
                            })
                            .inner;
                        let measured_height = measured_rect.height().max(1.0);
                        let slot_height = measured_height.max(estimated_height) + ROW_GAP;
                        let content_offset = (measured_rect.top() - row_rect.top()).max(0.0);
                        if state.heights.set(
                            row_index,
                            slot_height,
                            measured_height,
                            content_offset,
                        ) {
                            ui.ctx().request_repaint();
                        }
                    }
                }
            });

        let copy_requested = ui.input(|input| input.events.contains(&egui::Event::Copy));
        let has_focus = ui.memory(|memory| memory.has_focus(viewer_id));
        if copy_requested && has_focus {
            if let Some(selected_rows) = state.selection.range() {
                let text = selected_rows
                    .filter_map(|index| rows.get(index).map(&mut row_to_text))
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.ctx().copy_text(text);
            }
        }

        ui.ctx()
            .data_mut(|data| data.insert_temp(viewer_id, state.clone()));

        LogViewerOutput {
            selected_rows: state.selection.range(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use egui::{CentralPanel, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    use super::{HeightIndex, LogViewer, Selection};

    fn run_view(
        ctx: &egui::Context,
        events: Vec<Event>,
        modifiers: Modifiers,
        row_rects: &RefCell<Vec<Rect>>,
    ) -> super::LogViewerOutput {
        run_view_with_multiline(ctx, events, modifiers, row_rects, false)
    }

    fn run_view_with_multiline(
        ctx: &egui::Context,
        events: Vec<Event>,
        modifiers: Modifiers,
        row_rects: &RefCell<Vec<Rect>>,
        multiline: bool,
    ) -> super::LogViewerOutput {
        row_rects.borrow_mut().clear();
        let mut output = super::LogViewerOutput::default();
        let mut input_events = vec![Event::ModifiersChanged(modifiers)];
        input_events.extend(events);
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events: input_events,
            ..Default::default()
        };

        ctx.run_ui(input, |ui| {
            CentralPanel::default().show(ui, |ui| {
                output = LogViewer::show_rows(
                    ui,
                    "test-log-viewer",
                    &(0..100).collect::<Vec<_>>(),
                    20.0,
                    400.0,
                    |row| format!("row {row}"),
                    |ui, index, _, _| {
                        if row_rects.borrow().len() <= index {
                            row_rects.borrow_mut().resize(index + 1, Rect::NOTHING);
                        }
                        let text = if multiline && index == 2 {
                            "row\nsecond line"
                        } else {
                            "row"
                        };
                        let response = ui.label(text);
                        row_rects.borrow_mut()[index] = response.rect;
                    },
                );
            });
        })
        .drop_without_applying_deltas();
        output
    }

    fn pointer_button(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        }
    }

    fn row_center(row_rects: &RefCell<Vec<Rect>>, row: usize) -> Pos2 {
        row_rects.borrow()[row].center()
    }

    #[test]
    fn pointer_drag_selects_multiple_rows_and_shift_click_extends_range() {
        let ctx = egui::Context::default();
        let row_rects = RefCell::new(Vec::new());
        run_view(&ctx, Vec::new(), Modifiers::default(), &row_rects);

        let start = row_center(&row_rects, 1);
        let drag_end = row_center(&row_rects, 4);
        let dragged_row_y = row_rects.borrow()[4].top();
        let output = run_view(
            &ctx,
            vec![
                Event::PointerMoved(start),
                pointer_button(start, true, Modifiers::default()),
            ],
            Modifiers::default(),
            &row_rects,
        );
        assert_eq!(output.selected_rows, Some(1..=1));

        let output = run_view(
            &ctx,
            vec![Event::PointerMoved(drag_end)],
            Modifiers::default(),
            &row_rects,
        );
        assert_eq!(output.selected_rows, Some(1..=4));

        let output = run_view(
            &ctx,
            vec![
                Event::PointerMoved(drag_end),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -5.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::default(),
                },
            ],
            Modifiers::default(),
            &row_rects,
        );
        assert_eq!(output.selected_rows, Some(1..=4));

        let output = run_view(
            &ctx,
            vec![pointer_button(drag_end, false, Modifiers::default())],
            Modifiers::default(),
            &row_rects,
        );
        assert_eq!(output.selected_rows, Some(1..=4));

        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        let shift_target = row_center(&row_rects, 7);
        let output = run_view(
            &ctx,
            vec![
                Event::PointerMoved(shift_target),
                pointer_button(shift_target, true, shift),
            ],
            shift,
            &row_rects,
        );
        assert_eq!(output.selected_rows, Some(1..=7));

        let shift_drag_end = row_center(&row_rects, 10);
        let output = run_view(
            &ctx,
            vec![Event::PointerMoved(shift_drag_end)],
            shift,
            &row_rects,
        );
        assert_eq!(output.selected_rows, Some(1..=10));
    }

    #[test]
    fn ctrl_c_copies_the_selected_rows() {
        let ctx = egui::Context::default();
        let row_rects = RefCell::new(Vec::new());
        run_view(&ctx, Vec::new(), Modifiers::default(), &row_rects);

        let start = row_center(&row_rects, 1);
        run_view(
            &ctx,
            vec![
                Event::PointerMoved(start),
                pointer_button(start, true, Modifiers::default()),
            ],
            Modifiers::default(),
            &row_rects,
        );

        let end = row_center(&row_rects, 3);
        run_view(
            &ctx,
            vec![Event::PointerMoved(end)],
            Modifiers::default(),
            &row_rects,
        );

        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            events: vec![
                Event::ModifiersChanged(Modifiers::default()),
                pointer_button(end, false, Modifiers::default()),
                Event::Copy,
            ],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            CentralPanel::default().show(ui, |ui| {
                LogViewer::show_rows(
                    ui,
                    "test-log-viewer",
                    &(0..100).collect::<Vec<_>>(),
                    20.0,
                    400.0,
                    |row| format!("row {row}"),
                    |ui, _, row, _| {
                        ui.label(format!("row {row}"));
                    },
                );
            });
        });

        let commands = output.platform_output.commands.clone();
        output.drop_without_applying_deltas();
        assert!(commands.contains(&egui::OutputCommand::CopyText(
            "row 1\nrow 2\nrow 3".to_owned()
        )));
    }

    #[test]
    fn row_callback_defaults_disable_text_selection_and_use_compact_spacing() {
        let ctx = egui::Context::default();
        let selectable = RefCell::new(true);
        let spacing = RefCell::new(Vec2::ZERO);
        ctx.run_ui(RawInput::default(), |ui| {
            CentralPanel::default().show(ui, |ui| {
                LogViewer::show_rows(
                    ui,
                    "label-selection-default",
                    &[()][..],
                    20.0,
                    100.0,
                    |_| "row".to_owned(),
                    |ui, _, _, _| {
                        *selectable.borrow_mut() = ui.style().interaction.selectable_labels;
                        *spacing.borrow_mut() = ui.spacing().item_spacing;
                        ui.scope(|nested_ui| nested_ui.label("not independently selectable"));
                    },
                );
            });
        })
        .drop_without_applying_deltas();
        assert!(!*selectable.borrow());
        assert_eq!(*spacing.borrow(), Vec2::new(8.0, 3.0));
    }

    #[test]
    fn clicking_between_rows_selects_the_nearest_row() {
        let ctx = egui::Context::default();
        let row_rects = RefCell::new(Vec::new());
        run_view(&ctx, Vec::new(), Modifiers::default(), &row_rects);

        let previous = row_rects.borrow()[1];
        let next = row_rects.borrow()[2];
        let gap_point = Pos2::new(
            previous.center().x,
            previous.bottom() + (next.top() - previous.bottom()) * 0.75,
        );
        let output = run_view(
            &ctx,
            vec![
                Event::PointerMoved(gap_point),
                pointer_button(gap_point, true, Modifiers::default()),
            ],
            Modifiers::default(),
            &row_rects,
        );
        assert_eq!(output.selected_rows, Some(2..=2));

        let output = run_view(
            &ctx,
            vec![pointer_button(gap_point, false, Modifiers::default())],
            Modifiers::default(),
            &row_rects,
        );

        assert_eq!(output.selected_rows, Some(2..=2));
    }

    #[test]
    fn multiline_rows_measure_height_and_select_from_each_line() {
        let ctx = egui::Context::default();
        let row_rects = RefCell::new(Vec::new());
        run_view_with_multiline(&ctx, Vec::new(), Modifiers::default(), &row_rects, true);
        run_view_with_multiline(&ctx, Vec::new(), Modifiers::default(), &row_rects, true);

        let second_line = {
            let row_rects = row_rects.borrow();
            assert!(row_rects[2].height() > row_rects[1].height());
            Pos2::new(row_rects[2].center().x, row_rects[2].center().y + 6.0)
        };
        let output = run_view_with_multiline(
            &ctx,
            vec![
                Event::PointerMoved(second_line),
                pointer_button(second_line, true, Modifiers::default()),
            ],
            Modifiers::default(),
            &row_rects,
            true,
        );
        assert_eq!(output.selected_rows, Some(2..=2));
        let output = run_view_with_multiline(
            &ctx,
            vec![pointer_button(second_line, false, Modifiers::default())],
            Modifiers::default(),
            &row_rects,
            true,
        );
        assert_eq!(output.selected_rows, Some(2..=2));

        let first_line = row_center(&row_rects, 1);
        let output = run_view_with_multiline(
            &ctx,
            vec![
                Event::PointerMoved(first_line),
                pointer_button(first_line, true, Modifiers::default()),
            ],
            Modifiers::default(),
            &row_rects,
            true,
        );
        assert_eq!(output.selected_rows, Some(1..=1));

        let output = run_view_with_multiline(
            &ctx,
            vec![Event::PointerMoved(second_line)],
            Modifiers::default(),
            &row_rects,
            true,
        );
        assert_eq!(output.selected_rows, Some(1..=2));
    }

    #[test]
    fn selection_range_is_inclusive_and_order_independent() {
        let mut selection = Selection::default();
        selection.begin(8, false);
        selection.extend_to(3);

        assert_eq!(selection.range(), Some(3..=8));
    }

    #[test]
    fn shift_selection_extends_existing_anchor_across_unrendered_rows() {
        let mut selection = Selection::default();
        selection.begin(4, false);
        selection.finish();
        selection.begin(100_000, true);

        assert_eq!(selection.range(), Some(4..=100_000));
        assert_eq!(selection.anchor, Some(4));
    }

    #[test]
    fn ordinary_selection_replaces_the_previous_anchor() {
        let mut selection = Selection::default();
        selection.begin(4, false);
        selection.finish();
        selection.begin(9, false);

        assert_eq!(selection.range(), Some(9..=9));
        assert_eq!(selection.anchor, Some(9));
    }

    #[test]
    fn height_index_tracks_variable_rows_and_resizing() {
        let mut heights = HeightIndex::default();
        heights.resize(4, 20.0, 16.0);
        heights.set(1, 36.0, 32.0, 0.0);
        heights.set(3, 28.0, 24.0, 0.0);

        assert_eq!(heights.prefix_sum(3), 76.0);
        assert_eq!(heights.total(), 104.0);
        assert_eq!(heights.row_at(19.0), 0);
        assert_eq!(heights.row_at(20.0), 1);
        assert_eq!(heights.row_at(55.0), 1);
        assert_eq!(heights.row_at(56.0), 2);
        assert_eq!(heights.row_at(75.0), 2);
        assert_eq!(heights.row_at(76.0), 3);

        heights.resize(5, 20.0, 16.0);
        assert_eq!(heights.total(), 124.0);
        heights.resize(2, 20.0, 16.0);
        assert_eq!(heights.total(), 56.0);
    }
}
