#[allow(unused_imports)]
pub(crate) use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalColorLevel {
    TrueColor,
    Ansi256,
    Ansi16,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalProfile {
    pub(crate) foreground: Option<(u8, u8, u8)>,
    pub(crate) background: Option<(u8, u8, u8)>,
    pub(crate) color_level: TerminalColorLevel,
}

impl TerminalProfile {
    pub(crate) fn detect(interactive: bool) -> Self {
        let color_level = terminal_color_level();
        let colors = interactive.then(|| query_terminal_default_colors(Duration::from_millis(100)));
        let colors = colors.flatten();
        Self {
            foreground: colors.map(|colors| colors.foreground),
            background: colors.map(|colors| colors.background),
            color_level,
        }
    }

    #[cfg(test)]
    pub(crate) fn dark() -> Self {
        Self {
            foreground: Some((238, 238, 238)),
            background: Some((12, 12, 14)),
            color_level: TerminalColorLevel::TrueColor,
        }
    }

    #[cfg(test)]
    pub(crate) fn light() -> Self {
        Self {
            foreground: Some((20, 24, 28)),
            background: Some((250, 250, 247)),
            color_level: TerminalColorLevel::TrueColor,
        }
    }

    #[cfg(test)]
    pub(crate) fn unknown() -> Self {
        Self {
            foreground: None,
            background: None,
            color_level: TerminalColorLevel::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TuiTheme {
    pub(crate) surface_bg: Color,
    pub(crate) menu_bg: Color,
    pub(crate) menu_selected_bg: Color,
    pub(crate) selection_bg: Color,
    pub(crate) panel_field_bg: Color,
    pub(crate) panel_field_fg: Color,
    pub(crate) accent: Color,
    pub(crate) identity: Color,
    pub(crate) success: Color,
    pub(crate) error: Color,
    pub(crate) dim: Color,
    pub(crate) thinking: Color,
    pub(crate) code: Color,
}

impl TuiTheme {
    pub(crate) fn from_profile(profile: TerminalProfile) -> Self {
        let Some(bg) = profile.background else {
            return Self::fallback();
        };
        let light = is_light_rgb(bg);
        let color_level = profile.color_level;
        let accent_rgb = if light { (0, 95, 135) } else { (76, 190, 210) };
        let surface = if light {
            blend_rgb((0, 0, 0), bg, 0.045)
        } else {
            blend_rgb((255, 255, 255), bg, 0.13)
        };
        let menu = if light {
            blend_rgb((0, 0, 0), bg, 0.07)
        } else {
            blend_rgb((255, 255, 255), bg, 0.08)
        };
        let selected = if light {
            blend_rgb(accent_rgb, bg, 0.18)
        } else {
            blend_rgb(accent_rgb, bg, 0.22)
        };
        let field = if light {
            blend_rgb((246, 178, 127), bg, 0.46)
        } else {
            (246, 178, 127)
        };
        Self {
            surface_bg: best_theme_color(surface, color_level).unwrap_or(TUI_ROLE_SURFACE_BG),
            menu_bg: best_theme_color(menu, color_level).unwrap_or(Color::Rgb(16, 16, 20)),
            menu_selected_bg: best_theme_color(selected, color_level)
                .unwrap_or(Color::Rgb(24, 24, 28)),
            selection_bg: best_theme_color(selected, color_level).unwrap_or(TUI_ROLE_SELECTION_BG),
            panel_field_bg: best_theme_color(field, color_level)
                .unwrap_or(Color::Rgb(246, 178, 127)),
            panel_field_fg: if light { Color::Reset } else { Color::Black },
            accent: best_theme_color(accent_rgb, color_level).unwrap_or(TUI_ROLE_ACCENT),
            identity: TUI_ROLE_IDENTITY,
            success: Color::Green,
            error: TUI_ROLE_DANGER,
            dim: TUI_ROLE_DIM,
            thinking: TUI_ROLE_THINKING,
            code: best_theme_color(accent_rgb, color_level).unwrap_or(TUI_ROLE_ACCENT),
        }
    }

    pub(crate) fn fallback() -> Self {
        Self {
            surface_bg: TUI_ROLE_SURFACE_BG,
            menu_bg: Color::Rgb(16, 16, 20),
            menu_selected_bg: Color::Rgb(24, 24, 28),
            selection_bg: TUI_ROLE_SELECTION_BG,
            panel_field_bg: Color::Rgb(246, 178, 127),
            panel_field_fg: Color::Black,
            accent: TUI_ROLE_ACCENT,
            identity: TUI_ROLE_IDENTITY,
            success: Color::Green,
            error: TUI_ROLE_DANGER,
            dim: TUI_ROLE_DIM,
            thinking: TUI_ROLE_THINKING,
            code: TUI_ROLE_ACCENT,
        }
    }

    pub(crate) fn surface_style(self) -> Style {
        Style::default().bg(self.surface_bg)
    }

    pub(crate) fn menu_style(self) -> Style {
        Style::default().bg(self.menu_bg)
    }

    pub(crate) fn selected_row_style(self) -> Style {
        Style::default().fg(self.accent).bg(self.menu_selected_bg)
    }

    pub(crate) fn panel_field_style(self) -> Style {
        Style::default()
            .fg(self.panel_field_fg)
            .bg(self.panel_field_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub(crate) fn accent_style(self) -> Style {
        Style::default().fg(self.accent)
    }

    pub(crate) fn dim_style(self) -> Style {
        Style::default().fg(self.dim)
    }

    pub(crate) fn identity_style(self) -> Style {
        Style::default().fg(self.identity)
    }

    pub(crate) fn success_style(self) -> Style {
        Style::default().fg(self.success)
    }

    pub(crate) fn error_style(self) -> Style {
        Style::default().fg(self.error)
    }

    pub(crate) fn thinking_style(self) -> Style {
        Style::default().fg(self.thinking)
    }

    pub(crate) fn code_style(self) -> Style {
        Style::default().fg(self.code)
    }
}

pub(crate) fn text_selection_style() -> Style {
    Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
}

pub(crate) fn tui_theme() -> TuiTheme {
    static THEME: std::sync::OnceLock<TuiTheme> = std::sync::OnceLock::new();
    *THEME.get_or_init(|| {
        let interactive = std::io::IsTerminal::is_terminal(&std::io::stdout());
        TuiTheme::from_profile(TerminalProfile::detect(interactive))
    })
}

pub(crate) fn terminal_color_level() -> TerminalColorLevel {
    match supports_color::on_cached(supports_color::Stream::Stdout) {
        Some(level) if level.has_16m => TerminalColorLevel::TrueColor,
        Some(level) if level.has_256 => TerminalColorLevel::Ansi256,
        Some(_) => TerminalColorLevel::Ansi16,
        None => TerminalColorLevel::Unknown,
    }
}

pub(crate) fn is_light_rgb((r, g, b): (u8, u8, u8)) -> bool {
    let y = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    y > 128.0
}

pub(crate) fn blend_rgb(fg: (u8, u8, u8), bg: (u8, u8, u8), alpha: f32) -> (u8, u8, u8) {
    let r = f32::from(fg.0) * alpha + f32::from(bg.0) * (1.0 - alpha);
    let g = f32::from(fg.1) * alpha + f32::from(bg.1) * (1.0 - alpha);
    let b = f32::from(fg.2) * alpha + f32::from(bg.2) * (1.0 - alpha);
    (r as u8, g as u8, b as u8)
}

pub(crate) fn best_theme_color(target: (u8, u8, u8), level: TerminalColorLevel) -> Option<Color> {
    match level {
        TerminalColorLevel::TrueColor => Some(Color::Rgb(target.0, target.1, target.2)),
        TerminalColorLevel::Ansi256 => Some(Color::Indexed(nearest_xterm_color(target))),
        TerminalColorLevel::Ansi16 | TerminalColorLevel::Unknown => None,
    }
}

pub(crate) fn nearest_xterm_color(target: (u8, u8, u8)) -> u8 {
    XTERM_256_COLORS
        .iter()
        .enumerate()
        .skip(16)
        .min_by(|(_, a), (_, b)| {
            color_distance(**a, target)
                .partial_cmp(&color_distance(**b, target))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index as u8)
        .unwrap_or(0)
}

pub(crate) fn color_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
    let dr = f32::from(a.0) - f32::from(b.0);
    let dg = f32::from(a.1) - f32::from(b.1);
    let db = f32::from(a.2) - f32::from(b.2);
    (dr * dr + dg * dg + db * db).sqrt()
}

pub(crate) const XTERM_256_COLORS: [(u8, u8, u8); 256] = generate_xterm_256_colors();

pub(crate) const fn generate_xterm_256_colors() -> [(u8, u8, u8); 256] {
    let mut colors = [(0, 0, 0); 256];
    let system = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    let mut index = 0;
    while index < 16 {
        colors[index] = system[index];
        index += 1;
    }
    let levels = [0, 95, 135, 175, 215, 255];
    let mut r = 0;
    while r < 6 {
        let mut g = 0;
        while g < 6 {
            let mut b = 0;
            while b < 6 {
                colors[16 + 36 * r + 6 * g + b] = (levels[r], levels[g], levels[b]);
                b += 1;
            }
            g += 1;
        }
        r += 1;
    }
    let mut gray = 0;
    while gray < 24 {
        let value = 8 + gray as u8 * 10;
        colors[232 + gray] = (value, value, value);
        gray += 1;
    }
    colors
}
