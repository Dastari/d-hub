//! Semantic palette and per-source colours shared by every surface.
use super::*;

pub(super) const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub(super) bg: Color,
    pub(super) surface: Color,
    pub(super) fg: Color,
    pub(super) muted: Color,
    pub(super) accent: Color,
    pub(super) second: Color,
    pub(super) selected: Color,
    pub(super) warning: Color,
    pub(super) success: Color,
}

pub(super) fn palette(theme: Theme, elapsed: Duration, reduced: bool) -> Palette {
    let (base, alternate) = match theme {
        Theme::Midnight => ([145, 184, 255], [207, 163, 255]),
        Theme::Dusk => ([222, 174, 255], [255, 181, 164]),
    };
    let mix = if reduced {
        0.0
    } else {
        (elapsed.as_secs_f64() * std::f64::consts::TAU / 12.0).sin() * 0.5 + 0.5
    };
    let channel = |i| (f64::from(base[i]) * (1.0 - mix) + f64::from(alternate[i]) * mix) as u8;
    Palette {
        bg: if theme == Theme::Midnight {
            Color::Rgb(17, 19, 24)
        } else {
            Color::Rgb(27, 22, 33)
        },
        surface: if theme == Theme::Midnight {
            Color::Rgb(25, 29, 38)
        } else {
            Color::Rgb(39, 31, 45)
        },
        fg: Color::Rgb(230, 234, 242),
        muted: Color::Rgb(153, 164, 184),
        accent: Color::Rgb(channel(0), channel(1), channel(2)),
        second: Color::Rgb(191, 166, 232),
        selected: if theme == Theme::Midnight {
            Color::Rgb(35, 46, 64)
        } else {
            Color::Rgb(61, 42, 66)
        },
        warning: Color::Rgb(244, 199, 139),
        success: Color::Rgb(134, 214, 160),
    }
}

pub(super) fn source_color(source: SourceFilter, fallback: Color) -> Color {
    match source {
        SourceFilter::Discord => Color::Rgb(88, 101, 242),
        SourceFilter::Teams => Color::Rgb(139, 131, 224),
        SourceFilter::ICloud => Color::Rgb(60, 163, 255),
        SourceFilter::Office365 => Color::Rgb(244, 123, 75),
        SourceFilter::Email => Color::Rgb(109, 202, 196),
        SourceFilter::All => fallback,
    }
}

pub(super) fn provider_color(provider: Option<Provider>, fallback: Color) -> Color {
    source_color(
        match provider {
            Some(Provider::Discord) => SourceFilter::Discord,
            Some(Provider::Teams) => SourceFilter::Teams,
            Some(Provider::ICloud) => SourceFilter::ICloud,
            Some(Provider::Office365) => SourceFilter::Office365,
            None => SourceFilter::All,
        },
        fallback,
    )
}

pub(super) fn source_selection(color: Color, bg: Color) -> Color {
    match (color, bg) {
        (Color::Rgb(r, g, b), Color::Rgb(br, bg, bb)) => Color::Rgb(
            ((r as u16 + br as u16 * 4) / 5) as u8,
            ((g as u16 + bg as u16 * 4) / 5) as u8,
            ((b as u16 + bb as u16 * 4) / 5) as u8,
        ),
        _ => bg,
    }
}
