use ratatui::{
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType},
};
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub name: Name,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Name {
    #[default]
    SolarizedDark,
    SolarizedLight,
    Nord,
    Terminal,
}

// Semantic roles keep views independent of any particular theme's colors.
pub struct Palette {
    pub background: Color,
    pub foreground: Color,
    pub surface: Color,
    pub muted: Color,
    pub border: Color,
    pub accent: Color,
    pub selected_text: Color,
    pub error: Color,
    pub success: Color,
    pub running: Color,
    pub warning: Color,
    pub canceled: Color,
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Theme {
    pub fn palette(&self) -> Palette {
        match self.name {
            Name::SolarizedDark => Palette {
                background: rgb(0x002b36),
                foreground: rgb(0x839496),
                surface: rgb(0x073642),
                muted: rgb(0x657b83),
                border: rgb(0x586e75),
                accent: rgb(0x268bd2),
                selected_text: rgb(0x93a1a1),
                error: rgb(0xdc322f),
                success: rgb(0x859900),
                running: rgb(0x268bd2),
                warning: rgb(0xb58900),
                canceled: rgb(0x657b83),
            },
            Name::SolarizedLight => Palette {
                background: rgb(0xfdf6e3),
                foreground: rgb(0x657b83),
                surface: rgb(0xeee8d5),
                muted: rgb(0x839496),
                border: rgb(0x93a1a1),
                accent: rgb(0x268bd2),
                selected_text: rgb(0x586e75),
                error: rgb(0xdc322f),
                success: rgb(0x859900),
                running: rgb(0x268bd2),
                warning: rgb(0xb58900),
                canceled: rgb(0x839496),
            },
            Name::Nord => Palette {
                background: rgb(0x2e3440),
                foreground: rgb(0xd8dee9),
                surface: rgb(0x3b4252),
                muted: rgb(0x81a1c1),
                border: rgb(0x4c566a),
                accent: rgb(0x88c0d0),
                selected_text: rgb(0xeceff4),
                error: rgb(0xbf616a),
                success: rgb(0xa3be8c),
                running: rgb(0x81a1c1),
                warning: rgb(0xebcb8b),
                canceled: rgb(0xd8dee9),
            },
            Name::Terminal => Palette {
                background: Color::Reset,
                foreground: Color::Reset,
                surface: Color::DarkGray,
                muted: Color::Gray,
                border: Color::DarkGray,
                accent: Color::Cyan,
                selected_text: Color::White,
                error: Color::Red,
                success: Color::Green,
                running: Color::Blue,
                warning: Color::Yellow,
                canceled: Color::Gray,
            },
        }
    }
}

impl Palette {
    pub fn workflow_status(&self, status: &str) -> Style {
        let color = match status {
            "Completed" | "Continued as new" => self.success,
            "Running" => self.running,
            "Timed out" | "Failed" => self.error,
            "Canceled" => self.canceled,
            "Terminated" => self.warning,
            _ => self.foreground,
        };
        // Foreground only: retain the selected row's background and emphasis.
        Style::default().fg(color)
    }

    pub fn base(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
    }

    pub fn surface(&self) -> Style {
        self.base().bg(self.surface)
    }

    pub fn muted(&self) -> Style {
        self.base().fg(self.muted)
    }

    pub fn heading(&self) -> Style {
        self.base().add_modifier(Modifier::BOLD)
    }

    pub fn selected_header(&self) -> Style {
        self.surface()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    pub fn selection(&self) -> Style {
        self.surface()
            .fg(self.selected_text)
            .add_modifier(Modifier::BOLD)
    }

    pub fn error(&self) -> Style {
        self.base().fg(self.error)
    }

    pub fn border(&self, focused: bool) -> Style {
        Style::default().fg(if focused { self.accent } else { self.border })
    }

    pub fn block(&self) -> Block<'static> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .style(self.base())
            .border_style(self.border(false))
            .title_style(self.heading())
    }
}
