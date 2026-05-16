use cosmic::app::{Core, Task};
use cosmic::iced::widget::scrollable;
use cosmic::iced::window::Id;
use cosmic::iced::{Length, Rectangle};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::Element;
use cosmic::cosmic_config::{Config, ConfigGet};
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

const APP_ID: &str = "com.github.clipboard-history";
const CONFIG_VERSION: u64 = 1;
const DEFAULT_MAX_ENTRIES: usize = 20;
const SCROLL_ID: &str = "clipboard-scroll";
const SEARCH_ID: &str = "clipboard-search";
const FOCUS_SINK_ID: &str = "clipboard-focus-sink";

static TOOLTIP: Lazy<String> = Lazy::new(|| fl!("applet-tooltip"));

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
    let file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&path);
    if let Ok(f) = file {
        f.lock_exclusive().ok();
        if let Ok(json) = serde_json::to_string_pretty(history) {
            fs::write(&path, json).ok();
        }
        f.unlock().ok();
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
    CopyEntry(usize),
    DeleteEntry(usize),
    DeleteAll,
    TogglePrivateMode(bool),
    KeyUp,
    KeyDown,
    KeyEnter,
    SearchChanged(String),
    SearchBackspace,
    DeleteSelected,
    QuickCopy(usize),
}

impl Window {
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
            Message::SearchChanged(s) => {
                self.search = s;
                self.selected_index = None;
                return cosmic::widget::text_input::focus(cosmic::widget::Id::new(SEARCH_ID));
            }
            Message::SearchBackspace => {
                self.search.pop();
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
                        std::thread::spawn(move || { child.wait().ok(); });
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
            Message::KeyDown => {
                if self.popup.is_some() {
                    let count = self.filtered_entries().len();
                    self.selected_index = Some(match self.selected_index {
                        None => 0,
                        Some(i) => (i + 1).min(count.saturating_sub(1)),
                    });
                    let scroll = scroll_to_selected(self.selected_index, count);
                    let sink = cosmic::widget::button::focus(cosmic::widget::Id::new(FOCUS_SINK_ID));
                    return Task::batch([scroll, sink]);
                }
            }
            Message::KeyUp => {
                if self.popup.is_some() {
                    match self.selected_index {
                        None | Some(0) => {
                            self.selected_index = None;
                            return cosmic::widget::text_input::focus(cosmic::widget::Id::new(SEARCH_ID));
                        }
                        Some(i) => {
                            self.selected_index = Some(i - 1);
                            let count = self.filtered_entries().len();
                            return scroll_to_selected(self.selected_index, count);
                        }
                    }
                }
            }
            Message::KeyEnter => {
                if self.popup.is_some() {
                    if let Some(sel) = self.selected_index {
                        let entries = self.filtered_entries();
                        if let Some((idx, _)) = entries.get(sel) {
                            let idx = *idx;
                            return cosmic::task::message(cosmic::Action::App(Message::CopyEntry(idx)));
                        }
                    }
                }
            }
            Message::QuickCopy(pos) => {
                if self.popup.is_some() {
                    let entries = self.filtered_entries();
                    if let Some((idx, _)) = entries.get(pos) {
                        let idx = *idx;
                        return cosmic::task::message(cosmic::Action::App(Message::CopyEntry(idx)));
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let have_popup = self.popup;
        let btn = self
            .core
            .applet
            .icon_button("edit-paste-symbolic")
            .on_press_with_rectangle(move |offset, bounds| {
                let rx = (bounds.x - offset.x) as i32;
                let ry = (bounds.y - offset.y) as i32;
                let rw = bounds.width as i32;
                let rh = bounds.height as i32;
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
                            let parent = state.core.main_window_id().unwrap();
                            let mut popup_settings = state.core.applet.get_popup_settings(
                                parent,
                                new_id,
                                None,
                                None,
                                None,
                            );
                            popup_settings.positioner.size = Some((600, 400));
                            popup_settings.positioner.anchor_rect = Rectangle {
                                x: rx, y: ry, width: rw, height: rh,
                            };
                            popup_settings
                        },
                        None,
                    ))
                }
            });

        let tooltip: &str = &*TOOLTIP;
        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            tooltip,
            self.popup.is_some(),
            Message::Surface,
            None,
        ))
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        let entries = self.filtered_entries();

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

        let sink = cosmic::widget::button::custom(cosmic::widget::text(""))
            .id(cosmic::widget::Id::new(FOCUS_SINK_ID))
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0));

        let content = cosmic::widget::column::with_children(vec![
            cosmic::widget::container(
                cosmic::widget::search_input(fl!("search-placeholder"), &self.search)
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
            .height(Length::Fixed(340.0))
            .width(Length::Fill)
            .into(),
            cosmic::widget::container(bottom_bar)
                .padding([4, 8, 8, 8])
                .width(Length::Fill)
                .into(),
            sink.into(),
        ])
        .width(Length::Fill);

        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> cosmic::iced::Subscription<Message> {
        cosmic::iced::event::listen_with(|event, _, _| {
            use cosmic::iced::Event;
            use cosmic::iced::keyboard::{Event as KeyEvent, Key, key::Named};
            if let Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, .. }) = event {
                match key {
                    Key::Named(Named::ArrowDown) => return Some(Message::KeyDown),
                    Key::Named(Named::ArrowUp) => return Some(Message::KeyUp),
                    Key::Named(Named::Enter) => return Some(Message::KeyEnter),
                    Key::Named(Named::Backspace) => return Some(Message::SearchBackspace),
                    Key::Character(c) if c.as_ref() as &str == "j" && modifiers.control() => {
                        return Some(Message::KeyDown)
                    }
                    Key::Character(c) if c.as_ref() as &str == "k" && modifiers.control() => {
                        return Some(Message::KeyUp)
                    }
                    Key::Character(c) if modifiers.control() => {
                        let s = c.as_ref() as &str;
                        return match s {
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
                        };
                    }
                    Key::Character(c) if c.as_ref() as &str == "d" => {
                        return Some(Message::DeleteSelected)
                    }
                    _ => {}
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
