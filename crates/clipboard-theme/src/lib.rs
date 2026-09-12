// clipboard-theme: helpers de estilo compartilhados entre clipboard-applet e clipboard-launcher.

use cosmic::cosmic_config::{Config, ConfigGet};
use cosmic::iced::Color;
use cosmic::theme::Container;
use cosmic::widget::container;
use cosmic::Element;

/// Opacidade padrão do efeito glass (0.0 = sólido, 1.0 = invisível).
pub const DEFAULT_GLASS_OPACITY: f32 = 0.0;

// Cores do tema do usuário — hardcoded por não estarem disponíveis via cosmic-config
// em tempo de execução (o cosmic-settings-daemon não regrava o arquivo de tema no disco
// ao aplicar customizações, servindo apenas via D-Bus, o que não chega ao nosso app
// antes do primeiro render). Upgrade: remover quando o daemon persistir corretamente.
// ponytail: workaround de cor fixa — teto: depende do cosmic-settings-daemon persistir
// o tema no disco. Quando corrigido, substituir pelas cores do tema via theme.cosmic().
pub const COLOR_BG: Color       = Color { r: 0.227, g: 0.227, b: 0.227, a: 1.0 }; // #3A3A3A
pub const COLOR_ACCENT: Color   = Color { r: 0.404, g: 0.400, b: 0.396, a: 1.0 }; // #676665
pub const COLOR_TEXT: Color     = Color { r: 1.0,   g: 1.0,   b: 1.0,   a: 1.0 }; // #FFFFFF
pub const COLOR_CONTROL: Color  = Color { r: 0.467, g: 0.467, b: 0.467, a: 1.0 }; // #777777

/// Envolve `content` em um container com o estilo visual do tema do usuário.
///
/// `opacity` controla a transparência do fundo [0.0 = sólido, 1.0 = invisível].
///
/// ponytail: transparência simples via canal alpha — sem blur real,
/// pois `set_blur` no backend Wayland do libcosmic (commit adb3e34,
/// `iced/winit/src/platform_specific/wayland/winit_window.rs:280`) é um `// TODO`
/// vazio, e o launcher usa layer-surface SCTK que não passa por `winit::window::Action`.
/// Upgrade: quando `cosmic-comp` expuser blur via protocolo Wayland e libcosmic
/// implementar `EnableBlur` para layer-surfaces, trocar aqui pela chamada à API de blur
/// mantendo este fallback de cor como fallback secundário.
pub fn glass_container<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    opacity: f32,
) -> Element<'a, Message> {
    let opacity = clamp_opacity(opacity);
    container(content)
        .class(Container::Custom(Box::new(move |theme: &cosmic::Theme| {
            let cosmic = theme.cosmic();
            cosmic::iced_widget::container::Style {
                icon_color: Some(COLOR_TEXT),
                text_color: Some(COLOR_TEXT),
                background: Some(cosmic::iced::Background::Color(Color {
                    a: COLOR_BG.a * (1.0 - opacity),
                    ..COLOR_BG
                })),
                border: cosmic::iced::Border {
                    radius: cosmic.corner_radii.radius_s.into(),
                    width: 1.0,
                    color: Color { a: 0.15, ..COLOR_CONTROL },
                },
                shadow: cosmic::iced_core::Shadow::default(),
                snap: true,
            }
        })))
        .into()
}

/// Container para o popup do applet — replica o popup_container do libcosmic
/// mas com as cores corretas do tema do usuário.
pub fn popup_container<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    opacity: f32,
) -> Element<'a, Message> {
    let opacity = clamp_opacity(opacity);
    container(content)
        .class(Container::Custom(Box::new(move |theme: &cosmic::Theme| {
            let cosmic = theme.cosmic();
            cosmic::iced_widget::container::Style {
                icon_color: Some(COLOR_TEXT),
                text_color: Some(COLOR_TEXT),
                background: Some(cosmic::iced::Background::Color(Color {
                    a: COLOR_BG.a * (1.0 - opacity),
                    ..COLOR_BG
                })),
                border: cosmic::iced::Border {
                    radius: cosmic.corner_radii.radius_m.into(),
                    width: 1.0,
                    color: Color { a: 0.2, ..COLOR_CONTROL },
                },
                shadow: cosmic::iced_core::Shadow::default(),
                snap: true,
            }
        })))
        .width(cosmic::iced::Length::Shrink)
        .into()
}

/// Retorna o estilo customizado para o campo de busca, substituindo a cor de
/// destaque do tema (que seria aplicada na borda de foco) pela nossa COLOR_ACCENT.
pub fn search_input_style() -> cosmic::theme::TextInput {
    use cosmic::theme::TextInput;
    use cosmic::widget::text_input::Appearance;

    fn base_appearance(theme: &cosmic::Theme) -> Appearance {
        let palette = theme.cosmic();
        let container = theme.current_container();
        let mut bg: cosmic::iced::Color = container.small_widget.into();
        bg.a = 0.25;
        Appearance {
            background: bg.into(),
            border_radius: palette.corner_radii.radius_xl.into(),
            border_width: 2.0,
            border_offset: None,
            border_color: cosmic::iced::Color {
                a: 0.3,
                ..COLOR_CONTROL
            },
            icon_color: None,
            text_color: Some(COLOR_TEXT),
            placeholder_color: cosmic::iced::Color { a: 0.5, ..COLOR_TEXT },
            selected_text_color: COLOR_TEXT,
            selected_fill: COLOR_ACCENT,
            label_color: COLOR_CONTROL,
        }
    }

    TextInput::Custom {
        active: Box::new(|theme| base_appearance(theme)),
        error: Box::new(|theme| base_appearance(theme)),
        hovered: Box::new(|theme| {
            let mut a = base_appearance(theme);
            a.border_color = cosmic::iced::Color { a: 0.5, ..COLOR_CONTROL };
            a
        }),
        focused: Box::new(|theme| {
            let mut a = base_appearance(theme);
            // borda de foco com COLOR_ACCENT em vez do roxo do tema
            a.border_color = COLOR_ACCENT;
            a.border_offset = Some(2.0);
            a
        }),
        disabled: Box::new(|theme| {
            let mut a = base_appearance(theme);
            a.text_color = Some(cosmic::iced::Color { a: 0.4, ..COLOR_TEXT });
            a
        }),
    }
}
/// Retorna [`DEFAULT_GLASS_OPACITY`] se a chave não existir ou a leitura falhar.
/// O valor lido é sempre clampado em [0.0, 1.0].
pub fn load_glass_opacity(app_id: &str, config_version: u64) -> f32 {
    Config::new(app_id, config_version)
        .ok()
        .and_then(|c| c.get::<f32>("glass_opacity").ok())
        .map(clamp_opacity)
        .unwrap_or(DEFAULT_GLASS_OPACITY)
}

/// Clamp defensivo — extraído para ser testável sem instanciar Theme.
#[inline]
pub(crate) fn clamp_opacity(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_rejeita_negativos() {
        assert_eq!(clamp_opacity(-1.0), 0.0);
    }

    #[test]
    fn clamp_rejeita_acima_de_um() {
        assert_eq!(clamp_opacity(2.0), 1.0);
    }

    #[test]
    fn clamp_preserva_intervalo_valido() {
        assert_eq!(clamp_opacity(0.0), 0.0);
        assert_eq!(clamp_opacity(0.5), 0.5);
        assert_eq!(clamp_opacity(1.0), 1.0);
    }

    #[test]
    fn default_opacity_dentro_do_intervalo() {
        assert!(DEFAULT_GLASS_OPACITY >= 0.0 && DEFAULT_GLASS_OPACITY <= 1.0);
    }

    #[test]
    fn default_opacity_e_solido() {
        assert_eq!(DEFAULT_GLASS_OPACITY, 0.0);
    }

    #[test]
    fn cores_hardcoded_dentro_do_intervalo() {
        for c in [COLOR_BG, COLOR_ACCENT, COLOR_TEXT, COLOR_CONTROL] {
            assert!(c.r >= 0.0 && c.r <= 1.0);
            assert!(c.g >= 0.0 && c.g <= 1.0);
            assert!(c.b >= 0.0 && c.b <= 1.0);
            assert!(c.a >= 0.0 && c.a <= 1.0);
        }
    }
}
