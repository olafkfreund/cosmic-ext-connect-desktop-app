use cosmic::app::{Core, Task};
use cosmic::iced::Length;
use cosmic::widget::{self, container, text, button};
use cosmic::Element;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessengerType {
    GoogleMessages,
    WhatsApp,
    Telegram,
    Signal,
    Discord,
    Slack,
}

impl MessengerType {
    pub fn web_url(&self) -> &'static str {
        match self {
            Self::GoogleMessages => "https://messages.google.com/web",
            Self::WhatsApp => "https://web.whatsapp.com",
            Self::Telegram => "https://web.telegram.org",
            Self::Signal => "https://signal.link",
            Self::Discord => "https://discord.com/app",
            Self::Slack => "https://app.slack.com",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::GoogleMessages => "Messages",
            Self::WhatsApp => "WhatsApp",
            Self::Telegram => "Telegram",
            Self::Signal => "Signal",
            Self::Discord => "Discord",
            Self::Slack => "Slack",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagingNotification {
    pub messenger: MessengerType,
    pub sender: String,
    pub message: String,
    pub conversation_id: Option<String>,
}

pub struct CosmicMessages {
    core: Core,
    current_messenger: MessengerType,
    webview: Option<Arc<Mutex<Option<wry::WebView>>>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    SwitchMessenger(MessengerType),
    NotificationReceived(MessagingNotification),
    WebViewCreated,
}

impl cosmic::Application for CosmicMessages {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "org.cosmicde.Messages";

    fn core(&self) -> &Core { &self.core }
    fn core_mut(&mut self) -> &mut Core { &mut self.core }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        (
            Self {
                core,
                current_messenger: MessengerType::GoogleMessages,
                webview: Some(Arc::new(Mutex::new(None))),
            },
            Task::none()
        )
    }

    fn update(&mut self, message: Self::Message) -> Task<Message> {
        match message {
            Message::SwitchMessenger(m) => {
                self.current_messenger = m;
                if let Some(ref wv_arc) = self.webview {
                    if let Ok(guard) = wv_arc.lock() {
                        if let Some(ref wv) = *guard {
                            let _ = wv.load_url(m.web_url());
                        }
                    }
                }
                Task::none()
            }
            Message::NotificationReceived(notif) => {
                self.current_messenger = notif.messenger;
                if let Some(ref wv_arc) = self.webview {
                    if let Ok(guard) = wv_arc.lock() {
                        if let Some(ref wv) = *guard {
                            let _ = wv.load_url(notif.messenger.web_url());
                        }
                    }
                }
                Task::none()
            }
            Message::WebViewCreated => Task::none(),
        }
    }

    fn view(&self) -> Element<Message> {
        let tabs = widget::row::with_children(vec![
            button::text("Google Messages").on_press(Message::SwitchMessenger(MessengerType::GoogleMessages)).into(),
            button::text("WhatsApp").on_press(Message::SwitchMessenger(MessengerType::WhatsApp)).into(),
            button::text("Telegram").on_press(Message::SwitchMessenger(MessengerType::Telegram)).into(),
        ])
        .spacing(10)
        .padding(10);

        widget::column::with_children(vec![
            tabs.into(),
            container(text::body("WebView would be here (requires window handle integration)"))
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .into(),
        ]).into()
    }
}

fn main() -> cosmic::iced::Result {
    cosmic::app::run::<CosmicMessages>(cosmic::app::Settings::default(), ())
}
