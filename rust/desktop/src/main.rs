#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod editor;

use std::path::PathBuf;

use iced::widget::{
    button, column, container, opaque, pick_list, row, scrollable, space, stack, text, text_input,
};
use iced::{Alignment, Color, Element, Fill, Length, Task, Theme};
use soundtouch_radio_core::{
    ApplyMode, ApplyOutcome, Change, Codec, Error, Host, Presets, Result, SLOTS, Snapshot, Speaker,
    discover, load_backup, save_backup,
};

use editor::{Drafts, Edit};

const PANEL: Color = Color::from_rgb(0.062, 0.067, 0.068);
const CARD: Color = Color::from_rgb(0.102, 0.109, 0.113);
const BORDER: Color = Color::from_rgb(0.23, 0.26, 0.26);
const MUTED: Color = Color::from_rgb(0.67, 0.72, 0.71);
const GREEN: Color = Color::from_rgb(0.42, 0.85, 0.57);
const ERROR: Color = Color::from_rgb(1.0, 0.60, 0.56);
const AMBER: Color = Color::from_rgb(0.96, 0.79, 0.44);

fn main() -> iced::Result {
    iced::application(App::default, App::update, App::view)
        .title("SoundTouch Preset Manager")
        .theme(|_: &App| {
            Theme::custom(
                "SoundTouch",
                iced::theme::Palette {
                    background: Color::from_rgb(0.027, 0.031, 0.032),
                    text: Color::from_rgb(0.93, 0.95, 0.94),
                    primary: GREEN,
                    success: GREEN,
                    warning: AMBER,
                    danger: ERROR,
                },
            )
        })
        .window(iced::window::Settings {
            size: iced::Size::new(660.0, 820.0),
            min_size: Some(iced::Size::new(520.0, 540.0)),
            ..iced::window::Settings::default()
        })
        .run()
}

#[derive(Clone, Debug)]
struct Loaded {
    speaker: Speaker,
    label: String,
    snapshot: Snapshot,
}

impl Loaded {
    fn connect(ip: &str) -> Result<Self> {
        let speaker = Speaker::connect(ip.parse::<Host>()?)?;
        let info = speaker.info().ok();
        let model = info
            .as_ref()
            .map(|info| info.model.as_str())
            .unwrap_or("SoundTouch");
        let name = info
            .as_ref()
            .and_then(|info| info.friendly_name.as_deref())
            .or(speaker.agent().friendly_name.as_deref());
        let label = name.map_or_else(|| model.to_owned(), |name| format!("{name} · {model}"));
        let snapshot = speaker.snapshot()?;
        Ok(Self {
            speaker,
            label,
            snapshot,
        })
    }
}

#[derive(Default)]
struct App {
    ip: String,
    loaded: Option<Loaded>,
    drafts: Drafts,
    expanded: Option<u8>,
    discovered: Vec<String>,
    busy: Option<&'static str>,
    notice: Notice,
    dialog: Option<Dialog>,
    backup_menu: bool,
    reload_required: bool,
}

#[derive(Default)]
enum Notice {
    #[default]
    None,
    Info(String),
    Error(String),
}

enum Dialog {
    Discard(AfterDiscard),
    Apply {
        desired: Presets,
        mode: ApplyMode,
        changes: Vec<Change>,
    },
}

enum AfterDiscard {
    Reload(String),
    Reset,
}

#[derive(Clone, Debug)]
enum Message {
    Ip(String),
    Connect,
    ConnectTo(String),
    Connected(std::result::Result<Box<Loaded>, String>),
    Discover,
    Discovered(Vec<String>, Option<String>),
    Expand(u8),
    Name(u8, String),
    Url(u8, String),
    Codec(u8, Codec),
    Clear(u8),
    Undo(u8),
    Discard,
    ToggleBackup,
    Backup,
    Restore,
    PickedBackup(Option<PathBuf>),
    PickedRestore(Option<PathBuf>),
    Saved(std::result::Result<PathBuf, String>),
    Imported(std::result::Result<Presets, String>),
    Apply,
    Accept,
    Cancel,
    Applied(std::result::Result<ApplyOutcome, String>),
}

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Ip(ip) => self.ip = ip,
            Message::Connect => return self.request_connect(self.ip.clone()),
            Message::ConnectTo(ip) => return self.request_connect(ip),
            Message::Connected(result) => {
                self.busy = None;
                match result {
                    Ok(loaded) => {
                        self.install_snapshot(&loaded.snapshot);
                        self.notice = Notice::Info(format!(
                            "Loaded {} assigned presets.",
                            loaded.snapshot.stored.len()
                        ));
                        self.loaded = Some(*loaded);
                    }
                    Err(error) => {
                        self.loaded = None;
                        self.reload_required = false;
                        self.notice = Notice::Error(error);
                    }
                }
            }
            Message::Discover => {
                if self.busy.is_some() {
                    return Task::none();
                }
                self.busy = Some("Searching the local network…");
                self.discovered.clear();
                return Task::perform(
                    async {
                        let result = tokio::task::spawn_blocking(discover).await;
                        match result {
                            Ok(result) => {
                                let error = result.errors.into_values().next();
                                (result.devices.into_keys().collect(), error)
                            }
                            Err(_) => (Vec::new(), Some("Discovery failed".into())),
                        }
                    },
                    |(devices, error)| Message::Discovered(devices, error),
                );
            }
            Message::Discovered(devices, error) => {
                self.busy = None;
                self.notice = if devices.is_empty() {
                    Notice::Info(error.map_or_else(
                        || "No speakers found. Enter the speaker IP to connect.".into(),
                        |error| format!("{error}. Enter the speaker IP to connect."),
                    ))
                } else {
                    Notice::Info("Choose a speaker or enter its IP.".into())
                };
                self.discovered = devices;
            }
            Message::Expand(slot) => self.expanded = (self.expanded != Some(slot)).then_some(slot),
            Message::Name(slot, name) => self.drafts.name(slot, name),
            Message::Url(slot, url) => self.drafts.url(slot, url),
            Message::Codec(slot, codec) if self.ready() => self.drafts.codec(slot, codec),
            Message::Codec(..) => {}
            Message::Clear(slot) => {
                self.drafts.clear(slot);
                self.expanded = None;
            }
            Message::Undo(slot) => self.drafts.undo(slot),
            Message::Discard => self.dialog = Some(Dialog::Discard(AfterDiscard::Reset)),
            Message::ToggleBackup => self.backup_menu = !self.backup_menu,
            Message::Backup => {
                self.backup_menu = false;
                self.busy = Some("Choose where to save the backup…");
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Save preset backup")
                            .add_filter("JSON backup", &["json"])
                            .set_file_name("presets.json")
                            .save_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    Message::PickedBackup,
                );
            }
            Message::Restore => {
                self.backup_menu = false;
                self.busy = Some("Choose a backup to restore…");
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Open preset backup")
                            .add_filter("JSON backup", &["json"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_owned())
                    },
                    Message::PickedRestore,
                );
            }
            Message::PickedBackup(path) => {
                self.busy = None;
                if let (Some(path), Some(loaded)) = (path, self.loaded.as_ref()) {
                    let speaker = loaded.speaker.clone();
                    self.busy = Some("Backing up the saved presets…");
                    return work(
                        move || {
                            save_backup(&path, &speaker.stored()?)?;
                            Ok(path)
                        },
                        Message::Saved,
                    );
                }
            }
            Message::PickedRestore(path) => {
                self.busy = None;
                if let Some(path) = path {
                    self.busy = Some("Reading the backup…");
                    return work(move || load_backup(&path), Message::Imported);
                }
            }
            Message::Saved(result) => {
                self.busy = None;
                self.notice = match result {
                    Ok(path) => Notice::Info(format!("Backup saved to {}", path.display())),
                    Err(error) => Notice::Error(error),
                };
            }
            Message::Imported(result) => {
                self.busy = None;
                match result {
                    Ok(desired) => self.review(desired, ApplyMode::Restore),
                    Err(error) => self.notice = Notice::Error(error),
                }
            }
            Message::Apply => {
                if let Some(loaded) = &self.loaded {
                    match self.drafts.validate(&loaded.snapshot) {
                        Ok(desired) => self.review(desired, ApplyMode::Edit),
                        Err(_) => {
                            self.notice = Notice::Error(
                                "Fix the highlighted presets before applying changes.".into(),
                            )
                        }
                    }
                }
            }
            Message::Accept => {
                if let Some(dialog) = self.dialog.take() {
                    match dialog {
                        Dialog::Discard(AfterDiscard::Reload(ip)) => return self.begin_connect(ip),
                        Dialog::Discard(AfterDiscard::Reset) => {
                            self.drafts.discard();
                            self.expanded = None;
                            self.notice = Notice::Info("Edits discarded.".into());
                        }
                        Dialog::Apply { desired, mode, .. } => {
                            if let Some(loaded) = &self.loaded {
                                let speaker = loaded.speaker.clone();
                                let baseline = loaded.snapshot.clone();
                                self.busy = Some("Saving and verifying the physical buttons…");
                                return work(
                                    move || {
                                        let folder = dirs::data_local_dir()
                                            .ok_or_else(|| {
                                                Error::Invalid(
                                                    "could not locate the local backup directory"
                                                        .into(),
                                                )
                                            })?
                                            .join("soundtouch-radio")
                                            .join("backups");
                                        std::fs::create_dir_all(&folder).map_err(|e| {
                                            Error::File(
                                                "create recovery backup directory".into(),
                                                e,
                                            )
                                        })?;
                                        speaker.apply(&baseline, &desired, mode, &folder)
                                    },
                                    Message::Applied,
                                );
                            }
                        }
                    }
                }
            }
            Message::Cancel => self.dialog = None,
            Message::Applied(result) => {
                self.busy = None;
                match result {
                    Ok(outcome) => {
                        self.install_snapshot(&outcome.snapshot);
                        if let Some(loaded) = &mut self.loaded {
                            loaded.snapshot = outcome.snapshot;
                        }
                        self.notice = Notice::Info(outcome.recovery.map_or_else(
                            || "The presets already matched.".into(),
                            |path| {
                                format!(
                                    "Changes verified. Previous presets saved to {}.",
                                    path.display()
                                )
                            },
                        ));
                    }
                    Err(error) => {
                        self.reload_required = true;
                        self.notice = Notice::Error(format!(
                            "{error}. Load the presets again to see the current state."
                        ));
                    }
                }
            }
        }
        Task::none()
    }

    fn install_snapshot(&mut self, snapshot: &Snapshot) {
        self.drafts = Drafts::from_snapshot(snapshot);
        self.expanded = None;
        self.reload_required = false;
    }

    fn request_connect(&mut self, ip: String) -> Task<Message> {
        if self.busy.is_some() || self.dialog.is_some() {
            return Task::none();
        }
        if self.drafts.dirty() {
            self.dialog = Some(Dialog::Discard(AfterDiscard::Reload(ip)));
            Task::none()
        } else {
            self.begin_connect(ip)
        }
    }

    fn begin_connect(&mut self, ip: String) -> Task<Message> {
        self.ip = ip.trim().into();
        self.loaded = None;
        self.drafts = Drafts::default();
        self.expanded = None;
        self.backup_menu = false;
        self.notice = Notice::None;
        self.busy = Some("Connecting and loading the six presets…");
        work(
            move || Loaded::connect(&ip).map(Box::new),
            Message::Connected,
        )
    }

    fn review(&mut self, desired: Presets, mode: ApplyMode) {
        let Some(loaded) = &self.loaded else { return };
        match loaded.snapshot.changes(&desired, mode) {
            Ok(changes) if changes.is_empty() => {
                self.notice = Notice::Info("The presets already match.".into())
            }
            Ok(changes) => {
                self.dialog = Some(Dialog::Apply {
                    desired,
                    mode,
                    changes,
                })
            }
            Err(error) => self.notice = Notice::Error(error.to_string()),
        }
    }

    fn target_matches(&self) -> bool {
        self.loaded
            .as_ref()
            .is_some_and(|loaded| self.ip.parse::<Host>().ok() == Some(loaded.speaker.host()))
    }

    fn ready(&self) -> bool {
        self.busy.is_none()
            && self.dialog.is_none()
            && !self.reload_required
            && self.target_matches()
    }

    fn view(&self) -> Element<'_, Message> {
        let enabled = self.busy.is_none() && self.dialog.is_none();
        let ready = self.ready();
        let errors = self
            .loaded
            .as_ref()
            .and_then(|loaded| self.drafts.validate(&loaded.snapshot).err())
            .unwrap_or_default();
        let mismatches = self
            .loaded
            .as_ref()
            .map_or_else(Vec::new, |loaded| loaded.snapshot.mismatches());
        let header = row![
            column![text("SoundTouch").size(34), text("Preset Manager").size(34)].spacing(1),
            space().width(Fill),
            self.status()
        ]
        .align_y(Alignment::Center)
        .spacing(12);
        let target = row![
            text_input("Speaker IP address", &self.ip)
                .padding(11)
                .on_input_maybe(enabled.then_some(Message::Ip as fn(String) -> Message))
                .on_submit_maybe(
                    (enabled && !self.ip.trim().is_empty()).then_some(Message::Connect)
                )
                .width(Fill),
            button("Connect & load presets")
                .padding([10, 14])
                .on_press_maybe(
                    (enabled && !self.ip.trim().is_empty()).then_some(Message::Connect)
                ),
        ]
        .align_y(Alignment::Center)
        .spacing(9);

        let mut search = row![
            button("Search LAN")
                .style(button::text)
                .on_press_maybe(enabled.then_some(Message::Discover))
        ]
        .spacing(8);
        if !self.discovered.is_empty() {
            search = search.push(
                pick_list(&self.discovered[..], None::<String>, Message::ConnectTo)
                    .placeholder("Discovered speakers")
                    .padding([4, 8]),
            );
        }
        let mut content = column![header, target, search].spacing(14);
        for slot in SLOTS {
            let warning = mismatches
                .contains(&slot)
                .then_some("This button's registration differs from the STR preset.");
            content = content.push(
                self.preset_row(
                    slot,
                    ready,
                    errors
                        .get(&slot)
                        .cloned()
                        .or_else(|| warning.map(str::to_owned)),
                ),
            );
        }

        let dirty = self.drafts.dirty();
        let actions = row![
            button("Backup & restore")
                .style(button::secondary)
                .padding([9, 12])
                .on_press_maybe(ready.then_some(Message::ToggleBackup)),
            space().width(Fill),
            button("Discard")
                .style(button::text)
                .padding([9, 12])
                .on_press_maybe((enabled && dirty).then_some(Message::Discard)),
            button("Apply changes")
                .padding([9, 14])
                .on_press_maybe((ready && dirty && errors.is_empty()).then_some(Message::Apply)),
        ]
        .spacing(5)
        .align_y(Alignment::Center);
        if self.backup_menu && ready {
            content = content.push(
                row![
                    button("Save backup…")
                        .style(button::secondary)
                        .on_press(Message::Backup),
                    button("Restore backup…")
                        .style(button::secondary)
                        .on_press(Message::Restore),
                ]
                .spacing(10),
            );
        }
        content = content.push(actions);
        if let Some(busy) = self.busy {
            content = content.push(text(busy).size(14).color(MUTED));
        } else if self.loaded.is_some() && !self.target_matches() {
            content = content.push(
                text("Connect to the entered address to edit its presets.")
                    .size(14)
                    .color(AMBER),
            );
        } else {
            match &self.notice {
                Notice::None => {}
                Notice::Info(message) => {
                    content = content.push(text(message).size(14).color(MUTED))
                }
                Notice::Error(message) => {
                    content = content.push(text(message).size(14).color(ERROR))
                }
            }
        }
        let body = container(scrollable(container(content).padding(26).width(Fill)))
            .width(Fill)
            .height(Fill)
            .style(|_| container::background(PANEL));
        match &self.dialog {
            None => body.into(),
            Some(dialog) => stack![body, self.dialog_view(dialog)].into(),
        }
    }

    fn status(&self) -> Element<'_, Message> {
        match &self.loaded {
            Some(loaded) if !self.reload_required => column![
                container(text("Connected").size(14).color(GREEN))
                    .padding([4, 11])
                    .style(
                        |_| container::background(Color::from_rgb(0.055, 0.15, 0.092))
                            .border(iced::Border::default().rounded(14.0))
                    ),
                text(&loaded.label).size(14).color(MUTED),
                text(format!(
                    "{} · STR {}",
                    loaded.speaker.host(),
                    loaded.speaker.agent().version
                ))
                .size(12)
                .color(MUTED),
            ]
            .spacing(3)
            .into(),
            _ => text(if self.reload_required {
                "Reload required"
            } else if self.busy.is_some() {
                "Working…"
            } else {
                "Not connected"
            })
            .size(14)
            .color(MUTED)
            .into(),
        }
    }

    fn preset_row(&self, slot: u8, ready: bool, error: Option<String>) -> Element<'_, Message> {
        let draft = self.drafts.at(slot);
        let badge = container(text(slot.to_string()).size(24))
            .center_x(46)
            .center_y(44)
            .style(|_| card_style(CARD, BORDER, 9.0));
        let title = if self.loaded.is_none() {
            "—"
        } else if draft.edit == Edit::Clear || draft.fields.name.trim().is_empty() {
            "Empty"
        } else {
            draft.fields.name.as_str()
        };
        let label = if draft.edit != Edit::Keep {
            "Unsaved"
        } else if draft.bose_only {
            "Bose preset"
        } else if self.expanded == Some(slot) {
            "Close"
        } else if ready {
            "Edit"
        } else {
            ""
        };
        let line = row![
            text(title).size(18).width(Fill),
            text(label).size(13).color(if draft.edit != Edit::Keep {
                AMBER
            } else {
                MUTED
            }),
        ]
        .align_y(Alignment::Center)
        .spacing(12);
        let item = button(line)
            .width(Fill)
            .padding([11, 14])
            .style(preset_style)
            .on_press_maybe((ready && !draft.bose_only).then_some(Message::Expand(slot)));
        let mut block =
            column![row![badge, item].align_y(Alignment::Center).spacing(12)].spacing(8);
        if let Some(error) = error.filter(|_| !draft.bose_only) {
            block = block.push(text(error).size(13).color(ERROR));
        }
        if self.expanded == Some(slot) && self.loaded.is_some() && !draft.bose_only {
            let editor = column![
                text_input("Station name", &draft.fields.name)
                    .on_input_maybe(ready.then_some(move |name| Message::Name(slot, name)))
                    .padding(9),
                text_input("Direct HTTP or HTTPS stream URL", &draft.fields.url)
                    .on_input_maybe(ready.then_some(move |url| Message::Url(slot, url)))
                    .padding(9),
                row![
                    text("Codec").size(14).color(MUTED),
                    pick_list(Codec::OPTIONS, Some(draft.fields.codec), move |codec| {
                        Message::Codec(slot, codec)
                    }),
                    space().width(Fill),
                    button("Undo").style(button::text).on_press_maybe(
                        (ready && draft.edit != Edit::Keep).then_some(Message::Undo(slot))
                    ),
                    button("Clear slot").style(button::text).on_press_maybe(
                        (ready
                            && draft.edit != Edit::Clear
                            && (draft.edit == Edit::Replace
                                || self.loaded.as_ref().is_some_and(|loaded| loaded
                                    .snapshot
                                    .stored
                                    .contains_key(&slot))))
                        .then_some(Message::Clear(slot))
                    ),
                ]
                .align_y(Alignment::Center)
                .spacing(8),
            ]
            .spacing(8);
            block = block.push(
                container(editor)
                    .padding(12)
                    .style(|_| card_style(CARD, BORDER, 8.0)),
            );
        }
        block.into()
    }

    fn dialog_view(&self, dialog: &Dialog) -> Element<'_, Message> {
        let (title, content, action): (&str, Element<'_, Message>, &str) = match dialog {
            Dialog::Discard(_) => (
                "Discard unsaved changes?",
                text("Your edits have not been saved to the speaker.").into(),
                "Discard changes",
            ),
            Dialog::Apply {
                desired,
                mode,
                changes,
            } => {
                let mut notes = column![].spacing(9);
                for change in changes {
                    let from = change.before.as_deref().unwrap_or("Empty");
                    let to = change.after.as_deref().unwrap_or("Empty");
                    let from_preset = self
                        .loaded
                        .as_ref()
                        .and_then(|loaded| loaded.snapshot.stored.get(&change.slot));
                    let to_preset = desired.get(&change.slot);
                    let mut detail = Vec::new();
                    if let Some((from, to)) = from_preset.zip(to_preset) {
                        if from.stream_url() != to.stream_url() {
                            detail.push("stream URL changed");
                        }
                        if from.codec_label() != to.codec_label() {
                            detail.push("codec changed");
                        }
                        if from == to {
                            detail.push("register on button");
                        }
                        if from != to && from.name == to.name && detail.is_empty() {
                            detail.push("preset data changed");
                        }
                    }
                    let suffix = if detail.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", detail.join(", "))
                    };
                    let summary = if change.before == change.after {
                        to.to_owned()
                    } else {
                        format!("{from} → {to}")
                    };
                    notes =
                        notes.push(text(format!("{}   {summary}{suffix}", change.slot)).size(15));
                }
                if *mode == ApplyMode::Restore && self.drafts.dirty() {
                    notes = notes.push(
                        text("Restore replaces your unsaved edits.")
                            .size(14)
                            .color(AMBER),
                    );
                }
                (
                    "Confirm preset changes",
                    notes.into(),
                    if *mode == ApplyMode::Restore {
                        "Restore presets"
                    } else {
                        "Apply changes"
                    },
                )
            }
        };
        let buttons = row![
            space().width(Fill),
            button("Cancel")
                .style(button::secondary)
                .on_press(Message::Cancel),
            button(action).on_press(Message::Accept)
        ]
        .spacing(10);
        let card = container(column![text(title).size(23), content, buttons].spacing(17))
            .width(Length::Fill)
            .max_width(465)
            .padding(20)
            .style(|_| card_style(CARD, BORDER, 13.0));
        opaque(
            container(card)
                .center_x(Fill)
                .center_y(Fill)
                .padding(15)
                .style(|_| container::background(Color::from_rgba(0.0, 0.0, 0.0, 0.75))),
        )
    }
}

fn card_style(background: Color, border: Color, radius: f32) -> container::Style {
    container::background(background).border(iced::Border {
        color: border,
        width: 1.0,
        radius: radius.into(),
    })
}

fn preset_style(theme: &Theme, status: button::Status) -> button::Style {
    let (background, border, text) = match status {
        button::Status::Active => (PANEL, BORDER, theme.palette().text),
        button::Status::Hovered | button::Status::Pressed => (CARD, GREEN, theme.palette().text),
        button::Status::Disabled => (PANEL, BORDER, MUTED),
    };
    button::Style {
        background: Some(background.into()),
        text_color: text,
        border: iced::Border {
            color: border,
            width: 1.0,
            radius: 9.0.into(),
        },
        ..button::Style::default()
    }
}

fn work<T: Send + 'static>(
    action: impl FnOnce() -> Result<T> + Send + 'static,
    finished: impl Fn(std::result::Result<T, String>) -> Message + Send + Sync + 'static,
) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(action)
                .await
                .map_err(|_| "The requested operation stopped unexpectedly.".to_string())?
                .map_err(|e| e.to_string())
        },
        finished,
    )
}
