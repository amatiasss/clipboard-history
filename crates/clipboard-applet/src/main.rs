use cosmic::app::{Core, Task};
use cosmic::iced::widget::scrollable;
use cosmic::iced::window::Id;
use cosmic::iced::{Length, Rectangle};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::Element;
use cosmic::cosmic_config::{Config, ConfigGet, ConfigSet};
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

const APP_ID: &str = "com.github.clipboard-history";
const CONFIG_VERSION: u64 = 1;
const DEFAULT_MAX_ENTRIES: usize = 20;
const SCROLL_ID: &str = "clipboard-scroll";

fn private_mode_path() -> PathBuf {
    let mut path = dirs_next::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("clipboard-history");
    path.push(".private");
    path
}

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
    fs::read_to_string(history_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_history(history: &History) {
    if let Ok(json) = serde_json::to_string_pretty(history) {
        fs::write(history_path(), json).ok();
    }
}

fn load_max_entries() -> usize {
    Config::new(APP_ID, CONFIG_VERSION)
        .ok()
        .and_then(|c| c.get::<usize>("max_entries").ok())
        .unwrap_or(DEFAULT_MAX_ENTRIES)
}

fn replace_shortcodes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == ':' {
            if let Some(end) = text[i + 1..].find(':') {
                let shortcode = &text[i + 1..i + 1 + end];
                if !shortcode.is_empty() && shortcode.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '+') {
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

pub struct Window {
    core: Core,
    popup: Option<Id>,
    history: History,
    max_entries: usize,
    private_mode: bool,
    selected_index: Option<usize>,
    search: String,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            core: Core::default(),
            popup: None,
            history: History::default(),
            max_entries: load_max_entries(),
            private_mode: private_mode_path().exists(),
            selected_index: None,
            search: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    PopupClosed(Id),
    Surface(cosmic::surface::Action),
    TogglePopup,
    CopyEntry(usize),
    DeleteEntry(usize),
    DeleteAll,
    TogglePrivateMode(bool),
    KeyUp,
    KeyDown,
    KeyEnter,
    SearchChanged(String),
    SearchAppend(String),
    SearchBackspace,
    FocusSearch,
}

impl cosmic::Application for Window {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core { &self.core }
    fn core_mut(&mut self) -> &mut Core { &mut self.core }

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        (Window { core, ..Default::default() }, Task::none())
    }

    fn on_close_requested(&self, id: cosmic::iced_runtime::core::window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                    self.selected_index = None;
                    self.search = String::new();
                }
            }
            Message::Surface(a) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(a),
                ));
            }
            Message::TogglePopup => {
                if let Some(id) = self.popup {
                    return cosmic::task::message(cosmic::Action::Cosmic(
                        cosmic::app::Action::Surface(destroy_popup(id)),
                    ));
                }
                // simulate button press to open popup — reuse the same logic via Surface
                // We can't easily replicate on_press_with_rectangle here, so just do nothing
                // The user can click the icon to open; Super+V closes if open
            }
            Message::FocusSearch => {
                self.selected_index = None;
                self.search = String::new();
            }
            Message::SearchChanged(s) => {
                self.search = s;
                self.selected_index = None;
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
                        std::mem::forget(child);
                    }
                }
                if let Some(id) = self.popup.take() {
                    return cosmic::task::message(cosmic::Action::Cosmic(
                        cosmic::app::Action::Surface(destroy_popup(id)),
                    ));
                }
            }
            Message::DeleteEntry(idx) => {
                if idx < self.history.entries.len() {
                    self.history.entries.remove(idx);
                    save_history(&self.history);
                }
            }
            Message::DeleteAll => {
                if self.search.is_empty() {
                    self.history.entries.clear();
                } else {
                    let search_lower = self.search.to_lowercase();
                    self.history.entries.retain(|e| !e.to_lowercase().contains(&search_lower));
                    self.search = String::new();
                    self.selected_index = None;
                }
                save_history(&self.history);
                if let Some(id) = self.popup.take() {
                    return cosmic::task::message(cosmic::Action::Cosmic(
                        cosmic::app::Action::Surface(destroy_popup(id)),
                    ));
                }
            }
            Message::TogglePrivateMode(enabled) => {
                self.private_mode = enabled;
                let path = private_mode_path();
                if enabled {
                    fs::write(&path, "").ok();
                } else {
                    fs::remove_file(&path).ok();
                }
            }
            Message::SearchAppend(s) => {
                if self.popup.is_some() {
                    self.search.push_str(&s);
                    self.selected_index = None;
                }
            }
            Message::SearchBackspace => {
                if self.popup.is_some() {
                    self.search.pop();
                    self.selected_index = None;
                }
            }
            Message::KeyDown => {
                if self.popup.is_some() {
                    let search_lower = self.search.to_lowercase();
                    let count = self.history.entries.iter()
                        .filter(|e| search_lower.is_empty() || e.to_lowercase().contains(&search_lower))
                        .count()
                        .min(self.max_entries);
                    self.selected_index = Some(match self.selected_index {
                        None => 0,
                        Some(i) => (i + 1).min(count.saturating_sub(1)),
                    });
                    return scroll_to_selected(self.selected_index, count);
                }
            }
            Message::KeyUp => {
                if self.popup.is_some() {
                    match self.selected_index {
                        None => {}
                        Some(0) => {
                            self.selected_index = None;
                        }
                        Some(i) => {
                            self.selected_index = Some(i - 1);
                            let search_lower = self.search.to_lowercase();
                            let count = self.history.entries.iter()
                                .filter(|e| search_lower.is_empty() || e.to_lowercase().contains(&search_lower))
                                .count()
                                .min(self.max_entries);
                            return scroll_to_selected(self.selected_index, count);
                        }
                    }
                }
            }
            Message::KeyEnter => {
                if self.popup.is_some() {
                    if let Some(sel) = self.selected_index {
                        let search_lower = self.search.to_lowercase();
                        let entries: Vec<(usize, &String)> = self.history.entries
                            .iter().enumerate().rev()
                            .filter(|(_, e)| search_lower.is_empty() || e.to_lowercase().contains(&search_lower))
                            .take(self.max_entries)
                            .collect();
                        if let Some((idx, _)) = entries.get(sel) {
                            return cosmic::task::message(cosmic::Action::App(Message::CopyEntry(*idx)));
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<Message> {
        let have_popup = self.popup;
        let btn = self
            .core
            .applet
            .icon_button("edit-paste-symbolic")
            .on_press_with_rectangle(move |offset, bounds| {
                if let Some(id) = have_popup {
                    Message::Surface(destroy_popup(id))
                } else {
                    Message::Surface(app_popup::<Window>(
                        move |state: &mut Window| {
                            state.history = load_history();
                            state.max_entries = load_max_entries();
                            state.selected_index = None;
                            state.search = String::new();
                            let new_id = Id::unique();
                            state.popup = Some(new_id);
                            let mut popup_settings = state.core.applet.get_popup_settings(
                                state.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            );
                            popup_settings.positioner.size = Some((600, 400));
                            popup_settings.positioner.anchor_rect = Rectangle {
                                x: (bounds.x - offset.x) as i32,
                                y: (bounds.y - offset.y) as i32,
                                width: bounds.width as i32,
                                height: bounds.height as i32,
                            };
                            popup_settings
                        },
                        None,
                    ))
                }
            });

        let tooltip: &'static str = Box::leak(fl!("applet-tooltip").into_boxed_str());
        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            tooltip,
            self.popup.is_some(),
            Message::Surface,
            None,
        ))
    }

    fn view_window(&self, _id: Id) -> Element<Message> {
        let search_lower = self.search.to_lowercase();
        let entries: Vec<(usize, &String)> = self.history.entries
            .iter().enumerate().rev()
            .filter(|(_, e)| search_lower.is_empty() || e.to_lowercase().contains(&search_lower))
            .take(self.max_entries)
            .collect();

        let mut list_items: Vec<Element<Message>> = Vec::new();

        if entries.is_empty() {
            list_items.push(
                cosmic::widget::container(
                    cosmic::widget::text(fl!("no-history")).size(14)
                )
                .padding([8, 16])
                .into(),
            );
        } else {
            for (list_pos, (idx, entry)) in entries.iter().enumerate() {
                let converted = replace_shortcodes(entry);
                let first_line = converted.lines().next().unwrap_or("").chars().take(35).collect::<String>();
                let preview = if converted.lines().count() > 1 || converted.chars().count() > 35 {
                    format!("{}…", first_line)
                } else {
                    first_line
                };

                let row = cosmic::widget::row::with_children(vec![
                    cosmic::widget::text(preview).size(14).width(Length::Fill).into(),
                    cosmic::widget::button::icon(
                        cosmic::widget::icon::from_name("edit-delete-symbolic"),
                    )
                    .on_press(Message::DeleteEntry(*idx))
                    .into(),
                ])
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

        let bottom_bar = cosmic::widget::row::with_children(vec![
            cosmic::widget::toggler(self.private_mode)
                .on_toggle(Message::TogglePrivateMode)
                .label(fl!("private-mode"))
                .spacing(8)
                .into(),
            cosmic::widget::Space::new().width(Length::Fill).into(),
            cosmic::widget::button::destructive(if self.search.is_empty() {
                fl!("clear-all")
            } else {
                let n = self.history.entries.iter()
                    .filter(|e| e.to_lowercase().contains(&self.search.to_lowercase()))
                    .count()
                    .min(self.max_entries);
                fl!("clear-n", count = n)
            })
            .on_press(Message::DeleteAll)
            .into(),
        ])
        .align_y(cosmic::iced::Alignment::Center);

        let content = cosmic::widget::column::with_children(vec![
            cosmic::widget::container(
                cosmic::widget::search_input(fl!("search-placeholder"), &self.search)
                    .on_input(Message::SearchChanged)
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
            .height(Length::Fixed(340.0))
            .width(Length::Fill)
            .into(),
            cosmic::widget::container(bottom_bar)
                .padding([4, 8, 8, 8])
                .width(Length::Fill)
                .into(),
        ])
        .width(Length::Fill);

        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Message> {
        cosmic::iced::event::listen_with(|event, _, _| {
            use cosmic::iced::Event;
            use cosmic::iced::keyboard::{Event as KeyEvent, Key, Modifiers, key::Named};
            if let Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) = event {
                match key {
                    Key::Named(Named::ArrowDown) => return Some(Message::KeyDown),
                    Key::Named(Named::ArrowUp) => return Some(Message::KeyUp),
                    Key::Named(Named::Enter) => return Some(Message::KeyEnter),
                    Key::Named(Named::Backspace) => return Some(Message::SearchBackspace),
                    Key::Named(_) => return None,
                    Key::Character(c) if c.as_ref() as &str == "k" && modifiers.contains(Modifiers::CTRL) => {
                        return Some(Message::FocusSearch)
                    }
                    Key::Character(c) => return Some(Message::SearchAppend(c.to_string())),
                    _ => return None,
                }
            }
            None
        })
    }

    fn style(&self) -> Option<cosmic::iced_core::theme::Style> {
        Some(cosmic::applet::style())
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
    cosmic::applet::run::<Window>(())
}
