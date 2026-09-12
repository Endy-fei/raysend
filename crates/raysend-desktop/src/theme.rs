//! 配色对齐网页 `crates/raysend-web/public/app.css`。

use iced::theme::{self, Palette};
use iced::{Background, Border, Color, Shadow, Theme, Vector};
use iced::widget::{button, container, progress_bar, text};

/// 与 CSS 变量一一对应。
#[derive(Clone, Copy)]
pub struct Colors {
    pub bg: Color,
    #[allow(dead_code)]
    pub bg_accent: Color,
    pub ink: Color,
    pub muted: Color,
    pub card: Color,
    pub line: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub danger: Color,
    pub ok: Color,
    #[allow(dead_code)]
    pub video: Color,
}

impl Colors {
    pub fn light() -> Self {
        Self {
            bg: rgb(0xF3, 0xEF, 0xE6),
            bg_accent: rgb(0xE7, 0xDF, 0xD0),
            ink: rgb(0x1C, 0x19, 0x15),
            muted: rgb(0x6F, 0x67, 0x5C),
            card: Color::from_rgba8(255, 251, 245, 0.86),
            line: Color::from_rgba8(28, 25, 21, 0.08),
            accent: rgb(0xC4, 0x5C, 0x26),
            accent_soft: rgb(0xF3, 0xD2, 0xBF),
            danger: rgb(0xB4, 0x23, 0x18),
            ok: rgb(0x2F, 0x6F, 0x4E),
            video: rgb(0x0C, 0x0D, 0x10),
        }
    }

    pub fn dark() -> Self {
        Self {
            bg: rgb(0x12, 0x13, 0x18),
            bg_accent: rgb(0x1B, 0x1D, 0x26),
            ink: rgb(0xF4, 0xEF, 0xE6),
            muted: rgb(0xAD, 0xA5, 0x9A),
            card: Color::from_rgba8(24, 26, 34, 0.88),
            line: Color::from_rgba8(244, 239, 230, 0.08),
            accent: rgb(0xE3, 0x9A, 0x6A),
            accent_soft: Color::from_rgba8(227, 154, 106, 0.16),
            danger: rgb(0xFF, 0x7D, 0x73),
            ok: rgb(0x7D, 0xCE, 0xA0),
            video: rgb(0x0C, 0x0D, 0x10),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

impl Mode {
    pub fn toggle(self) -> Self {
        match self {
            Mode::Light => Mode::Dark,
            Mode::Dark => Mode::Light,
        }
    }

    pub fn colors(self) -> Colors {
        match self {
            Mode::Light => Colors::light(),
            Mode::Dark => Colors::dark(),
        }
    }

    pub fn iced_theme(self) -> Theme {
        let c = self.colors();
        let name = match self {
            Mode::Light => "RaySend Light",
            Mode::Dark => "RaySend Dark",
        };
        Theme::custom(
            name,
            Palette {
                background: c.bg,
                text: c.ink,
                primary: c.accent,
                success: c.ok,
                warning: c.accent,
                danger: c.danger,
            },
        )
    }

    pub fn appearance(self) -> theme::Style {
        let c = self.colors();
        theme::Style {
            background_color: c.bg,
            text_color: c.ink,
        }
    }
}

pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

fn card_shadow(mode: Mode) -> Shadow {
    match mode {
        Mode::Light => Shadow {
            color: Color::from_rgba8(60, 42, 24, 0.12),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 30.0,
        },
        Mode::Dark => Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 35.0,
        },
    }
}

pub fn shell(mode: Mode) -> impl Fn(&Theme) -> container::Style {
    let c = mode.colors();
    move |_| container::Style {
        background: Some(Background::Color(c.bg)),
        text_color: Some(c.ink),
        ..container::Style::default()
    }
}

pub fn card(mode: Mode) -> impl Fn(&Theme) -> container::Style {
    let c = mode.colors();
    let shadow = card_shadow(mode);
    move |_| container::Style {
        background: Some(Background::Color(c.card)),
        text_color: Some(c.ink),
        border: Border {
            color: c.line,
            width: 1.0,
            radius: 22.0.into(),
        },
        shadow,
        ..container::Style::default()
    }
}

pub fn segmented(mode: Mode) -> impl Fn(&Theme) -> container::Style {
    let c = mode.colors();
    move |_| container::Style {
        background: Some(Background::Color(fade(c.card, 0.7))),
        border: Border {
            color: c.line,
            width: 1.0,
            radius: 18.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn dropzone(mode: Mode) -> impl Fn(&Theme) -> container::Style {
    let c = mode.colors();
    move |_| container::Style {
        background: Some(Background::Color(fade(c.accent_soft, 0.55))),
        border: Border {
            color: Color {
                a: 0.45,
                ..c.accent
            },
            width: 1.5,
            radius: 18.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn dock(mode: Mode) -> impl Fn(&Theme) -> container::Style {
    let c = mode.colors();
    move |_| container::Style {
        background: Some(Background::Color(fade(c.card, 0.92))),
        border: Border {
            color: c.line,
            width: 1.0,
            radius: 20.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn qr_stage() -> impl Fn(&Theme) -> container::Style {
    |_| container::Style {
        background: Some(Background::Color(Color::WHITE)),
        border: Border {
            color: Color::WHITE,
            width: 0.0,
            radius: 18.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(60, 42, 24, 0.12),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 30.0,
        },
        ..container::Style::default()
    }
}

pub fn video_wrap() -> impl Fn(&Theme) -> container::Style {
    |_| container::Style {
        background: Some(Background::Color(rgb(0x0C, 0x0D, 0x10))),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 18.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn success(mode: Mode) -> impl Fn(&Theme) -> container::Style {
    let c = mode.colors();
    move |_| container::Style {
        background: Some(Background::Color(mix(c.ok, c.card, 0.12))),
        border: Border {
            color: c.line,
            width: 0.0,
            radius: 16.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn brand_mark(mode: Mode) -> impl Fn(&Theme) -> container::Style {
    let c = mode.colors();
    move |_| container::Style {
        background: Some(Background::Color(c.accent)),
        border: Border {
            color: c.accent,
            width: 0.0,
            radius: 14.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(196, 92, 38, 0.28),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}

pub fn mark_cell() -> impl Fn(&Theme) -> container::Style {
    |_| container::Style {
        background: Some(Background::Color(Color::WHITE)),
        border: Border {
            color: Color::WHITE,
            width: 0.0,
            radius: 3.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn pill(mode: Mode) -> impl Fn(&Theme) -> container::Style {
    let c = mode.colors();
    move |_| container::Style {
        background: Some(Background::Color(c.card)),
        text_color: Some(c.ink),
        border: Border {
            color: c.line,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn live_badge() -> impl Fn(&Theme) -> container::Style {
    |_| container::Style {
        background: Some(Background::Color(rgb(0xE2, 0x3D, 0x2A))),
        text_color: Some(Color::WHITE),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 999.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn muted_text(mode: Mode) -> impl Fn(&Theme) -> text::Style {
    let c = mode.colors();
    move |_| text::Style {
        color: Some(c.muted),
    }
}

pub fn btn_primary(mode: Mode) -> impl Fn(&Theme, button::Status) -> button::Style {
    let c = mode.colors();
    move |_, status| pill_button(c.accent, Color::WHITE, Color::TRANSPARENT, status)
}

pub fn btn_danger(mode: Mode) -> impl Fn(&Theme, button::Status) -> button::Style {
    let c = mode.colors();
    move |_, status| pill_button(c.danger, Color::WHITE, Color::TRANSPARENT, status)
}

pub fn btn_card(mode: Mode) -> impl Fn(&Theme, button::Status) -> button::Style {
    let c = mode.colors();
    move |_, status| pill_button(c.card, c.ink, c.line, status)
}

pub fn btn_ghost(mode: Mode) -> impl Fn(&Theme, button::Status) -> button::Style {
    let c = mode.colors();
    move |_, status| pill_button(Color::TRANSPARENT, c.ink, Color::TRANSPARENT, status)
}

pub fn btn_lang(mode: Mode, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    let c = mode.colors();
    move |_, status| {
        if active {
            pill_button(c.accent, Color::WHITE, Color::TRANSPARENT, status)
        } else {
            pill_button(c.card, c.muted, c.line, status)
        }
    }
}

pub fn btn_segment(mode: Mode, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    let c = mode.colors();
    let shadow = card_shadow(mode);
    move |_, status| {
        let mut style = if active {
            pill_button(c.card, c.ink, Color::TRANSPARENT, status)
        } else {
            pill_button(Color::TRANSPARENT, c.muted, Color::TRANSPARENT, status)
        };
        style.border.radius = 14.0.into();
        if active {
            style.shadow = shadow;
        }
        style
    }
}

pub fn progress(mode: Mode) -> impl Fn(&Theme) -> progress_bar::Style {
    let c = mode.colors();
    move |_| progress_bar::Style {
        background: Background::Color(c.line),
        bar: Background::Color(c.accent),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 999.0.into(),
        },
    }
}

fn pill_button(bg: Color, fg: Color, border: Color, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Color { a: (bg.a * 0.88).min(1.0), ..bg }
        }
        _ => bg,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: fg,
        border: Border {
            color: border,
            width: if border.a == 0.0 { 0.0 } else { 1.0 },
            radius: 999.0.into(),
        },
        ..button::Style::default()
    }
}

fn fade(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color {
        r: a.r * amount + b.r * (1.0 - amount),
        g: a.g * amount + b.g * (1.0 - amount),
        b: a.b * amount + b.b * (1.0 - amount),
        a: a.a * amount + b.a * (1.0 - amount),
    }
}
