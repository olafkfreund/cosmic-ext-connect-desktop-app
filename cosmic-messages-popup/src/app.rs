//! COSMIC Application Implementation
//!
//! Main application state and update logic for the messages popup.

use crate::config::{Config, PopupPosition};
use crate::dbus::{DbusCommand, NotificationData};
use crate::gtk_webview;
use crate::notification::NotificationHandler;
use crate::webview::WebViewManager;
use cosmic::app::{Core, Task};
use cosmic::iced::keyboard::Key;
use cosmic::iced::{keyboard, Length, Subscription};
use cosmic::widget::{self, button, column, container, divider, icon, row, text, toggler};
use cosmic::{Action, Application, Element};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Application messages
#[derive(Debug, Clone)]
pub enum Message {
    /// Switch to a different messenger
    SwitchMessenger(String),
    /// Show the popup
    ShowPopup,
    /// Hide the popup
    HidePopup,
    /// Toggle popup visibility
    TogglePopup,
    /// Notification received from D-Bus
    NotificationReceived(NotificationData),
    /// WebView finished loading
    WebViewLoaded,
    /// Open settings
    OpenSettings,
    /// Close settings
    CloseSettings,
    /// Toggle a messenger's enabled state
    ToggleMessengerEnabled(String, bool),
    /// Set popup position
    SetPopupPosition(PopupPosition),
    /// Toggle auto-open
    SetAutoOpen(bool),
    /// Toggle notifications
    SetNotifications(bool),
    /// Toggle sound
    SetSound(bool),
    /// Clear WebView data for a messenger
    ClearWebViewData(String),
    /// Open messenger in external browser
    OpenExternal(String),
    /// Keyboard shortcut pressed
    KeyPressed(Key),
    /// D-Bus command received
    DbusCommand(DbusCommand),
    /// Config changed
    ConfigChanged(Config),
    /// No operation
    Noop,
}

/// Main application state
pub struct MessagesPopup {
    /// COSMIC core
    core: Core,
    /// Current configuration
    config: Config,
    /// WebView manager
    webview_manager: WebViewManager,
    /// Notification handler
    notification_handler: NotificationHandler,
    /// Whether the popup is visible
    visible: bool,
    /// Settings panel open
    settings_open: bool,
    /// D-Bus command sender (for creating the receiver)
    #[allow(dead_code)]
    dbus_sender: mpsc::Sender<DbusCommand>,
}

impl Application for MessagesPopup {
    type Executor = cosmic::executor::Default;
    type Flags = mpsc::Sender<DbusCommand>;
    type Message = Message;

    const APP_ID: &'static str = "org.cosmicde.MessagesPopup";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let config = Config::load().unwrap_or_default();
        let mut webview_manager = WebViewManager::new(config.clone());

        let initial_messenger = config
            .popup
            .last_messenger
            .as_deref()
            .unwrap_or("google-messages");

        let _ = webview_manager.set_current(initial_messenger);

        let notification_handler = NotificationHandler::new(config.clone());

        let app = Self {
            core,
            config,
            webview_manager,
            notification_handler,
            visible: false,
            settings_open: false,
            dbus_sender: flags,
        };

        info!("COSMIC Messages Popup initialized");

        (app, Task::none())
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::SwitchMessenger(messenger_id) => {
                debug!("Switching to messenger: {}", messenger_id);
                if let Err(e) = self.webview_manager.set_current(&messenger_id) {
                    error!("Failed to switch messenger: {}", e);
                }

                // Show the GTK WebView window
                if let Some(url) = self.webview_manager.current_url() {
                    if let Err(e) = gtk_webview::show_messenger_window(&messenger_id, url, &self.config) {
                        error!("Failed to show WebView window: {}", e);
                    }
                }

                // Update last messenger in config
                if self.config.popup.remember_last {
                    self.config.popup.last_messenger = Some(messenger_id);
                    let _ = self.config.save();
                }
            }

            Message::ShowPopup => {
                self.visible = true;
                // Show current messenger's GTK window
                if let Some(messenger_id) = self.webview_manager.current() {
                    if let Some(url) = self.webview_manager.current_url() {
                        let _ = gtk_webview::show_messenger_window(messenger_id, url, &self.config);
                    }
                }
                debug!("Showing popup");
            }

            Message::HidePopup => {
                self.visible = false;
                // Hide all GTK windows
                let _ = gtk_webview::hide_all_windows();
                debug!("Hiding popup");
            }

            Message::TogglePopup => {
                self.visible = !self.visible;
                if self.visible {
                    if let Some(messenger_id) = self.webview_manager.current() {
                        if let Some(url) = self.webview_manager.current_url() {
                            let _ = gtk_webview::show_messenger_window(messenger_id, url, &self.config);
                        }
                    }
                } else {
                    let _ = gtk_webview::hide_all_windows();
                }
                debug!("Toggling popup: {}", self.visible);
            }

            Message::NotificationReceived(data) => {
                debug!("Notification received: {} - {}", data.title, data.text);

                if let Some(messenger_id) = self.notification_handler.handle_notification(&data) {
                    // Switch to the messenger
                    let _ = self.webview_manager.set_current(&messenger_id);

                    // Auto-open if enabled
                    if self.notification_handler.should_auto_open() {
                        self.visible = true;
                        // Show GTK WebView window
                        if let Some(url) = self.webview_manager.current_url() {
                            let _ = gtk_webview::show_messenger_window(&messenger_id, url, &self.config);
                        }
                    }
                }
            }

            Message::WebViewLoaded => {
                self.webview_manager.mark_loaded();
                debug!("WebView loaded");
            }

            Message::OpenSettings => {
                self.settings_open = true;
            }

            Message::CloseSettings => {
                self.settings_open = false;
            }

            Message::ToggleMessengerEnabled(id, enabled) => {
                self.config.toggle_messenger(&id, enabled);
                let _ = self.config.save();
                self.webview_manager.update_config(self.config.clone());
                self.notification_handler.update_config(self.config.clone());
            }

            Message::SetPopupPosition(pos) => {
                self.config.popup.position = pos;
                let _ = self.config.save();
            }

            Message::SetAutoOpen(enabled) => {
                self.config.notifications.auto_open = enabled;
                let _ = self.config.save();
                self.notification_handler.update_config(self.config.clone());
            }

            Message::SetNotifications(enabled) => {
                self.config.notifications.show_notifications = enabled;
                let _ = self.config.save();
                self.notification_handler.update_config(self.config.clone());
            }

            Message::SetSound(enabled) => {
                self.config.notifications.play_sound = enabled;
                let _ = self.config.save();
            }

            Message::ClearWebViewData(messenger_id) => {
                if let Err(e) = self.webview_manager.clear_data(&messenger_id) {
                    error!("Failed to clear WebView data: {}", e);
                }
            }

            Message::OpenExternal(messenger_id) => {
                if let Some(url) = self.notification_handler.get_messenger_url(&messenger_id) {
                    let _ = open::that(&url);
                }
            }

            Message::KeyPressed(key) => {
                if let Key::Character(c) = key {
                    let messenger = match c.as_str() {
                        "1" => Some("google-messages"),
                        "2" => Some("whatsapp"),
                        "3" => Some("telegram"),
                        _ => None,
                    };
                    if let Some(id) = messenger {
                        return Task::done(Action::App(Message::SwitchMessenger(id.to_string())));
                    }
                }
            }

            Message::DbusCommand(cmd) => {
                match cmd {
                    DbusCommand::ShowMessenger(id) => {
                        let _ = self.webview_manager.set_current(&id);
                        self.visible = true;
                    }
                    DbusCommand::HidePopup => self.visible = false,
                    DbusCommand::TogglePopup => self.visible = !self.visible,
                    DbusCommand::NotificationReceived(data) => {
                        return Task::done(Action::App(Message::NotificationReceived(data)));
                    }
                }
            }

            Message::ConfigChanged(config) => {
                self.config = config.clone();
                self.webview_manager.update_config(config.clone());
                self.notification_handler.update_config(config);
            }

            Message::Noop => {}
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        if self.settings_open {
            return self.build_settings();
        }

        if !self.visible {
            // Return minimal view when hidden
            return container(text::body("Messages Popup - Hidden"))
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        }

        // Build messenger tabs
        let tabs = self.build_messenger_tabs();

        // Build main content area
        let content = self.build_content();

        // Build header with settings button
        let header = self.build_header();

        column::with_capacity(3)
            .push(header)
            .push(tabs)
            .push(content)
            .spacing(0)
            .width(Length::Fixed(self.config.popup.width as f32))
            .height(Length::Fixed(self.config.popup.height as f32))
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // Subscribe to keyboard events
        keyboard::on_key_press(|key, modifiers| {
            if modifiers.command() {
                Some(Message::KeyPressed(key))
            } else {
                None
            }
        })
    }
}

impl MessagesPopup {
    /// Build the header bar
    fn build_header(&self) -> Element<'_, Message> {
        let display_name = self
            .webview_manager
            .current()
            .map(|id| self.webview_manager.get_display_name(id))
            .unwrap_or_else(|| "Messages".to_string());
        let title = text::heading(display_name);

        let settings_button = button::icon(icon::from_name("emblem-system-symbolic"))
            .padding(8)
            .on_press(Message::OpenSettings);

        let close_button = button::icon(icon::from_name("window-close-symbolic"))
            .padding(8)
            .on_press(Message::HidePopup);

        row::with_capacity(4)
            .push(title)
            .push(widget::horizontal_space())
            .push(settings_button)
            .push(close_button)
            .padding(8)
            .spacing(8)
            .align_y(cosmic::iced::Alignment::Center)
            .into()
    }

    /// Build messenger tabs
    fn build_messenger_tabs(&self) -> Element<'_, Message> {
        let tabs = self.webview_manager.get_all_info().into_iter().fold(
            row::with_capacity(6).spacing(4).padding(8),
            |tabs, info| {
                let btn = if info.is_current {
                    button::suggested(info.display_name.clone())
                } else {
                    button::text(info.display_name.clone())
                };
                tabs.push(btn.on_press(Message::SwitchMessenger(info.messenger_id)))
            },
        );

        container(tabs).width(Length::Fill).into()
    }

    /// Build the main content area
    fn build_content(&self) -> Element<'_, Message> {
        if let Some(ctx) = self.webview_manager.current_context() {
            // Show WebView placeholder with URL info
            // Note: Actual WebView rendering requires platform-specific integration
            let info = column::with_capacity(4)
                .push(text::body("WebView Content"))
                .push(text::caption(&ctx.url))
                .push(widget::vertical_space())
                .push(text::caption(
                    "WebView integration requires window handle.\nSee wry documentation for COSMIC integration.",
                ))
                .spacing(8)
                .align_x(cosmic::iced::Alignment::Center);

            container(info)
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .into()
        } else {
            let no_messenger = column::with_capacity(2)
                .push(text::title1("No Messenger Selected"))
                .push(text::body("Select a messenger tab above to start."))
                .spacing(8)
                .align_x(cosmic::iced::Alignment::Center);

            container(no_messenger)
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .into()
        }
    }

    /// Build settings view
    pub fn build_settings(&self) -> Element<'_, Message> {
        let mut content = column::with_capacity(20).spacing(16).padding(16);

        // Header
        content = content.push(
            row::with_capacity(3)
                .push(text::heading("Settings"))
                .push(widget::horizontal_space())
                .push(
                    button::icon(icon::from_name("window-close-symbolic"))
                        .padding(8)
                        .on_press(Message::CloseSettings),
                )
                .align_y(cosmic::iced::Alignment::Center),
        );

        // Messengers section
        content = content.push(text::title4("Enabled Messengers"));

        for messenger in &self.config.enabled_messengers {
            let messenger_id = messenger.id.clone();
            let toggle = toggler(messenger.enabled).on_toggle(move |v| {
                Message::ToggleMessengerEnabled(messenger_id.clone(), v)
            });

            content = content.push(
                row::with_capacity(3)
                    .push(text::body(&messenger.name))
                    .push(widget::horizontal_space())
                    .push(toggle)
                    .spacing(8)
                    .align_y(cosmic::iced::Alignment::Center),
            );
        }

        // Popup settings
        content = content.push(divider::horizontal::default());
        content = content.push(text::title4("Popup Settings"));

        // Position info
        content = content.push(
            row::with_capacity(3)
                .push(text::body("Position:"))
                .push(widget::horizontal_space())
                .push(text::body(self.config.popup.position.display_name())),
        );

        // Notification settings
        content = content.push(divider::horizontal::default());
        content = content.push(text::title4("Notifications"));

        let notif_toggle =
            toggler(self.config.notifications.show_notifications).on_toggle(Message::SetNotifications);
        content = content.push(
            row::with_capacity(3)
                .push(text::body("Show notifications"))
                .push(widget::horizontal_space())
                .push(notif_toggle)
                .align_y(cosmic::iced::Alignment::Center),
        );

        let auto_toggle =
            toggler(self.config.notifications.auto_open).on_toggle(Message::SetAutoOpen);
        content = content.push(
            row::with_capacity(3)
                .push(text::body("Auto-open on notification"))
                .push(widget::horizontal_space())
                .push(auto_toggle)
                .align_y(cosmic::iced::Alignment::Center),
        );

        let sound_toggle =
            toggler(self.config.notifications.play_sound).on_toggle(Message::SetSound);
        content = content.push(
            row::with_capacity(3)
                .push(text::body("Play sound"))
                .push(widget::horizontal_space())
                .push(sound_toggle)
                .align_y(cosmic::iced::Alignment::Center),
        );

        // Close button
        content = content.push(widget::vertical_space());
        content = content.push(
            button::suggested("Close Settings")
                .on_press(Message::CloseSettings)
                .width(Length::Fill),
        );

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// Create the D-Bus command channel
pub fn create_dbus_channel() -> (mpsc::Sender<DbusCommand>, mpsc::Receiver<DbusCommand>) {
    mpsc::channel(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_variants() {
        // Ensure all message variants are clonable
        let msg = Message::ShowPopup;
        let _ = msg.clone();

        let msg = Message::SwitchMessenger("test".to_string());
        let _ = msg.clone();
    }
}
