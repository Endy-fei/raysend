//! 原生桌面端（iced）：Windows / Linux / macOS，无 WebView。
//!
//! 布局与配色对齐网页端；编解码走 `raysend-core`。

mod camera;
#[cfg(windows)]
mod dshow;
mod fonts;
mod i18n;
mod theme;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use camera::{CamEvent, CameraChoice};
use i18n::Lang;
use iced::widget::image::Handle;
use iced::widget::{
    button, column, container, image, mouse_area, pick_list, progress_bar, row, scrollable, text,
    Space,
};
use iced::{
    event, window, Alignment, Element, Event, Fill, Length, Padding, Subscription, Task, Theme,
};
use raysend_core::session::{Outgoing, PrepareError, MAX_FILE_SIZE};
use raysend_core::{
    clamp_grid, compose_qr_grid_density, format_bytes, grid_dims, Decoder, Density, TransferReceipt,
};
use theme::Mode;

/// 编码器不能放进 iced Message（不必 Clone）；准备线程写到这里，主线程再取走。
static PENDING: Mutex<Option<Outgoing>> = Mutex::new(None);

fn main() -> iced::Result {
    let mut app = iced::application(App::new, App::update, App::view)
        .title("RaySend · 光传")
        .subscription(App::subscription)
        .theme(App::theme)
        .style(|app, _| app.mode.appearance())
        .window_size(iced::Size::new(980.0, 820.0))
        .default_font(fonts::default_font());
    if let Some(bytes) = fonts::load_cjk_bytes() {
        app = app.font(bytes);
    }
    app.run()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Send,
    Receive,
}

#[derive(Clone, Debug)]
struct SessionMeta {
    file_name: String,
    orig_size: u64,
    compressed_size: u64,
    symbol_mtu: u16,
    qr_version: i16,
}

struct SavedFile {
    name: String,
    size: u64,
    data: Vec<u8>,
    receipt: Option<TransferReceipt>,
}

struct App {
    lang: Lang,
    mode: Mode,
    tab: Tab,
    status: String,
    preparing: bool,
    session: Option<SessionMeta>,
    outgoing: Option<Outgoing>,
    playing: bool,
    fps: u32,
    grid: u8,
    density: Density,
    fullscreen: bool,
    qr: Option<Handle>,
    qr_slots: Vec<Vec<u8>>,
    cell_cursor: usize,
    decoder: Decoder,
    scanning: bool,
    cam_rx: Option<Receiver<CamEvent>>,
    cam_tx: Option<SyncSender<()>>,
    cameras: Vec<CameraChoice>,
    camera: Option<CameraChoice>,
    preview: Option<Handle>,
    unique: usize,
    needed: usize,
    saved: Option<SavedFile>,
    cam_wait: Option<Instant>,
    transfer_start: Option<Instant>,
    receipt: Option<TransferReceipt>,
}

#[derive(Clone, Debug)]
enum Message {
    Lang(Lang),
    ToggleTheme,
    Tab(Tab),
    PickFile,
    Dropped(PathBuf),
    Prepared(Result<SessionMeta, String>),
    Tick,
    TogglePlay,
    SetFps(u32),
    SetDensity(Density),
    ToggleGrid,
    ToggleFullscreen,
    Back,
    StartCamera,
    StopCamera,
    SelectCamera(CameraChoice),
    CameraTick,
    SaveAgain,
    OpenGitHub,
}

impl App {
    fn new() -> Self {
        let cameras = camera::list_cameras();
        let camera = cameras.first().cloned();
        Self {
            lang: Lang::detect(),
            mode: Mode::Light,
            tab: Tab::Send,
            status: String::new(),
            preparing: false,
            session: None,
            outgoing: None,
            playing: true,
            fps: 60,
            grid: 4,
            density: Density::Fast,
            fullscreen: false,
            qr: None,
            qr_slots: Vec::new(),
            cell_cursor: 0,
            decoder: Decoder::new(),
            scanning: false,
            cam_rx: None,
            cam_tx: None,
            cameras,
            camera,
            preview: None,
            unique: 0,
            needed: 0,
            saved: None,
            cam_wait: None,
            transfer_start: None,
            receipt: None,
        }
    }

    fn theme(&self) -> Theme {
        self.mode.iced_theme()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![event::listen_with(|event, _status, _id| match event {
            Event::Window(window::Event::FileDropped(path)) => Some(Message::Dropped(path)),
            _ => None,
        })];
        if self.session.is_some() && self.playing {
            let ms = (1000 / (self.fps.max(1) * u32::from(self.grid.max(1)))) as u64;
            subs.push(iced::time::every(Duration::from_millis(ms.max(1))).map(|_| Message::Tick));
        }
        if self.scanning {
            subs.push(iced::time::every(Duration::from_millis(50)).map(|_| Message::CameraTick));
        }
        Subscription::batch(subs)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Lang(lang) => self.lang = lang,
            Message::ToggleTheme => self.mode = self.mode.toggle(),
            Message::Tab(tab) => {
                if tab != Tab::Receive {
                    self.stop_camera();
                } else {
                    self.refresh_cameras();
                }
                self.tab = tab;
            }
            Message::PickFile => {
                if self.preparing {
                    return Task::none();
                }
                let path = rfd::FileDialog::new().pick_file();
                if let Some(path) = path {
                    return self.begin_prepare(path);
                }
            }
            Message::Dropped(path) => {
                if self.session.is_none() && self.tab == Tab::Send && !self.preparing {
                    return self.begin_prepare(path);
                }
            }
            Message::Prepared(result) => {
                self.preparing = false;
                match result {
                    Ok(meta) => {
                        self.outgoing = PENDING.lock().ok().and_then(|mut g| g.take());
                        self.session = Some(meta);
                        self.playing = true;
                        self.status.clear();
                        self.tick_qr();
                    }
                    Err(err) => self.status = err,
                }
            }
            Message::Tick => self.tick_qr(),
            Message::TogglePlay => self.playing = !self.playing,
            Message::SetFps(fps) => self.fps = fps,
            Message::SetDensity(density) => {
                self.density = density;
                if let Some(outgoing) = self.outgoing.as_mut() {
                    outgoing.set_density(density);
                    if let Some(session) = self.session.as_mut() {
                        session.symbol_mtu = outgoing.symbol_mtu();
                        session.qr_version = density.qr_version();
                    }
                }
                self.tick_qr();
            }
            Message::ToggleGrid => {
                self.grid = match self.grid {
                    1 => 2,
                    2 => 4,
                    4 => 6,
                    _ => 1,
                };
                self.qr_slots.clear();
                self.cell_cursor = 0;
                self.tick_qr();
            }
            Message::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                self.tick_qr();
            }
            Message::Back => {
                self.session = None;
                self.outgoing = None;
                self.qr = None;
                self.qr_slots.clear();
                self.cell_cursor = 0;
                self.status.clear();
            }
            Message::StartCamera => {
                self.start_camera();
            }
            Message::StopCamera => {
                self.stop_camera();
                self.status = self.lang.t("receive_idle").to_string();
            }
            Message::SelectCamera(choice) => {
                let changed = self.camera.as_ref() != Some(&choice);
                self.camera = Some(choice);
                if changed && self.scanning {
                    self.start_camera();
                }
            }
            Message::CameraTick => self.drain_camera(),
            Message::SaveAgain => {
                if let Some(saved) = &self.saved {
                    save_bytes(&saved.name, &saved.data, self.lang);
                }
            }
            Message::OpenGitHub => {
                let _ = open::that("https://github.com/Endy-fei/raysend");
            }
        }
        Task::none()
    }

    fn begin_prepare(&mut self, path: PathBuf) -> Task<Message> {
        let lang = self.lang;
        self.preparing = true;
        self.status = lang.t("reading").to_string();
        Task::perform(
            async move {
                let (tx, rx) = futures::channel::oneshot::channel();
                let _ = std::thread::Builder::new()
                    .name("raysend-prepare".into())
                    .spawn(move || {
                        let _ = tx.send(prepare_file(path, lang));
                    });
                rx.await.unwrap_or_else(|_| Err(lang.t("empty_file").to_string()))
            },
            Message::Prepared,
        )
    }

    fn tick_qr(&mut self) {
        let Some(session) = self.outgoing.as_mut() else {
            return;
        };
        let n = clamp_grid(self.grid) as usize;
        if self.qr_slots.len() != n {
            self.qr_slots = vec![Vec::new(); n];
            self.cell_cursor = 0;
        }
        if let Some(payload) = session.next_payloads(1).into_iter().next() {
            self.qr_slots[self.cell_cursor] = payload;
            self.cell_cursor = (self.cell_cursor + 1) % n.max(1);
        }
        if self.qr_slots.iter().any(|slot| slot.is_empty()) {
            return;
        }
        let canvas = if self.fullscreen { 900 } else { 680 };
        if let Some((w, h, rgba)) = compose_qr_grid_density(&self.qr_slots, canvas, self.density) {
            self.qr = Some(Handle::from_rgba(w, h, rgba));
        }
    }

    fn refresh_cameras(&mut self) {
        let list = camera::list_cameras();
        if let Some(current) = &self.camera {
            if !list.iter().any(|c| c.index == current.index) {
                self.camera = list.first().cloned();
            } else if let Some(fresh) = list.iter().find(|c| c.index == current.index) {
                self.camera = Some(fresh.clone());
            }
        } else {
            self.camera = list.first().cloned();
        }
        self.cameras = list;
    }

    fn start_camera(&mut self) {
        self.stop_camera();
        self.decoder = Decoder::new();
        self.saved = None;
        self.receipt = None;
        self.transfer_start = None;
        self.unique = 0;
        self.needed = 0;
        self.cam_wait = Some(Instant::now());
        self.status = self.lang.t("cam_requesting").to_string();
        let Some(choice) = self.camera.clone() else {
            self.status = self.lang.t("no_camera").to_string();
            return;
        };
        let (rx, tx) = camera::spawn(choice.index);
        self.cam_rx = Some(rx);
        self.cam_tx = Some(tx);
        self.scanning = true;
    }

    fn stop_camera(&mut self) {
        self.scanning = false;
        self.cam_wait = None;
        if let Some(tx) = self.cam_tx.take() {
            let _ = tx.try_send(());
        }
        self.cam_rx = None;
        self.preview = None;
    }

    fn drain_camera(&mut self) {
        let mut latest = None;
        let mut skipped = 0u64;
        if let Some(rx) = self.cam_rx.as_ref() {
            while let Ok(event) = rx.try_recv() {
                if latest.is_some() {
                    skipped += 1;
                }
                latest = Some(event);
            }
        } else {
            return;
        }
        if latest.is_none() {
            if self.preview.is_none() {
                if let Some(started) = self.cam_wait {
                    if started.elapsed() > Duration::from_secs(6) {
                        self.status = self.lang.t("cam_timeout").to_string();
                        self.stop_camera();
                    }
                }
            }
            return;
        }
        match latest.unwrap() {
            CamEvent::Error(err) => {
                self.status = format!("{} ({err})", self.lang.t("cam_open_failed"));
                self.stop_camera();
            }
            CamEvent::Frame(frame) => {
                self.preview = Some(Handle::from_rgba(
                    frame.preview_w,
                    frame.preview_h,
                    frame.preview_rgba,
                ));
                self.decoder.note_capture();
                self.decoder.note_busy_drops(frame.dropped + skipped);
                self.decoder.add_slice(frame.slice);
                if let Some(camera) = self.camera.as_ref() {
                    self.decoder.stats_mut().camera = camera.label.clone();
                }
                self.decoder.stats_mut().workers = 1;
                let _ = self.decoder.ingest_payloads(&frame.payloads);
                if self.decoder.unique_count() > 0 && self.transfer_start.is_none() {
                    self.transfer_start = Some(Instant::now());
                }
                if self.decoder.legacy_detected() {
                    self.status = self.lang.t("err_legacy").to_string();
                    self.stop_camera();
                    return;
                }
                if let Some(err) = self.decoder.error() {
                    self.status = err.to_string();
                    self.stop_camera();
                    return;
                }
                if self.decoder.is_finished() {
                    let elapsed_ms = self
                        .transfer_start
                        .map(|t| t.elapsed().as_millis() as u64)
                        .unwrap_or(0);
                    let stats = self.decoder.stats().clone();
                    let decoder = std::mem::take(&mut self.decoder);
                    if let Some(finished) = decoder.take_finished() {
                        match finished.decompressed() {
                            Ok(bytes) => {
                                let name = finished.get_name();
                                save_bytes(&name, &bytes, self.lang);
                                let receipt = TransferReceipt {
                                    name: name.clone(),
                                    bytes: bytes.len() as u64,
                                    elapsed_ms,
                                    stats,
                                };
                                self.status =
                                    self.lang.saved(&name, &format_bytes(bytes.len() as u64));
                                self.receipt = Some(receipt.clone());
                                self.saved = Some(SavedFile {
                                    name,
                                    size: bytes.len() as u64,
                                    data: bytes,
                                    receipt: Some(receipt),
                                });
                            }
                            Err(err) => self.status = err,
                        }
                    }
                    self.stop_camera();
                    return;
                }
                self.unique = self.decoder.unique_count();
                self.needed = self.decoder.needed();
                if self.needed == 0 {
                    self.status = self.lang.t("aim").to_string();
                } else {
                    let mtu = u64::from(self.decoder.symbol_mtu());
                    let mut line = self.lang.scanning(
                        &format_bytes(self.unique as u64 * mtu),
                        &format_bytes(self.needed as u64 * mtu),
                    );
                    let elapsed_ms = self
                        .transfer_start
                        .map(|t| t.elapsed().as_millis() as u64)
                        .unwrap_or(0);
                    let live = self
                        .decoder
                        .stats()
                        .live_line(elapsed_ms, matches!(self.lang, Lang::Zh));
                    if !live.is_empty() {
                        line = format!("{line} · {live}");
                    }
                    self.status = line;
                }
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let body = if self.session.is_some() {
            self.view_play()
        } else {
            self.view_home()
        };
        container(scrollable(container(body).width(Length::Fill).max_width(760)).width(Fill))
            .style(theme::shell(self.mode))
            .padding(Padding::from([28, 24]))
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn view_home(&self) -> Element<'_, Message> {
        let l = self.lang;
        let brand = row![
            container(column![
                row![cell(), Space::new().width(4), cell()].align_y(Alignment::Center),
                Space::new().height(4),
                row![cell(), Space::new().width(4), cell()],
            ])
            .width(42)
            .height(42)
            .center_x(Fill)
            .center_y(Fill)
            .style(theme::brand_mark(self.mode)),
            column![
                txt(l.t("brand")).size(28),
                txt(l.t("brand_sub"))
                    .size(12)
                    .style(theme::muted_text(self.mode)),
                txt(l.t("lede"))
                    .size(15)
                    .style(theme::muted_text(self.mode)),
            ]
            .spacing(2),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let actions = row![
            lang_btn(self.mode, self.lang, Lang::En, "EN"),
            lang_btn(self.mode, self.lang, Lang::Zh, "中文"),
            pill_btn(
                self.mode,
                if self.mode == Mode::Dark {
                    l.t("theme_light")
                } else {
                    l.t("theme_dark")
                },
                Message::ToggleTheme,
            ),
            pill_btn(self.mode, "GitHub", Message::OpenGitHub),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let top = row![brand, Space::new().width(Fill), actions]
            .align_y(Alignment::Start)
            .spacing(16);

        let tabs = container(row![
            segment_btn(self.mode, l.t("send"), self.tab == Tab::Send, Message::Tab(Tab::Send)),
            segment_btn(
                self.mode,
                l.t("receive"),
                self.tab == Tab::Receive,
                Message::Tab(Tab::Receive),
            ),
        ]
        .spacing(4))
        .padding(5)
        .width(Fill)
        .style(theme::segmented(self.mode));

        let page = match self.tab {
            Tab::Send => self.view_send(),
            Tab::Receive => self.view_receive(),
        };

        column![top, tabs, page].spacing(18).into()
    }

    fn view_send(&self) -> Element<'_, Message> {
        let l = self.lang;
        let art = column![
            row![qcell(self.mode), qcell(self.mode)].spacing(4),
            row![qcell(self.mode), qcell(self.mode)].spacing(4),
        ]
        .spacing(4);

        let zone = mouse_area(
            container(
                column![
                    art,
                    txt(l.t("choose_file")).size(22),
                    txt(l.t("dropzone_body"))
                        .size(15)
                        .style(theme::muted_text(self.mode)),
                    container(txt(l.t("max_size")).size(12))
                        .padding(Padding::from([4, 10]))
                        .style(theme::pill(self.mode)),
                ]
                .spacing(8)
                .align_x(Alignment::Center),
            )
            .padding(Padding::from([28, 16]))
            .width(Fill)
            .style(theme::dropzone(self.mode)),
        )
        .on_press(Message::PickFile);

        let mut col = column![zone].spacing(12);
        if !self.status.is_empty() {
            col = col.push(txt(&self.status).style(theme::muted_text(self.mode)));
        }
        col = col.push(
            column![
                txt(format!("· {}", l.t("tip_compress"))).style(theme::muted_text(self.mode)),
                txt(format!("· {}", l.t("tip_grid"))).style(theme::muted_text(self.mode)),
                txt(format!("· {}", l.t("tip_fountain"))).style(theme::muted_text(self.mode)),
                txt(format!("· {}", l.t("tip_slow"))).style(theme::muted_text(self.mode)),
            ]
            .spacing(6),
        );

        container(col)
            .padding(18)
            .width(Fill)
            .style(theme::card(self.mode))
            .into()
    }

    fn view_receive(&self) -> Element<'_, Message> {
        let l = self.lang;
        let preview: Element<_> = if let Some(handle) = &self.preview {
            image(handle)
                .width(Fill)
                .height(Length::Fixed(420.0))
                .content_fit(iced::ContentFit::Cover)
                .into()
        } else {
            container(Space::new().width(Fill).height(420))
                .width(Fill)
                .height(420)
                .style(theme::video_wrap())
                .into()
        };

        let video = container(preview)
            .width(Fill)
            .height(420)
            .style(theme::video_wrap());

        let stack: Element<_> = if self.scanning {
            column![
                container(txt(l.t("live")).size(12).style(|_| text::Style {
                    color: Some(iced::Color::WHITE),
                }))
                .padding(Padding::from([4, 8]))
                .style(theme::live_badge()),
                video,
            ]
            .spacing(8)
            .into()
        } else {
            video.into()
        };

        let percent = if self.needed == 0 {
            0.0
        } else {
            (self.unique as f32 / self.needed as f32 * 100.0).clamp(0.0, 99.0)
        };
        let bar_value = if self.saved.is_some() { 100.0 } else { percent };
        let status = if self.status.is_empty() {
            l.t("receive_idle").to_string()
        } else {
            self.status.clone()
        };

        let cam_btn = if self.scanning {
            button(txt(l.t("stop")))
                .padding(Padding::from([11, 18]))
                .style(theme::btn_danger(self.mode))
                .on_press(Message::StopCamera)
        } else {
            button(txt(l.t("start_camera")))
                .padding(Padding::from([11, 18]))
                .style(theme::btn_primary(self.mode))
                .on_press(Message::StartCamera)
        };

        let mut col = column![
            stack,
            progress_bar(0.0..=100.0, bar_value)
                .girth(8)
                .style(theme::progress(self.mode)),
            txt(status).style(theme::muted_text(self.mode)),
            row![
                cam_btn,
                txt(l.t("camera")).style(theme::muted_text(self.mode)),
                pick_list(
                    self.cameras.clone(),
                    self.camera.clone(),
                    Message::SelectCamera,
                )
                .placeholder(l.t("no_camera"))
                .width(Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(14);

        if let Some(saved) = &self.saved {
            let mut done = column![
                txt(l.saved(&saved.name, &format_bytes(saved.size))),
                button(txt(l.t("download_again")))
                    .padding(Padding::from([11, 18]))
                    .style(theme::btn_primary(self.mode))
                    .on_press(Message::SaveAgain),
            ]
            .spacing(10);
            if let Some(receipt) = saved.receipt.as_ref().or(self.receipt.as_ref()) {
                done = done.push(txt(receipt.summary(matches!(l, Lang::Zh))).size(13));
            }
            col = col.push(
                container(done)
                    .padding(14)
                    .width(Fill)
                    .style(theme::success(self.mode)),
            );
        }

        col = col.push(txt(l.t("video_hint")).style(theme::muted_text(self.mode)));

        container(col)
            .padding(18)
            .width(Fill)
            .style(theme::card(self.mode))
            .into()
    }

    fn view_play(&self) -> Element<'_, Message> {
        let l = self.lang;
        let Some(session) = self.session.as_ref() else {
            return Space::new().into();
        };
        let eta = {
            let rate = u64::from(session.symbol_mtu) * self.fps as u64 * self.grid as u64 * 7 / 10;
            session.compressed_size.div_ceil(rate.max(1))
        };
        let ratio = if session.orig_size == 0 {
            1.0
        } else {
            session.compressed_size as f64 / session.orig_size as f64
        };
        let transfer = l.transfer_line(
            &format_bytes(session.orig_size),
            &format_bytes(session.compressed_size),
            &l.duration(eta),
        );
        let hint = if self.fullscreen {
            l.t("hint_fullscreen")
        } else if self.grid > 1 {
            l.t("hint_quad")
        } else {
            l.t("hint_single")
        };
        let (cols, rows) = grid_dims(clamp_grid(self.grid));
        let long = if self.fullscreen { 720.0 } else { 520.0 };
        let cell = long / cols.max(rows) as f32;
        let qr_w = cell * cols as f32;
        let qr_h = cell * rows as f32;
        let qr: Element<_> = if let Some(handle) = &self.qr {
            mouse_area(
                container(
                    image(handle)
                        .width(Length::Fixed(qr_w))
                        .height(Length::Fixed(qr_h))
                        .filter_method(image::FilterMethod::Nearest),
                )
                .style(theme::qr_stage()),
            )
            .on_press(Message::ToggleFullscreen)
            .into()
        } else {
            container(Space::new().width(qr_w).height(qr_h))
                .style(theme::qr_stage())
                .into()
        };

        let fps_choices = [10u32, 15, 20, 24, 30, 55, 60];
        let density_choices = [Density::Stable, Density::Default, Density::Fast];
        let dock = container(
            column![
                row![
                    pill_label(self.mode, format!("{} fps", self.fps)),
                    pill_label(
                        self.mode,
                        match self.grid {
                            2 => l.t("grid_two").to_string(),
                            4 => l.t("grid_quad").to_string(),
                            6 => l.t("grid_six").to_string(),
                            _ => l.t("grid_single").to_string(),
                        },
                    ),
                    pill_label(self.mode, format!("v{}", session.qr_version)),
                    pill_label(self.mode, l.after_compress(ratio * 100.0)),
                ]
                .spacing(8),
                row![
                    button(txt(if self.playing {
                        l.t("pause")
                    } else {
                        l.t("play")
                    }))
                    .padding(Padding::from([11, 16]))
                    .style(theme::btn_card(self.mode))
                    .on_press(Message::TogglePlay),
                    row![
                        txt(l.t("speed")).style(theme::muted_text(self.mode)),
                        pick_list(fps_choices, Some(self.fps), Message::SetFps).width(110),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    row![
                        txt(l.t("density")).style(theme::muted_text(self.mode)),
                        pick_list(density_choices, Some(self.density), Message::SetDensity).width(120),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    button(txt(l.t("use_quad")))
                    .padding(Padding::from([11, 16]))
                    .style(theme::btn_ghost(self.mode))
                    .on_press(Message::ToggleGrid),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            ]
            .spacing(12),
        )
        .padding(14)
        .width(Fill)
        .style(theme::dock(self.mode));

        column![
            row![
                button(txt(l.t("back")))
                    .padding(Padding::from([11, 16]))
                    .style(theme::btn_ghost(self.mode))
                    .on_press(Message::Back),
                column![
                    txt(&session.file_name).size(18),
                    txt(transfer).style(theme::muted_text(self.mode)),
                ]
                .spacing(4)
                .width(Fill)
                .align_x(Alignment::Center),
                row![
                    lang_btn(self.mode, self.lang, Lang::En, "EN"),
                    lang_btn(self.mode, self.lang, Lang::Zh, "中文"),
                ]
                .spacing(4),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            container(column![qr, txt(hint).style(theme::muted_text(self.mode))]
                .spacing(12)
                .align_x(Alignment::Center))
            .width(Fill)
            .center_x(Fill),
            dock,
        ]
        .spacing(18)
        .into()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.stop_camera();
    }
}

fn txt<'a>(content: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(content).shaping(text::Shaping::Advanced)
}

fn cell() -> Element<'static, Message> {
    container(Space::new().width(10).height(10))
        .style(theme::mark_cell())
        .into()
}

fn qcell(mode: Mode) -> Element<'static, Message> {
    container(Space::new().width(22).height(22))
        .style(move |_| iced::widget::container::Style {
            border: iced::Border {
                color: mode.colors().accent,
                width: 3.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn lang_btn(mode: Mode, current: Lang, target: Lang, label: &'static str) -> Element<'static, Message> {
    button(txt(label))
        .padding(Padding::from([10, 12]))
        .style(theme::btn_lang(mode, current == target))
        .on_press(Message::Lang(target))
        .into()
}

fn pill_btn(mode: Mode, label: &'static str, msg: Message) -> Element<'static, Message> {
    button(txt(label))
        .padding(Padding::from([10, 14]))
        .style(theme::btn_card(mode))
        .on_press(msg)
        .into()
}

fn segment_btn(
    mode: Mode,
    label: &'static str,
    active: bool,
    msg: Message,
) -> Element<'static, Message> {
    button(txt(label))
        .padding(Padding::from([12, 8]))
        .width(Fill)
        .style(theme::btn_segment(mode, active))
        .on_press(msg)
        .into()
}

fn pill_label(mode: Mode, label: String) -> Element<'static, Message> {
    container(txt(label).size(13))
        .padding(Padding::from([4, 10]))
        .style(theme::pill(mode))
        .into()
}

fn prepare_file(path: PathBuf, lang: Lang) -> Result<SessionMeta, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file.bin")
        .to_string();
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() == 0 {
        return Err(lang.t("empty_file").to_string());
    }
    if meta.len() > MAX_FILE_SIZE {
        return Err(lang.file_too_large(&format_bytes(meta.len())));
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    match Outgoing::prepare_with(name.clone(), bytes, Density::Fast) {
        Ok(outgoing) => {
            let session = SessionMeta {
                file_name: outgoing.file_name.clone(),
                orig_size: outgoing.orig_size,
                compressed_size: outgoing.compressed_size,
                symbol_mtu: outgoing.symbol_mtu(),
                qr_version: outgoing.density.qr_version(),
            };
            *PENDING.lock().expect("pending") = Some(outgoing);
            Ok(session)
        }
        Err(PrepareError::Empty) => Err(lang.t("empty_file").to_string()),
        Err(PrepareError::TooLarge(size)) => Err(lang.file_too_large(&format_bytes(size))),
    }
}

fn save_bytes(name: &str, data: &[u8], lang: Lang) {
    let Some(path) = rfd::FileDialog::new().set_file_name(name).save_file() else {
        return;
    };
    if let Err(err) = std::fs::write(&path, data) {
        eprintln!("RaySend：保存失败：{err}");
        let _ = lang;
    }
}
