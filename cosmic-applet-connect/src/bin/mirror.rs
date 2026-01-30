use cosmic::app::{Core, Task};
use cosmic::iced::mouse;
use cosmic::iced::widget::{canvas, image};
use cosmic::iced::{Color, Length, Rectangle, Size};
use cosmic::iced_widget::Stack;
use cosmic::widget::{button, column, container, text};
use cosmic::Element;
use std::env;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use cosmic_applet_connect::dbus_client::{DaemonEvent, DbusClient};
use cosmic_connect_protocol::plugins::screenshare::decoder::VideoDecoder;
use cosmic_connect_protocol::plugins::screenshare::stream_receiver::StreamReceiver;

/// Remote cursor state
#[derive(Debug, Clone, Default)]
struct CursorState {
    x: i32,
    y: i32,
    visible: bool,
}

/// Annotation shape received from remote
#[derive(Debug, Clone)]
struct Annotation {
    annotation_type: String,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    color: String,
    width: u8,
}

/// Overlay program for rendering cursor and annotations on top of video
struct OverlayProgram {
    cursor: CursorState,
    annotations: Vec<Annotation>,
    video_size: Option<(u32, u32)>,
}

impl<Message> canvas::Program<Message, cosmic::Theme, cosmic::Renderer> for OverlayProgram {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &cosmic::Renderer,
        _theme: &cosmic::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<cosmic::Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // Calculate scale factors if we know video dimensions
        let (scale_x, scale_y) = if let Some((video_w, video_h)) = self.video_size {
            (bounds.width / video_w as f32, bounds.height / video_h as f32)
        } else {
            (1.0, 1.0)
        };

        // Draw annotations first (they appear behind cursor)
        for annotation in &self.annotations {
            let color = parse_color(&annotation.color);
            let stroke_width = annotation.width as f32;

            let x1 = annotation.x1 as f32 * scale_x;
            let y1 = annotation.y1 as f32 * scale_y;
            let x2 = annotation.x2 as f32 * scale_x;
            let y2 = annotation.y2 as f32 * scale_y;

            match annotation.annotation_type.as_str() {
                "line" => {
                    let path = canvas::Path::line(
                        cosmic::iced::Point::new(x1, y1),
                        cosmic::iced::Point::new(x2, y2),
                    );
                    frame.stroke(
                        &path,
                        canvas::Stroke::default().with_width(stroke_width).with_color(color),
                    );
                }
                "rect" => {
                    let rect = canvas::Path::rectangle(
                        cosmic::iced::Point::new(x1.min(x2), y1.min(y2)),
                        Size::new((x2 - x1).abs(), (y2 - y1).abs()),
                    );
                    frame.stroke(
                        &rect,
                        canvas::Stroke::default().with_width(stroke_width).with_color(color),
                    );
                }
                "circle" => {
                    let center = cosmic::iced::Point::new((x1 + x2) / 2.0, (y1 + y2) / 2.0);
                    let radius = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt() / 2.0;
                    let circle = canvas::Path::circle(center, radius);
                    frame.stroke(
                        &circle,
                        canvas::Stroke::default().with_width(stroke_width).with_color(color),
                    );
                }
                _ => {}
            }
        }

        // Draw cursor if visible
        if self.cursor.visible {
            let cursor_x = self.cursor.x as f32 * scale_x;
            let cursor_y = self.cursor.y as f32 * scale_y;

            // Draw cursor as a circle with crosshair
            let cursor_color = Color::from_rgba(1.0, 0.5, 0.0, 0.9); // Orange

            // Outer circle
            let outer_circle = canvas::Path::circle(
                cosmic::iced::Point::new(cursor_x, cursor_y),
                8.0,
            );
            frame.stroke(
                &outer_circle,
                canvas::Stroke::default().with_width(2.0).with_color(cursor_color),
            );

            // Inner dot
            let inner_circle = canvas::Path::circle(
                cosmic::iced::Point::new(cursor_x, cursor_y),
                3.0,
            );
            frame.fill(&inner_circle, cursor_color);
        }

        vec![frame.into_geometry()]
    }
}

/// Parse a color string (hex format like "#FF0000") into a Color
fn parse_color(color_str: &str) -> Color {
    if color_str.starts_with('#') && color_str.len() == 7 {
        let r = u8::from_str_radix(&color_str[1..3], 16).unwrap_or(255);
        let g = u8::from_str_radix(&color_str[3..5], 16).unwrap_or(0);
        let b = u8::from_str_radix(&color_str[5..7], 16).unwrap_or(0);
        Color::from_rgb8(r, g, b)
    } else {
        Color::from_rgb(1.0, 0.0, 0.0) // Default to red
    }
}

struct MirrorApp {
    core: Core,
    device_id: String,
    status: String,
    frame: Option<image::Handle>,
    frame_size: Option<(u32, u32)>,
    receiver_rx: Arc<Mutex<mpsc::Receiver<Message>>>,
    #[allow(dead_code)]
    dbus: Option<DbusClient>,
    cursor: CursorState,
    annotations: Vec<Annotation>,
}

#[derive(Debug, Clone)]
enum Message {
    Close,
    StatusUpdate(String),
    FrameReceived(image::Handle, u32, u32), // handle, width, height
    Error(String),
    Connected,
    Loop(Box<Message>),
    DbusConnected(DbusClient),
    CursorUpdate { x: i32, y: i32, visible: bool },
    AnnotationReceived(Annotation),
    #[allow(dead_code)]
    ClearAnnotations,
}

impl cosmic::Application for MirrorApp {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "com.system76.CosmicConnect.Mirror";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let args: Vec<String> = env::args().collect();
        let device_id = args.get(1).cloned().unwrap_or_else(|| "unknown".to_string());

        let (tx, rx) = mpsc::channel(100); // Increased buffer for cursor updates
        let receiver_rx = Arc::new(Mutex::new(rx));
        let dev_id = device_id.clone();
        let dev_id_for_events = device_id.clone();

        // Spawn main streaming task
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let _ = tx_clone
                .send(Message::StatusUpdate("Connecting to daemon...".into()))
                .await;

            let (client, mut event_rx) = match DbusClient::connect().await {
                Ok((c, r)) => {
                    let _ = tx_clone.send(Message::DbusConnected(c.clone())).await;
                    (c, r)
                }
                Err(e) => {
                    let _ = tx_clone
                        .send(Message::Error(format!("DBus connect failed: {}", e)))
                        .await;
                    return;
                }
            };

            // Start signal listener for cursor/annotation events
            if let Err(e) = client.start_signal_listener().await {
                let _ = tx_clone
                    .send(Message::Error(format!("Signal listener failed: {}", e)))
                    .await;
                return;
            }

            // Spawn task to handle DBus events (cursor/annotations)
            let tx_events = tx_clone.clone();
            let target_device = dev_id_for_events.clone();
            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    match event {
                        DaemonEvent::ScreenShareCursorUpdate {
                            device_id,
                            x,
                            y,
                            visible,
                        } => {
                            // Only process events for our target device
                            if device_id == target_device {
                                let _ = tx_events
                                    .send(Message::CursorUpdate { x, y, visible })
                                    .await;
                            }
                        }
                        DaemonEvent::ScreenShareAnnotation {
                            device_id,
                            annotation_type,
                            x1,
                            y1,
                            x2,
                            y2,
                            color,
                            width,
                        } => {
                            if device_id == target_device {
                                let _ = tx_events
                                    .send(Message::AnnotationReceived(Annotation {
                                        annotation_type,
                                        x1,
                                        y1,
                                        x2,
                                        y2,
                                        color,
                                        width,
                                    }))
                                    .await;
                            }
                        }
                        _ => {}
                    }
                }
            });

            let _ = tx_clone
                .send(Message::StatusUpdate("Starting listener...".into()))
                .await;

            let mut receiver = StreamReceiver::new();
            let port = match receiver.listen().await {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx_clone
                        .send(Message::Error(format!("Listen failed: {}", e)))
                        .await;
                    return;
                }
            };

            let _ = tx_clone
                .send(Message::StatusUpdate(format!(
                    "Listening on port {}. Requesting stream...",
                    port
                )))
                .await;

            if let Err(e) = client.start_screen_share(&dev_id, port).await {
                let _ = tx_clone
                    .send(Message::Error(format!("StartScreenShare failed: {}", e)))
                    .await;
                return;
            }

            let _ = tx_clone
                .send(Message::StatusUpdate("Waiting for connection...".into()))
                .await;

            if let Err(e) = receiver.accept().await {
                let _ = tx_clone
                    .send(Message::Error(format!("Accept failed: {}", e)))
                    .await;
                return;
            }

            let _ = tx_clone.send(Message::Connected).await;

            let decoder = match VideoDecoder::new() {
                Ok(d) => d,
                Err(e) => {
                    let _ = tx_clone
                        .send(Message::Error(format!("Decoder init failed: {}", e)))
                        .await;
                    return;
                }
            };

            if let Err(e) = decoder.start() {
                let _ = tx_clone
                    .send(Message::Error(format!("Decoder start failed: {}", e)))
                    .await;
                return;
            }

            loop {
                match receiver.next_frame().await {
                    Ok((_type, _ts, payload)) => {
                        if let Err(e) = decoder.push_frame(&payload) {
                            let _ = tx_clone
                                .send(Message::Error(format!("Decode push error: {}", e)))
                                .await;
                            break;
                        }

                        match decoder.pull_frame() {
                            Ok(Some((data, width, height))) => {
                                let handle = image::Handle::from_rgba(width, height, data);
                                if tx_clone
                                    .send(Message::FrameReceived(handle, width, height))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                let _ = tx_clone
                                    .send(Message::Error(format!("Decode pull error: {}", e)))
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx_clone
                            .send(Message::Error(format!("Stream error: {}", e)))
                            .await;
                        break;
                    }
                }
            }
        });

        let app = Self {
            core,
            device_id,
            status: "Initializing...".to_string(),
            frame: None,
            frame_size: None,
            receiver_rx: receiver_rx.clone(),
            dbus: None,
            cursor: CursorState::default(),
            annotations: Vec::new(),
        };

        let task = Task::perform(wait_for_message(receiver_rx), |msg| {
            cosmic::Action::App(Message::Loop(Box::new(msg)))
        });

        (app, task)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loop(inner) => {
                let task = self.update(*inner);
                let next = Task::perform(wait_for_message(self.receiver_rx.clone()), |msg| {
                    cosmic::Action::App(Message::Loop(Box::new(msg)))
                });
                Task::batch(vec![task, next])
            }
            Message::Close => {
                std::process::exit(0);
            }
            Message::StatusUpdate(s) => {
                self.status = s;
                Task::none()
            }
            Message::Error(e) => {
                self.status = format!("Error: {}", e);
                Task::none()
            }
            Message::FrameReceived(handle, width, height) => {
                self.frame = Some(handle);
                self.frame_size = Some((width, height));
                Task::none()
            }
            Message::Connected => {
                self.status = "Connected".to_string();
                Task::none()
            }
            Message::DbusConnected(client) => {
                self.dbus = Some(client);
                Task::none()
            }
            Message::CursorUpdate { x, y, visible } => {
                self.cursor = CursorState { x, y, visible };
                Task::none()
            }
            Message::AnnotationReceived(annotation) => {
                self.annotations.push(annotation);
                // Keep only last 100 annotations to prevent memory growth
                if self.annotations.len() > 100 {
                    self.annotations.remove(0);
                }
                Task::none()
            }
            Message::ClearAnnotations => {
                self.annotations.clear();
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        if let Some(handle) = &self.frame {
            // Main video display
            let video: Element<'_, Message> = image::viewer(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .into();

            // Canvas overlay for cursor and annotations
            let overlay_program = OverlayProgram {
                cursor: self.cursor.clone(),
                annotations: self.annotations.clone(),
                video_size: self.frame_size,
            };

            let overlay: Element<'_, Message> =
                canvas::Canvas::<OverlayProgram, Message, cosmic::Theme, cosmic::Renderer>::new(
                    overlay_program,
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .into();

            // Stack video with overlay on top
            let stacked: Element<'_, Message> = Stack::with_children(vec![video, overlay]).into();

            container(stacked)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            container(
                column()
                    .push(text(format!("Mirroring: {}", self.device_id)).size(24))
                    .push(text(&self.status))
                    .push(button::text("Close").on_press(Message::Close))
                    .padding(20)
                    .spacing(10)
                    .align_x(cosmic::iced::Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(cosmic::iced::Alignment::Center)
            .align_y(cosmic::iced::Alignment::Center)
            .into()
        }
    }
}

async fn wait_for_message(rx: Arc<Mutex<mpsc::Receiver<Message>>>) -> Message {
    let mut rx = rx.lock().await;
    rx.recv().await.unwrap_or(Message::Close)
}

fn main() -> cosmic::iced::Result {
    cosmic::app::run::<MirrorApp>(cosmic::app::Settings::default(), ())
}
