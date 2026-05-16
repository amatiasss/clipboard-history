use cosmic::app::{Core, Settings, Task};
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    Anchor, KeyboardInteractivity, get_layer_surface,
};
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::SctkLayerSurfaceSettings;
use cosmic::iced::runtime::core::layout::Limits;
use cosmic::iced::runtime::core::window::Id as SurfaceId;
use cosmic::iced::widget::scrollable;
use cosmic::iced::{Length, Subscription};
use cosmic::Element;
use fs2::FileExt;
use i18n_embed::{
    fluent::{fluent_language_loader, FluentLanguageLoader},
    DesktopLanguageRequester,
};
use i18n_embed_fl::fl;
use once_cell::sync::Lazy;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(RustEmbed)]
#[folder = "i18n"]
struct Localizations;

static LANGUAGE_LOADER: Lazy<FluentLanguageLoader> = Lazy::new(|| {
    let loader = fluent_language_loader!();
    let requested = DesktopLanguageRequester::requested_languages();
    let _ = i18n_embed::select(&loader, &Localizations, &requested);
    loader
});

macro_rules! fl {
    ($message_id:literal) => {{
        i18n_embed_fl::fl!(&*LANGUAGE_LOADER, $message_id)
    }};
    ($message_id:literal, $($key:ident = $value:expr),+) => {{
        i18n_embed_fl::fl!(&*LANGUAGE_LOADER, $message_id, $($key = $value),+)
    }};
}

const APP_ID: &str = "com.github.clipboard-history.Launcher";
const SCROLL_ID: &str = "launcher-scroll";
const SEARCH_ID: &str = "launcher-search";
const FOCUS_SINK_ID: &str = "launcher-focus-sink";

#[derive(Serialize, Deserialize, Default)]
struct History {
    entries: Vec<String>,
}

fn history_path() -> PathBuf {
    let mut path = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("clipboard-history");
    path.push("history.json");
    path
}

fn load_history() -> History {
    let path = history_path();
    let file = fs::OpenOptions::new().read(true).write(true).create(true).open(&path);
    match file {
        Ok(f) => {
            f.lock_exclusive().ok();
            let result = fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            f.unlock().ok();
            result
        }
        Err(_) => History::default(),
    }
}

fn save_history(history: &History) {
    let path = history_path();
    if let Ok(f) = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&path) {
        f.lock_exclusive().ok();
        if let Ok(json) = serde_json::to_string_pretty(history) {
            fs::write(&path, json).ok();
        }
        f.unlock().ok();
    }
}

fn replace_shortcodes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == ':' {
            if let Some(end) = text[i + 1..].find(':') {
                let shortcode = &text[i + 1..i + 1 + end];
                if !shortcode.is_empty()
                    && shortcode.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '+')
                {
                    if let Some(emoji) = emojis::get_by_shortcode(shortcode) {
                        result.push_str(emoji.as_str());
                        let skip_to = i + 1 + end + 1;
                        while chars.peek().map(|(j, _)| *j < skip_to).unwrap_or(false) {
                            chars.next();
                        }
                        continue;
                    }
                }
            }
        }
        result.push(c);
    }
    result
}

pub struct Launcher {
    core: Core,
    window_id: SurfaceId,
    history: History,
    search: String,
    selected_index: Option<usize>,
    max_entries: usize,
}

#[derive(Clone, Debug)]
pub enum Message {
    Hide,
    Focused,
    SearchChanged(String),
    CopyEntry(usize),
    DeleteEntry(usize),
    KeyUp,
    KeyDown,
    KeyEnter,
    SearchBackspace,
    DeleteSelected,
    QuickCopy(usize), // 1-based position in filtered list
}

impl Launcher {
    fn filtered_entries(&self) -> Vec<(usize, &String)> {
        let search_lower = self.search.to_lowercase();
        self.history
            .entries
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, e)| search_lower.is_empty() || e.to_lowercase().contains(&search_lower))
            .take(self.max_entries)
            .collect()
    }
}

impl cosmic::Application for Launcher {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core { &self.core }
    fn core_mut(&mut self) -> &mut Core { &mut self.core }

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        let window_id = SurfaceId::unique();
        let app = Launcher {
            core,
            window_id,
            history: load_history(),
            search: String::new(),
            selected_index: None,
            max_entries: 20,
        };
        let open = get_layer_surface(SctkLayerSurfaceSettings {
            id: window_id,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            anchor: Anchor::empty(),
            namespace: "clipboard-launcher".into(),
            size: Some((Some(600), Some(500))),
            size_limits: Limits::NONE.min_width(600.0).min_height(100.0).max_width(600.0).max_height(800.0),
            exclusive_zone: -1,
            ..Default::default()
        });
        (app, open)
    }

    fn on_close_requested(&self, _id: SurfaceId) -> Option<Message> {
        None
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Focused => {
                return cosmic::widget::text_input::focus(
                    cosmic::widget::Id::new(SEARCH_ID),
                );
            }
            Message::Hide => {
                return cosmic::iced::exit();
            }
            Message::SearchChanged(s) => {
                self.search = s;
                self.selected_index = None;
                return cosmic::widget::text_input::focus(
                    cosmic::widget::Id::new(SEARCH_ID),
                );
            }
            Message::SearchBackspace => {
                self.search.pop();
                self.selected_index = None;
            }
            Message::KeyDown => {
                let count = self.filtered_entries().len();
                self.selected_index = Some(match self.selected_index {
                    None => 0,
                    Some(i) => (i + 1).min(count.saturating_sub(1)),
                });
                let scroll = scroll_to_selected(self.selected_index, count);
                let sink = cosmic::widget::button::focus(cosmic::widget::Id::new(FOCUS_SINK_ID));
                return Task::batch([scroll, sink]);
            }
            Message::KeyUp => {
                match self.selected_index {
                    None | Some(0) => {
                        self.selected_index = None;
                        return cosmic::widget::text_input::focus(
                            cosmic::widget::Id::new(SEARCH_ID),
                        );
                    }
                    Some(i) => {
                        self.selected_index = Some(i - 1);
                        let count = self.filtered_entries().len();
                        return scroll_to_selected(self.selected_index, count);
                    }
                }
            }
            Message::KeyEnter => {
                if let Some(sel) = self.selected_index {
                    let entries = self.filtered_entries();
                    if let Some((idx, _)) = entries.get(sel) {
                        let idx = *idx;
                        return cosmic::task::message(cosmic::Action::App(Message::CopyEntry(idx)));
                    }
                }
            }
            Message::QuickCopy(pos) => {
                let entries = self.filtered_entries();
                if let Some((idx, _)) = entries.get(pos) {
                    let idx = *idx;
                    return cosmic::task::message(cosmic::Action::App(Message::CopyEntry(idx)));
                }
            }
            Message::DeleteSelected => {
                if let Some(sel) = self.selected_index {
                    let entries = self.filtered_entries();
                    if let Some((idx, _)) = entries.get(sel) {
                        let idx = *idx;
                        self.history.entries.remove(idx);
                        save_history(&self.history);
                        let count = self.filtered_entries().len();
                        self.selected_index = if count == 0 { None } else { Some(sel.min(count - 1)) };
                    }
                }
            }
            Message::DeleteEntry(idx) => {
                if idx < self.history.entries.len() {
                    // Find list position of deleted entry to adjust selected_index
                    let deleted_list_pos = self.filtered_entries()
                        .iter()
                        .position(|(i, _)| *i == idx);
                    self.history.entries.remove(idx);
                    save_history(&self.history);
                    if let (Some(sel), Some(del_pos)) = (self.selected_index, deleted_list_pos) {
                        let count = self.filtered_entries().len();
                        self.selected_index = if count == 0 {
                            None
                        } else if del_pos < sel {
                            Some((sel - 1).min(count - 1))
                        } else {
                            Some(sel.min(count - 1))
                        };
                    }
                }
            }
            Message::CopyEntry(idx) => {
                if let Some(text) = self.history.entries.get(idx) {
                    use std::io::Write;
                    use std::process::{Command, Stdio};
                    let mut cmd = Command::new("wl-copy");
                    cmd.stdin(Stdio::piped());
                    for var in ["WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "DISPLAY"] {
                        if let Ok(val) = std::env::var(var) { cmd.env(var, val); }
                    }
                    if let Ok(mut child) = cmd.spawn() {
                        if let Some(mut stdin) = child.stdin.take() {
                            stdin.write_all(text.as_bytes()).ok();
                        }
                        std::thread::spawn(move || { child.wait().ok(); });
                    }
                }
                return cosmic::iced::exit();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        cosmic::widget::text("").into()
    }

    fn view_window(&self, _id: SurfaceId) -> Element<'_, Message> {
        let entries = self.filtered_entries();

        let mut list_items: Vec<Element<Message>> = Vec::new();
        if entries.is_empty() {
            list_items.push(
                cosmic::widget::container(cosmic::widget::text(fl!("no-history")).size(14))
                    .padding([8, 16])
                    .into(),
            );
        } else {
            for (list_pos, (idx, entry)) in entries.iter().enumerate() {
                let converted = replace_shortcodes(entry);
                let first_line = converted.lines().next().unwrap_or("").chars().take(70).collect::<String>();
                let preview = if converted.lines().count() > 1 || converted.chars().count() > 70 {
                    format!("{}…", first_line)
                } else {
                    first_line
                };

                let row = cosmic::widget::row::with_children(vec![
                    if list_pos >= 1 && list_pos <= 9 {
                        cosmic::widget::text(format!("{}", list_pos))
                            .size(12)
                            .class(cosmic::theme::Text::Color(
                                cosmic::iced::Color { r: 0.5, g: 0.5, b: 0.5, a: 1.0 }
                            ))
                            .into()
                    } else {
                        cosmic::widget::Space::new().width(Length::Fixed(12.0)).into()
                    },
                    cosmic::widget::text(preview).size(14).width(Length::Fill).into(),
                    cosmic::widget::button::icon(
                        cosmic::widget::icon::from_name("edit-delete-symbolic"),
                    )
                    .on_press(Message::DeleteEntry(*idx))
                    .into(),
                ])
                .spacing(8)
                .align_y(cosmic::iced::Alignment::Center)
                .padding([0, 8])
                .width(Length::Fill);

                let is_selected = self.selected_index == Some(list_pos);
                let item = cosmic::widget::button::custom(row)
                    .on_press(Message::CopyEntry(*idx))
                    .width(Length::Fill)
                    .class(if is_selected {
                        cosmic::theme::Button::Suggested
                    } else {
                        cosmic::theme::Button::Custom {
                            active: Box::new(|_, theme| {
                                let cosmic = theme.cosmic();
                                cosmic::widget::button::Style {
                                    border_radius: cosmic.corner_radii.radius_s.into(),
                                    text_color: Some(cosmic.background.component.on.into()),
                                    ..Default::default()
                                }
                            }),
                            disabled: Box::new(|theme| {
                                let cosmic = theme.cosmic();
                                cosmic::widget::button::Style {
                                    border_radius: cosmic.corner_radii.radius_s.into(),
                                    text_color: Some(cosmic.background.component.on.into()),
                                    ..Default::default()
                                }
                            }),
                            hovered: Box::new(|_, theme| {
                                let cosmic = theme.cosmic();
                                cosmic::widget::button::Style {
                                    background: Some(cosmic::iced::Background::Color(cosmic.background.component.hover.into())),
                                    border_radius: cosmic.corner_radii.radius_s.into(),
                                    text_color: Some(cosmic.background.component.on.into()),
                                    ..Default::default()
                                }
                            }),
                            pressed: Box::new(|_, theme| {
                                let cosmic = theme.cosmic();
                                cosmic::widget::button::Style {
                                    background: Some(cosmic::iced::Background::Color(cosmic.background.component.pressed.into())),
                                    border_radius: cosmic.corner_radii.radius_s.into(),
                                    text_color: Some(cosmic.background.component.on.into()),
                                    ..Default::default()
                                }
                            }),
                        }
                    });
                list_items.push(item.into());
            }
        }

        let search_placeholder = fl!("search-placeholder");
        let content = cosmic::widget::column::with_children(vec![
            cosmic::widget::container(
                cosmic::widget::search_input(search_placeholder, &self.search)
                    .on_input(Message::SearchChanged)
                    .id(cosmic::widget::Id::new(SEARCH_ID))
                    .width(Length::Fill),
            )
            .padding([8, 8, 4, 8])
            .width(Length::Fill)
            .into(),
            cosmic::widget::scrollable(
                cosmic::widget::column::with_children(list_items)
                    .padding(8)
                    .spacing(2)
                    .width(Length::Fill),
            )
            .id(cosmic::widget::Id::new(SCROLL_ID))
            .height(Length::Fixed(370.0))
            .width(Length::Fill)
            .into(),
        ])
        .width(Length::Fill);

        let sink = cosmic::widget::button::custom(cosmic::widget::text(""))
            .id(cosmic::widget::Id::new(FOCUS_SINK_ID))
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0));

        cosmic::widget::column::with_children(vec![
            cosmic::widget::container(content)
                .class(cosmic::theme::Container::Background)
                .width(Length::Fill)
                .height(Length::Shrink)
                .into(),
            sink.into(),
        ])
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        cosmic::iced::event::listen_with(|event, _, _| {
            use cosmic::iced::Event;
            use cosmic::iced::keyboard::{Event as KeyEvent, Key, key::Named};
            use cosmic::iced::runtime::core::event::wayland::LayerEvent;
            use cosmic::iced::runtime::core::event::PlatformSpecific;

            match event {
                Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) => match key {
                    Key::Named(Named::Escape) => Some(Message::Hide),
                    Key::Named(Named::ArrowDown) => Some(Message::KeyDown),
                    Key::Named(Named::ArrowUp) => Some(Message::KeyUp),
                    Key::Named(Named::Enter) => Some(Message::KeyEnter),
                    Key::Named(Named::Backspace) => Some(Message::SearchBackspace),
                    Key::Character(c) if c.as_ref() as &str == "j" && modifiers.control() => Some(Message::KeyDown),
                    Key::Character(c) if c.as_ref() as &str == "k" && modifiers.control() => Some(Message::KeyUp),
                    Key::Character(c) if modifiers.control() => {
                        let s = c.as_ref() as &str;
                        match s {
                            "1" => Some(Message::QuickCopy(1)),
                            "2" => Some(Message::QuickCopy(2)),
                            "3" => Some(Message::QuickCopy(3)),
                            "4" => Some(Message::QuickCopy(4)),
                            "5" => Some(Message::QuickCopy(5)),
                            "6" => Some(Message::QuickCopy(6)),
                            "7" => Some(Message::QuickCopy(7)),
                            "8" => Some(Message::QuickCopy(8)),
                            "9" => Some(Message::QuickCopy(9)),
                            _ => None,
                        }
                    }
                    Key::Character(c) if c.as_ref() as &str == "d" => Some(Message::DeleteSelected),
                    _ => None,
                },
                Event::PlatformSpecific(PlatformSpecific::Wayland(
                    cosmic::iced::runtime::core::event::wayland::Event::Layer(
                        LayerEvent::Unfocused, _, _,
                    ),
                )) => Some(Message::Hide),
                Event::PlatformSpecific(PlatformSpecific::Wayland(
                    cosmic::iced::runtime::core::event::wayland::Event::Layer(
                        LayerEvent::Focused, _, _,
                    ),
                )) => Some(Message::Focused),
                _ => None,
            }
        })
    }
}

fn scroll_to_selected(selected_index: Option<usize>, count: usize) -> Task<Message> {
    let idx = match selected_index {
        Some(i) => i,
        None => return Task::none(),
    };
    if count <= 1 { return Task::none(); }
    let y = idx as f32 / (count - 1) as f32;
    scrollable::snap_to(
        cosmic::widget::Id::new(SCROLL_ID),
        scrollable::RelativeOffset { x: None, y: Some(y) },
    )
}

fn main() -> cosmic::iced::Result {
    cosmic::app::run::<Launcher>(
        Settings::default().no_main_window(true),
        (),
    )
}
