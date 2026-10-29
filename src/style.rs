//! GeoConveyor's look: a soft grey-blue canvas, white rounded inputs, dark pill buttons and
//! tabs, a blue accent and a dark code-style command box. Both a light and a dark palette.

use eframe::egui;
use eframe::egui::{
    text::{LayoutJob, TextFormat},
    vec2, Color32, CornerRadius, FontFamily, FontId, Margin, Response, RichText, Sense, Stroke, StrokeKind, Style, TextStyle, Theme, Ui, Visuals,
};

#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: Color32,
    pub card: Color32,
    pub input: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub pill: Color32,
    pub pill_text: Color32,
    pub accent: Color32,
    pub go: Color32,
    pub bad: Color32,
    pub code_bg: Color32,
    pub code_text: Color32,
    pub code_cmd: Color32,
    pub code_flag: Color32,
    pub code_num: Color32,
    pub stripe: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn pal(dark: bool) -> Pal {
    if dark {
        Pal {
            bg: rgb(0x1c, 0x20, 0x28),
            card: rgb(0x2a, 0x30, 0x3b),
            input: rgb(0x12, 0x15, 0x1b),
            text: rgb(0xe6, 0xe9, 0xef),
            muted: rgb(0x8d, 0x96, 0xa6),
            border: rgb(0x3b, 0x43, 0x52),
            pill: rgb(0x39, 0x42, 0x52),
            pill_text: rgb(0xf3, 0xf5, 0xf9),
            accent: rgb(0x3b, 0x8b, 0xd9),
            go: rgb(0x23, 0x8a, 0x45),
            bad: rgb(0xef, 0x6b, 0x6b),
            code_bg: rgb(0x0e, 0x11, 0x16),
            code_text: rgb(0xe6, 0xe9, 0xef),
            code_cmd: rgb(0x7e, 0xe7, 0x87),
            code_flag: rgb(0xc7, 0x92, 0xea),
            code_num: rgb(0xf6, 0xd3, 0x6b),
            stripe: rgb(0x22, 0x27, 0x30),
        }
    } else {
        Pal {
            bg: rgb(0xe6, 0xe9, 0xf0),
            card: rgb(0xf4, 0xf6, 0xfa),
            input: rgb(0xff, 0xff, 0xff),
            text: rgb(0x1d, 0x23, 0x2e),
            muted: rgb(0x6a, 0x73, 0x84),
            border: rgb(0xc4, 0xca, 0xd6),
            pill: rgb(0x2b, 0x35, 0x44),
            pill_text: rgb(0xff, 0xff, 0xff),
            accent: rgb(0x2f, 0x86, 0xd6),
            go: rgb(0x1d, 0x7a, 0x3a),
            bad: rgb(0xd0, 0x3a, 0x3a),
            code_bg: rgb(0x1e, 0x24, 0x30),
            code_text: rgb(0xee, 0xf1, 0xf6),
            code_cmd: rgb(0x7e, 0xe7, 0x87),
            code_flag: rgb(0xc7, 0x92, 0xea),
            code_num: rgb(0xf6, 0xd3, 0x6b),
            stripe: rgb(0xef, 0xf1, 0xf6),
        }
    }
}

pub fn p(ui: &Ui) -> Pal {
    pal(ui.visuals().dark_mode)
}

fn style_for(theme: Theme) -> Style {
    let dark = theme == Theme::Dark;
    let c = pal(dark);
    let mut s = Style::default();
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };

    let r8 = CornerRadius::same(8);
    v.panel_fill = c.bg;
    v.window_fill = c.bg;
    v.window_stroke = Stroke::new(1.0, c.border);
    v.window_corner_radius = CornerRadius::same(14);
    v.menu_corner_radius = CornerRadius::same(10);
    v.extreme_bg_color = c.input;
    v.faint_bg_color = c.stripe;
    v.hyperlink_color = c.accent;
    v.selection.bg_fill = c.accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);

    v.widgets.noninteractive.bg_fill = c.bg;
    v.widgets.noninteractive.weak_bg_fill = c.bg;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
    v.widgets.noninteractive.corner_radius = r8;

    for (w, lift) in [(&mut v.widgets.inactive, 0), (&mut v.widgets.hovered, 1), (&mut v.widgets.active, 2), (&mut v.widgets.open, 1)] {
        w.corner_radius = r8;
        w.weak_bg_fill = if dark { rgb(0x2a + lift * 6, 0x30 + lift * 6, 0x3b + lift * 6) } else { rgb(0xf4 - lift * 7, 0xf6 - lift * 6, 0xfa - lift * 5) };
        w.bg_fill = c.input;
        w.bg_stroke = Stroke::new(1.0, if lift == 0 { c.border } else { c.accent });
        w.fg_stroke = Stroke::new(1.0, c.text);
        w.expansion = 0.0;
    }
    s.visuals = v;

    s.spacing.item_spacing = vec2(8.0, 7.0);
    s.spacing.button_padding = vec2(12.0, 5.0);
    s.spacing.interact_size.y = 28.0;
    s.spacing.text_edit_width = 220.0;
    s.spacing.menu_margin = Margin::same(8);
    s.spacing.window_margin = Margin::same(14);
    s.spacing.indent = 18.0;
    s.spacing.combo_height = 400.0;

    s.text_styles.insert(TextStyle::Body, FontId::new(14.5, FontFamily::Proportional));
    s.text_styles.insert(TextStyle::Button, FontId::new(14.5, FontFamily::Proportional));
    s.text_styles.insert(TextStyle::Heading, FontId::new(21.0, FontFamily::Proportional));
    s.text_styles.insert(TextStyle::Small, FontId::new(11.5, FontFamily::Proportional));
    s.text_styles.insert(TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace));
    s
}

pub fn install(ctx: &egui::Context) {
    ctx.set_style_of(Theme::Light, style_for(Theme::Light));
    ctx.set_style_of(Theme::Dark, style_for(Theme::Dark));
}

// ---------------------------------------------------------------------------
// widgets
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    None,
    Folder,
    Copy,
}

fn paint_icon(ui: &Ui, icon: Icon, r: egui::Rect, color: Color32) {
    let c = r.center();
    let pt = ui.painter();
    match icon {
        Icon::None => {}
        Icon::Folder => {
            let (w, h) = (15.0, 11.0);
            let body = egui::Rect::from_center_size(egui::pos2(c.x, c.y + 1.0), vec2(w, h));
            let tab = egui::Rect::from_min_size(body.left_top() - vec2(0.0, 2.5), vec2(6.0, 3.5));
            pt.rect_filled(tab, CornerRadius::same(1), color);
            pt.rect_filled(body, CornerRadius::same(2), color);
        }
        Icon::Copy => {
            let a = egui::Rect::from_center_size(egui::pos2(c.x - 1.5, c.y - 1.5), vec2(9.0, 11.0));
            let b = egui::Rect::from_center_size(egui::pos2(c.x + 2.0, c.y + 2.0), vec2(9.0, 11.0));
            pt.rect_stroke(a, CornerRadius::same(2), Stroke::new(1.4, color), StrokeKind::Inside);
            pt.rect_filled(b, CornerRadius::same(2), color);
        }
    }
}

/// A dark pill button (the "primary chrome" of the app), optionally with a small icon.
pub fn pill_button(ui: &mut Ui, text: &str, icon: Icon) -> Response {
    let c = p(ui);
    let on = ui.is_enabled();
    let fg = if on { c.pill_text } else { c.muted };
    let fill = if on { c.pill } else { c.card };
    let label = if icon == Icon::None { text.to_string() } else { format!("      {text}") };
    let r = ui.add(egui::Button::new(RichText::new(label).color(fg)).fill(fill).stroke(Stroke::NONE).corner_radius(CornerRadius::same(15)).min_size(vec2(0.0, 30.0)));
    if icon != Icon::None {
        let ir = egui::Rect::from_min_size(r.rect.left_top() + vec2(10.0, 0.0), vec2(18.0, r.rect.height()));
        paint_icon(ui, icon, ir, fg);
    }
    r
}

/// The green "go" button.
pub fn go_button(ui: &mut Ui, text: &str) -> Response {
    let c = p(ui);
    let on = ui.is_enabled();
    ui.add(
        egui::Button::new(RichText::new(text).strong().color(if on { Color32::WHITE } else { c.muted }))
            .fill(if on { c.go } else { c.card })
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(15))
            .min_size(vec2(64.0, 30.0)),
    )
}

/// A function tab: dark pill, or the accent colour when selected.
pub fn tab_pill(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let c = p(ui);
    let fill = if selected { c.accent } else { c.pill };
    ui.add(egui::Button::new(RichText::new(text).color(c.pill_text)).fill(fill).stroke(Stroke::NONE).corner_radius(CornerRadius::same(16)).min_size(vec2(0.0, 32.0)))
}

/// A choice among a few: the chosen one is an accent pill, the rest plain text.
pub fn choice(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let c = p(ui);
    if selected {
        ui.add(egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(c.accent).stroke(Stroke::NONE).corner_radius(CornerRadius::same(14)).min_size(vec2(0.0, 28.0)))
    } else {
        ui.add(egui::Button::new(RichText::new(text).color(c.text)).fill(Color32::TRANSPARENT).stroke(Stroke::NONE).min_size(vec2(0.0, 28.0)))
    }
}

/// An on/off switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let c = p(ui);
    let size = vec2(44.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let track = if *on { c.accent } else { c.border };
    let track = if ui.is_enabled() { track } else { track.gamma_multiply(0.5) };
    ui.painter().rect_filled(rect, CornerRadius::same(12), track);
    let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 9.0, Color32::WHITE);
    resp
}

/// A rounded status badge with a coloured dot.
pub fn status_pill(ui: &mut Ui, text: &str, dot: Color32) {
    let fill = if ui.visuals().dark_mode { rgb(0x3a, 0x42, 0x50) } else { rgb(0x7b, 0x84, 0x95) };
    egui::Frame::new().fill(fill).corner_radius(CornerRadius::same(11)).inner_margin(Margin::symmetric(9, 0)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new(text).color(Color32::WHITE).size(13.0));
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 4.5, dot);
        });
    });
}

/// The command preview: a dark rounded box with the command syntax-coloured.
pub fn command_box(ui: &mut Ui, cmd: &[String]) {
    let c = p(ui);
    let font = FontId::new(14.0, FontFamily::Monospace);
    let mut job = LayoutJob::default();
    job.wrap.max_width = ui.available_width() - 24.0;
    for (i, tok) in cmd.iter().enumerate() {
        let color = if i < 2 {
            c.code_cmd
        } else if tok.starts_with('-') {
            c.code_flag
        } else if !tok.is_empty() && tok.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
            c.code_num
        } else {
            c.code_text
        };
        job.append(tok, 0.0, TextFormat { font_id: font.clone(), color, ..Default::default() });
        job.append(" ", 0.0, TextFormat { font_id: font.clone(), color: c.code_text, ..Default::default() });
    }
    egui::Frame::new().fill(c.code_bg).corner_radius(CornerRadius::same(10)).inner_margin(Margin::symmetric(12, 7)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(job).selectable(true));
    });
}

/// A checkbox that fills with the accent colour when ticked.
pub fn check(ui: &mut Ui, on: &mut bool, text: &str) -> Response {
    let c = p(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (rect, mut r) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
        let l = ui.add(egui::Label::new(text).sense(Sense::click()));
        if r.clicked() || l.clicked() {
            *on = !*on;
            r.mark_changed();
        }
        let pt = ui.painter();
        if *on {
            pt.rect_filled(rect, CornerRadius::same(5), c.accent);
            let (a, b, d) = (rect.left_top() + vec2(4.5, 9.5), rect.left_top() + vec2(7.8, 12.8), rect.left_top() + vec2(13.5, 5.5));
            pt.line_segment([a, b], Stroke::new(2.0, Color32::WHITE));
            pt.line_segment([b, d], Stroke::new(2.0, Color32::WHITE));
        } else {
            pt.rect_filled(rect, CornerRadius::same(5), c.input);
            pt.rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0, c.border), StrokeKind::Inside);
        }
        r
    })
    .inner
}

/// A rounded progress bar with centred text.
pub fn progress(ui: &mut Ui, frac: f32, text: &str) {
    let c = p(ui);
    let h = 24.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let track = if ui.visuals().dark_mode { rgb(0x2a, 0x30, 0x3b) } else { rgb(0xd5, 0xda, 0xe4) };
    let pt = ui.painter();
    pt.rect_filled(rect, CornerRadius::same(12), track);
    let w = (rect.width() * frac.clamp(0.0, 1.0)).max(if frac > 0.0 { h } else { 0.0 });
    if w > 0.0 {
        pt.rect_filled(egui::Rect::from_min_size(rect.min, vec2(w, h)), CornerRadius::same(12), c.accent);
    }
    pt.text(rect.left_center() + vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, text, FontId::new(14.0, FontFamily::Proportional), if frac > 0.0 { Color32::WHITE } else { c.text });
}
