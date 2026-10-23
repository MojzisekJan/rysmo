//! Check boxes, radio buttons and pop-up menus that take the platform's look.
//! On macOS they are drawn like AppKit controls; elsewhere they are egui's own.
use bevy_egui::egui;

/// A check box; `indeterminate` shows a dash for a mixed selection.
pub struct Check<'a> {
    value: &'a mut bool,
    text: egui::WidgetText,
    indeterminate: bool,
}

impl<'a> Check<'a> {
    pub fn new(value: &'a mut bool, text: impl Into<egui::WidgetText>) -> Self {
        Self { value, text: text.into(), indeterminate: false }
    }
    pub fn indeterminate(mut self, indeterminate: bool) -> Self {
        self.indeterminate = indeterminate;
        self
    }
}

impl egui::Widget for Check<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        #[cfg(target_os = "macos")]
        {
            ui.add(egui_native_look::Checkbox::new(self.value, self.text).indeterminate(self.indeterminate))
        }
        #[cfg(not(target_os = "macos"))]
        {
            ui.add(egui::Checkbox::new(self.value, self.text).indeterminate(self.indeterminate))
        }
    }
}

pub fn checkbox(ui: &mut egui::Ui, value: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(Check::new(value, text))
}

/// A radio button that sets `current` to `value` when clicked.
pub fn radio_value<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    #[cfg(target_os = "macos")]
    {
        egui_native_look::radio_value(ui, current, value, text)
    }
    #[cfg(not(target_os = "macos"))]
    {
        ui.radio_value(current, value, text)
    }
}

/// A pop-up menu button; on macOS it carries the accent chevron box.
pub fn combo(combo: egui::ComboBox) -> egui::ComboBox {
    #[cfg(target_os = "macos")]
    {
        combo.icon(egui_native_look::combo_icon)
    }
    #[cfg(not(target_os = "macos"))]
    {
        combo
    }
}

/// A slider with its number beside it; on macOS an AppKit slider with a white knob.
pub struct Slider<'a, T: egui::emath::Numeric> {
    value: &'a mut T,
    range: std::ops::RangeInclusive<T>,
    text: String,
    suffix: String,
    logarithmic: bool,
    step: Option<f64>,
    clamping: egui::SliderClamping,
}

impl<'a, T: egui::emath::Numeric> Slider<'a, T> {
    pub fn new(value: &'a mut T, range: std::ops::RangeInclusive<T>) -> Self {
        Self {
            value,
            range,
            text: String::new(),
            suffix: String::new(),
            logarithmic: false,
            step: None,
            clamping: Default::default(),
        }
    }
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }
    pub fn logarithmic(mut self, logarithmic: bool) -> Self {
        self.logarithmic = logarithmic;
        self
    }
    pub fn step_by(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }
    pub fn clamping(mut self, clamping: egui::SliderClamping) -> Self {
        self.clamping = clamping;
        self
    }
}

impl<T: egui::emath::Numeric> egui::Widget for Slider<'_, T> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        #[cfg(target_os = "macos")]
        {
            let mut slider = egui_native_look::Slider::new(self.value, self.range)
                .show_value(true)
                .text(self.text)
                .suffix(self.suffix)
                .logarithmic(self.logarithmic)
                .clamping(self.clamping);
            if let Some(step) = self.step {
                slider = slider.step_by(step);
            }
            ui.add(slider)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mut slider = egui::Slider::new(self.value, self.range)
                .text(self.text)
                .suffix(self.suffix)
                .logarithmic(self.logarithmic)
                .clamping(self.clamping);
            if let Some(step) = self.step {
                slider = slider.step_by(step);
            }
            ui.add(slider)
        }
    }
}

/// A number field; on macOS an inset AppKit text field without steppers.
pub struct Number<'a, T: egui::emath::Numeric> {
    value: &'a mut T,
    range: Option<std::ops::RangeInclusive<f64>>,
    speed: Option<f64>,
    prefix: String,
    suffix: String,
    max_decimals: Option<usize>,
}

impl<'a, T: egui::emath::Numeric> Number<'a, T> {
    pub fn new(value: &'a mut T) -> Self {
        Self { value, range: None, speed: None, prefix: String::new(), suffix: String::new(), max_decimals: None }
    }
    pub fn range<N: egui::emath::Numeric>(mut self, range: std::ops::RangeInclusive<N>) -> Self {
        self.range = Some(range.start().to_f64()..=range.end().to_f64());
        self
    }
    pub fn speed(mut self, speed: impl Into<f64>) -> Self {
        self.speed = Some(speed.into());
        self
    }
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }
    pub fn max_decimals(mut self, decimals: usize) -> Self {
        self.max_decimals = Some(decimals);
        self
    }
}

impl<T: egui::emath::Numeric> egui::Widget for Number<'_, T> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        #[cfg(target_os = "macos")]
        {
            let mut field =
                egui_native_look::NumberField::new(self.value).steppers(false).prefix(self.prefix).suffix(self.suffix);
            if let Some(range) = self.range {
                field = field.range(range);
            }
            if let Some(speed) = self.speed {
                field = field.speed(speed);
            }
            if let Some(decimals) = self.max_decimals {
                field = field.max_decimals(decimals);
            }
            ui.scope(|ui| {
                let ctx = ui.ctx().clone();
                crate::macos::field_visuals(&ctx, ui.visuals_mut());
                ui.add(field)
            })
            .inner
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mut drag = egui::DragValue::new(self.value).prefix(self.prefix).suffix(self.suffix);
            if let Some(range) = self.range {
                drag = drag.range(range);
            }
            if let Some(speed) = self.speed {
                drag = drag.speed(speed);
            }
            if let Some(decimals) = self.max_decimals {
                drag = drag.max_decimals(decimals);
            }
            // A minimum width keeps the field from shrinking to its typed text while editing.
            ui.scope(|ui| {
                let size = &mut ui.spacing_mut().interact_size.x;
                *size = size.max(64.);
                ui.add(drag)
            })
            .inner
        }
    }
}

/// A collapsing section with a title; on macOS drawn as an AppKit disclosure row.
pub fn section<R>(
    ui: &mut egui::Ui,
    title: &str,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    #[cfg(target_os = "macos")]
    {
        egui_native_look::Section::new(title).default_open(default_open).show(ui, add_contents).body_returned
    }
    #[cfg(not(target_os = "macos"))]
    {
        egui::CollapsingHeader::new(title).default_open(default_open).show(ui, add_contents).body_returned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(ui_fn: impl Fn(&mut egui::Ui) -> egui::Response) -> bool {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut at = egui::Pos2::ZERO;
        let mut changed = false;
        for pass in 0..5 {
            let events = match pass {
                3 => vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                4 => vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                _ => vec![],
            };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400., 300.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let r = ui_fn(ui);
                    at = r.rect.left_center() + egui::vec2(4., 0.);
                    changed |= r.changed();
                },
            );
            output.textures_delta.clear();
        }
        changed
    }

    #[test]
    fn check_box_and_radio_change_their_values_when_clicked() {
        let value = std::cell::Cell::new(false);
        assert!(click(|ui| {
            let mut v = value.get();
            let r = checkbox(ui, &mut v, "Snap");
            value.set(v);
            r
        }));
        assert!(value.get());
        let choice = std::cell::Cell::new(0);
        assert!(click(|ui| {
            let mut c = choice.get();
            let r = radio_value(ui, &mut c, 2, "Two");
            choice.set(c);
            r
        }));
        assert_eq!(choice.get(), 2);
    }
}
