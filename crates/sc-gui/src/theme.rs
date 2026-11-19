//! Colours, fonts and the few custom widgets the pages share.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin,
    RichText, Sense, Stroke, TextStyle, Ui, Vec2,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(10, 13, 18);
pub const PANEL: Color32 = Color32::from_rgb(15, 19, 26);
pub const CARD: Color32 = Color32::from_rgb(21, 27, 36);
pub const LINE: Color32 = Color32::from_rgb(36, 45, 58);
pub const TEXT: Color32 = Color32::from_rgb(222, 229, 238);
pub const WEAK: Color32 = Color32::from_rgb(132, 146, 166);
pub const ACCENT: Color32 = Color32::from_rgb(64, 186, 236);
pub const ON_ACCENT: Color32 = Color32::from_rgb(6, 20, 30);
pub const AMBER: Color32 = Color32::from_rgb(242, 172, 64);
pub const GREEN: Color32 = Color32::from_rgb(88, 204, 134);
pub const RED: Color32 = Color32::from_rgb(240, 98, 98);

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.otf"))),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.otf"))),
    );
    // Inter first; egui's own fonts stay behind it for symbols and icons.
    fonts.families.get_mut(&FontFamily::Proportional).expect("default family").insert(0, "inter".into());
    let mut bold_family = vec!["inter-semibold".to_string()];
    bold_family.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(bold(), bold_family);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|s| {
        s.text_styles.insert(TextStyle::Body, FontId::proportional(14.5));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(14.5));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.5));
        s.text_styles.insert(TextStyle::Heading, FontId::new(24.0, bold()));
        s.spacing.item_spacing = Vec2::new(10.0, 8.0);
        s.spacing.button_padding = Vec2::new(14.0, 7.0);
        s.spacing.interact_size.y = 30.0;
        let v = &mut s.visuals;
        v.dark_mode = true;
        v.panel_fill = BG;
        v.window_fill = CARD;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = CornerRadius::same(12);
        v.extreme_bg_color = Color32::from_rgb(12, 16, 22);
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = ACCENT;
        v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(8);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(30, 38, 50);
        v.widgets.inactive.bg_fill = Color32::from_rgb(30, 38, 50);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(38, 48, 63);
        v.widgets.hovered.bg_fill = Color32::from_rgb(38, 48, 63);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
        v.widgets.active.weak_bg_fill = Color32::from_rgb(44, 56, 74);
        v.widgets.active.bg_fill = Color32::from_rgb(44, 56, 74);
    });
}

/// A rounded panel the width of the page.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A small rounded label, e.g. a status.
pub fn pill(ui: &mut Ui, text: &str, color: Color32) {
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(12.5), color);
    let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::new(22.0, 8.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(255), color.gamma_multiply(0.14));
    p.rect_stroke(
        rect,
        CornerRadius::same(255),
        Stroke::new(1.0, color.gamma_multiply(0.55)),
        egui::StrokeKind::Inside,
    );
    p.galley(rect.center() - galley.size() / 2.0, galley, color);
}

/// The main action on a page.
pub fn primary_button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(ON_ACCENT).family(bold()))
            .fill(ACCENT)
            .min_size(Vec2::new(0.0, 36.0)),
    )
}

pub fn button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(Vec2::new(0.0, 36.0)))
}

pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).family(bold()).size(16.0));
}

pub fn weak(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(WEAK)
}

/// A sidebar entry. Returns true when clicked.
pub fn nav_item(
    ui: &mut Ui,
    selected: bool,
    icon: &str,
    label: &str,
    badge: Option<(String, Color32)>,
) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::same(8), ACCENT.gamma_multiply(0.14));
        p.rect_filled(
            egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 9.0), Vec2::new(3.0, 20.0)),
            CornerRadius::same(2),
            ACCENT,
        );
    } else if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(24, 31, 41));
    }
    let (icon_color, text_color) = if selected { (ACCENT, TEXT) } else { (WEAK, WEAK) };
    p.text(
        rect.left_center() + Vec2::new(16.0, 0.0),
        Align2::LEFT_CENTER,
        icon,
        FontId::proportional(16.0),
        icon_color,
    );
    p.text(
        rect.left_center() + Vec2::new(44.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.5),
        text_color,
    );
    if let Some((text, color)) = badge {
        let galley = ui.painter().layout_no_wrap(text, FontId::proportional(11.5), ON_ACCENT);
        let size = Vec2::new(galley.size().x.max(galley.size().y) + 10.0, galley.size().y + 4.0);
        let r = egui::Rect::from_center_size(rect.right_center() - Vec2::new(10.0 + size.x / 2.0, 0.0), size);
        p.rect_filled(r, CornerRadius::same(255), color);
        p.galley(r.center() - galley.size() / 2.0, galley, ON_ACCENT);
    }
    resp.clicked()
}
