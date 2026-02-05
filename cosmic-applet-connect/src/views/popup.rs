use cosmic::{
    iced::{
        alignment::Horizontal,
        widget::{column, container, row, scrollable},
        Length, Padding,
    },
    widget::{button, divider, icon, text},
    Element,
};

use crate::{
    horizontal_space, messages::NotificationType, space_m, space_none, space_xxs, space_xs,
    space_xxxs, state::ViewMode, CConnectApplet, Message, ICON_14, ICON_S, ICON_XL,
    ICON_XS,
};

use super::device::{categorize_device, DeviceCategory};

impl CConnectApplet {
    pub fn popup_view(&self) -> Element<'_, Message> {
        let mut content = self.inner_view();

        // Screen share control overlay - shows when actively sharing/receiving
        if let Some(screen_share) = &self.active_screen_share {
            let device_name = self
                .devices
                .iter()
                .find(|d| d.device.id() == screen_share.device_id)
                .map(|d| d.device.name().to_string())
                .unwrap_or_else(|| screen_share.device_id.clone());

            let viewer_count = screen_share.viewer_count;
            let (status_text, status_caption) = if screen_share.is_sender {
                if screen_share.is_paused {
                    (
                        format!("Sharing paused to {}", device_name),
                        "Screen sharing paused".to_string(),
                    )
                } else {
                    let viewer_text = if viewer_count == 1 {
                        "1 viewer".to_string()
                    } else {
                        format!("{} viewers", viewer_count)
                    };
                    (format!("Sharing screen to {}", device_name), viewer_text)
                }
            } else {
                (
                    format!("Receiving screen from {}", device_name),
                    "Screen sharing active".to_string(),
                )
            };

            let device_id = screen_share.device_id.clone();
            let device_id_for_pause = device_id.clone();
            let device_id_for_audio = device_id.clone();
            let is_paused = screen_share.is_paused;
            let is_sender = screen_share.is_sender;
            let include_audio = screen_share.include_audio;

            // Build control buttons
            let mut controls = row![].spacing(space_xxs());

            // Audio toggle (only for sender)
            if is_sender {
                let audio_toggle = cosmic::widget::toggler(include_audio)
                    .label("Include Audio")
                    .on_toggle(move |enabled| {
                        Message::ToggleScreenShareAudio(device_id_for_audio.clone(), enabled)
                    });
                controls = controls.push(audio_toggle);
            }

            // Pause/Resume button (only for sender)
            if is_sender {
                let pause_resume_btn = if is_paused {
                    button::standard("Resume")
                        .on_press(Message::ResumeScreenShare(device_id_for_pause))
                        .padding(space_xxxs())
                } else {
                    button::standard("Pause")
                        .on_press(Message::PauseScreenShare(device_id_for_pause))
                        .padding(space_xxxs())
                };
                controls = controls.push(pause_resume_btn);
            }

            // Stop button
            controls = controls.push(
                button::destructive("Stop")
                    .on_press(Message::StopScreenShare(device_id))
                    .padding(space_xxxs()),
            );

            let control_row = row![
                icon::from_name("video-display-symbolic").size(ICON_S),
                column![
                    text(status_text),
                    cosmic::widget::text::caption(status_caption),
                ]
                .spacing(space_xxxs()),
                horizontal_space(),
                controls,
            ]
            .spacing(space_xxs())
            .align_y(cosmic::iced::Alignment::Center);

            // Quality settings panel (only for sender)
            let mut overlay_content = vec![container(control_row)
                .width(Length::Fill)
                .padding(space_xxs())
                .class(cosmic::theme::Container::Primary)
                .into()];

            if is_sender {
                let current_quality = &screen_share.quality;
                let current_fps = screen_share.fps;

                // Quality preset buttons
                let quality_buttons = row![
                    text("Quality:").width(Length::Fixed(60.0)),
                    button::text("Low")
                        .on_press(Message::SetScreenShareQuality("low".to_string()))
                        .padding(space_xxxs())
                        .class(if current_quality == "low" {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                    button::text("Medium")
                        .on_press(Message::SetScreenShareQuality("medium".to_string()))
                        .padding(space_xxxs())
                        .class(if current_quality == "medium" {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                    button::text("High")
                        .on_press(Message::SetScreenShareQuality("high".to_string()))
                        .padding(space_xxxs())
                        .class(if current_quality == "high" {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                ]
                .spacing(space_xxs())
                .align_y(cosmic::iced::Alignment::Center);

                // FPS buttons
                let fps_buttons = row![
                    text("FPS:").width(Length::Fixed(60.0)),
                    button::text("15")
                        .on_press(Message::SetScreenShareFps(15))
                        .padding(space_xxxs())
                        .class(if current_fps == 15 {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                    button::text("30")
                        .on_press(Message::SetScreenShareFps(30))
                        .padding(space_xxxs())
                        .class(if current_fps == 30 {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                    button::text("60")
                        .on_press(Message::SetScreenShareFps(60))
                        .padding(space_xxxs())
                        .class(if current_fps == 60 {
                            cosmic::theme::Button::Suggested
                        } else {
                            cosmic::theme::Button::Standard
                        }),
                ]
                .spacing(space_xxs())
                .align_y(cosmic::iced::Alignment::Center);

                let settings_panel = column![quality_buttons, fps_buttons,].spacing(space_xxs());

                overlay_content.push(
                    container(settings_panel)
                        .width(Length::Fill)
                        .padding(space_xxs())
                        .class(cosmic::theme::Container::Secondary)
                        .into(),
                );
            }

            overlay_content.push(content);

            content = column(overlay_content).spacing(space_xxs()).into();
        }

        // Keyboard shortcuts help dialog
        if self.show_keyboard_shortcuts_help {
            let shortcuts_content = column![
                row![
                    cosmic::widget::text::title3("Keyboard Shortcuts").width(Length::Fill),
                    cosmic::widget::tooltip(
                        button::icon(icon::from_name("window-close-symbolic").size(ICON_14))
                            .on_press(Message::ToggleKeyboardShortcutsHelp)
                            .padding(space_xxxs()),
                        "Close",
                        cosmic::widget::tooltip::Position::Bottom,
                    )
                ]
                .align_y(cosmic::iced::Alignment::Center),
                divider::horizontal::default(),
                column![
                    row![
                        cosmic::widget::text::body("Escape").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Close dialogs/overlays")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(space_xxs()),
                    row![
                        cosmic::widget::text::body("Ctrl+R").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Refresh devices").width(Length::FillPortion(3)),
                    ]
                    .spacing(space_xxs()),
                    row![
                        cosmic::widget::text::body("Ctrl+F").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Focus search").width(Length::FillPortion(3)),
                    ]
                    .spacing(space_xxs()),
                    row![
                        cosmic::widget::text::body("Ctrl+,").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Toggle device settings")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(space_xxs()),
                    row![
                        cosmic::widget::text::body("Ctrl+M").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Open Manager").width(Length::FillPortion(3)),
                    ]
                    .spacing(space_xxs()),
                    row![
                        cosmic::widget::text::body("F1 or ?").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Show this help dialog")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(space_xxs()),
                    divider::horizontal::light(),
                    cosmic::widget::text::title4("Navigation"),
                    row![
                        cosmic::widget::text::body("Tab / Shift+Tab").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Next/Previous element")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(space_xxs()),
                    row![
                        cosmic::widget::text::body("Arrow Keys").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Navigate elements")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(space_xxs()),
                    row![
                        cosmic::widget::text::body("Enter / Space").width(Length::FillPortion(2)),
                        cosmic::widget::text::body("Activate focused element")
                            .width(Length::FillPortion(3)),
                    ]
                    .spacing(space_xxs()),
                ]
                .spacing(space_xxs()),
            ]
            .spacing(space_xxs());

            content = column![
                container(shortcuts_content)
                    .padding(space_xs())
                    .class(cosmic::theme::Container::Card),
                content
            ]
            .spacing(space_xxs())
            .into();
        }

        if let Some(notification) = &self.notification {
            let icon_name = match notification.kind {
                NotificationType::Error => "dialog-error-symbolic",
                NotificationType::Success => "emblem-ok-symbolic",
                NotificationType::Info => "dialog-information-symbolic",
            };

            let mut notification_row = row![
                icon::from_name(icon_name),
                text(notification.message.clone()).width(Length::Fill),
            ]
            .spacing(space_xxs())
            .align_y(cosmic::iced::Alignment::Center);

            if let Some((label, msg)) = &notification.action {
                notification_row = notification_row.push(
                    button::text(label)
                        .on_press(Message::Loop(msg.clone()))
                        .padding(space_xxxs()),
                );
            }

            column![
                container(
                    container(notification_row)
                        .padding(space_xxs())
                        .class(cosmic::theme::Container::Card)
                )
                .height(Length::Fixed(self.notification_progress * 50.0))
                .clip(true),
                content
            ]
            .spacing(if self.notification_progress > 0.0 {
                space_xxs()
            } else {
                space_none()
            })
            .into()
        } else if !self.daemon_connected {
            column![
                container(
                    row![
                        icon::from_name("dialog-warning-symbolic").size(ICON_XS),
                        cosmic::widget::text::caption("Disconnected from background daemon"),
                    ]
                    .spacing(space_xxs())
                    .align_y(cosmic::iced::Alignment::Center)
                )
                .width(Length::Fill)
                .padding(space_xxxs())
                .class(cosmic::theme::Container::Card),
                content
            ]
            .spacing(space_xxs())
            .into()
        } else {
            content
        }
    }

    pub fn inner_view(&self) -> Element<'_, Message> {
        // App Continuity dialog (Open on Phone)
        if let Some(device_id) = &self.open_url_dialog_device {
            return self.open_url_dialog_view(device_id);
        }
        // SMS dialog
        if let Some(device_id) = &self.sms_dialog_device {
            return self.sms_dialog_view(device_id);
        }

        // Settings overrides
        if let Some(device_id) = &self.remotedesktop_settings_device {
            if let Some(settings) = self.remotedesktop_settings.get(device_id) {
                return self.remotedesktop_settings_view(device_id, settings);
            }
        }
        if let Some(device_id) = &self.file_sync_settings_device {
            return self.file_sync_settings_view(device_id);
        }
        if let Some(device_id) = &self.run_command_settings_device {
            return self.run_command_settings_view(device_id);
        }

        if let ViewMode::DeviceDetails(device_id) = &self.view_mode {
            return self.device_details_view(device_id);
        }
        if self.view_mode == ViewMode::TransferQueue {
            return self.transfer_queue_view();
        }

        let view_switcher = row![
            button::text("Devices")
                .on_press(Message::SetViewMode(ViewMode::Devices))
                .width(Length::Fill),
            button::text("History")
                .on_press(Message::SetViewMode(ViewMode::History))
                .width(Length::Fill)
        ]
        .spacing(space_xxxs())
        .width(Length::Fill);

        if self.view_mode == ViewMode::History {
            return column![
                view_switcher,
                divider::horizontal::default(),
                self.history_view()
            ]
            .spacing(space_xxs())
            .padding(space_xs())
            .into();
        }

        let search_input = cosmic::widget::tooltip(
            cosmic::widget::text_input("Search devices...", &self.search_query)
                .on_input(Message::SearchChanged)
                .width(Length::Fill),
            "Search devices (Ctrl+F)",
            cosmic::widget::tooltip::Position::Bottom,
        );

        let header = row![view_switcher,]
            .spacing(space_xxs())
            .align_y(cosmic::iced::Alignment::Center)
            .width(Length::Fill);

        let controls = if self.scanning {
            row![
                search_input,
                container(
                    row![
                        icon::from_name("process-working-symbolic").size(ICON_S),
                        cosmic::widget::text::caption("Scanning..."),
                    ]
                    .spacing(space_xxs())
                    .align_y(cosmic::iced::Alignment::Center)
                )
                .padding(space_xxxs())
            ]
            .spacing(space_xxs())
        } else {
            row![
                search_input,
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("view-refresh-symbolic"))
                        .on_press(Message::RefreshDevices)
                        .padding(space_xxxs()),
                    "Refresh devices (Ctrl+R)",
                    cosmic::widget::tooltip::Position::Bottom,
                ),
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("help-about-symbolic").size(ICON_S))
                        .on_press(Message::ToggleKeyboardShortcutsHelp)
                        .padding(space_xxxs()),
                    "Keyboard shortcuts",
                    cosmic::widget::tooltip::Position::Bottom,
                ),
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("preferences-desktop-apps-symbolic").size(ICON_S))
                        .on_press(Message::OpenManager)
                        .padding(space_xxxs()),
                    "Open Manager (Ctrl+M)",
                    cosmic::widget::tooltip::Position::Bottom,
                )
            ]
            .spacing(space_xxs())
        };

        // MPRIS media controls section
        let mpris_section = self.mpris_controls_view();

        // Camera streaming controls section
        let camera_section = self.camera_controls_view();

        let content: Element<'_, Message> = if self.devices.is_empty() {
            container(
                column![
                    container(icon::from_name("phone-disconnected-symbolic").size(ICON_XL))
                        .padding(Padding::new(0.0).bottom(space_xs())),
                    cosmic::widget::text::heading("No Devices Connected"),
                    column![
                        cosmic::widget::text::body("Make sure your devices are:"),
                        cosmic::widget::text::caption("• On the same network"),
                        cosmic::widget::text::caption("• Running the CConnect app"),
                    ]
                    .spacing(space_xxs())
                    .align_x(Horizontal::Center),
                    container(
                        button::text("Refresh Devices")
                            .on_press(Message::RefreshDevices)
                            .padding(space_xxs())
                    )
                    .padding(Padding::new(0.0).top(space_xs())),
                ]
                .spacing(space_xxs())
                .align_x(Horizontal::Center),
            )
            .padding(space_m())
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(cosmic::iced::Alignment::Center)
            .into()
        } else {
            // Group devices by category
            let mut connected = Vec::new();
            let mut available = Vec::new();
            let mut offline = Vec::new();

            for device_state in &self.devices {
                // Filter logic
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    let name_match = device_state
                        .device
                        .info
                        .device_name
                        .to_lowercase()
                        .contains(&q);
                    let type_match =
                        super::device::device_type_icon(device_state.device.info.device_type)
                            .contains(&q); // rough proxy
                    if !name_match && !type_match {
                        continue;
                    }
                }

                match categorize_device(device_state) {
                    DeviceCategory::Connected => connected.push(device_state),
                    DeviceCategory::Available => available.push(device_state),
                    DeviceCategory::Offline => offline.push(device_state),
                }
            }

            // Sort each category: pinned devices first
            let sort_by_pinned = |a: &&crate::state::DeviceState, b: &&crate::state::DeviceState| {
                let a_pinned = self
                    .pinned_devices_config
                    .is_pinned(&a.device.info.device_id);
                let b_pinned = self
                    .pinned_devices_config
                    .is_pinned(&b.device.info.device_id);
                match (a_pinned, b_pinned) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                }
            };

            connected.sort_by(sort_by_pinned);
            available.sort_by(sort_by_pinned);
            offline.sort_by(sort_by_pinned);

            let mut device_groups = column![].spacing(space_xxxs()).width(Length::Fill);
            // Track device index for focus navigation (matches filtered_devices() order)
            let mut device_index = 0usize;

            // Connected devices section
            if !connected.is_empty() {
                device_groups = device_groups.push(
                    container(cosmic::widget::text::caption("Connected"))
                        .padding(Padding::from([space_xxs(), space_xs(), space_xxxs(), space_xs()]))
                        .width(Length::Fill),
                );
                for device_state in &connected {
                    device_groups = device_groups.push(self.device_row(device_state, device_index));
                    device_index += 1;
                }
            }

            // Available devices section
            if !available.is_empty() {
                if !connected.is_empty() {
                    device_groups = device_groups.push(divider::horizontal::default());
                }
                device_groups = device_groups.push(
                    container(cosmic::widget::text::caption("Available"))
                        .padding(Padding::from([space_xxs(), space_xs(), space_xxxs(), space_xs()]))
                        .width(Length::Fill),
                );
                for device_state in &available {
                    device_groups = device_groups.push(self.device_row(device_state, device_index));
                    device_index += 1;
                }
            }

            // Offline devices section
            if !offline.is_empty() {
                if !connected.is_empty() || !available.is_empty() {
                    device_groups = device_groups.push(divider::horizontal::default());
                }
                device_groups = device_groups.push(
                    container(cosmic::widget::text::caption("Offline"))
                        .padding(Padding::from([space_xxs(), space_xs(), space_xxxs(), space_xs()]))
                        .width(Length::Fill),
                );
                for device_state in &offline {
                    device_groups = device_groups.push(self.device_row(device_state, device_index));
                    device_index += 1;
                }
            }

            device_groups.into()
        };

        let content = content;

        let popup_content = column![
            container(header)
                .padding(Padding::from([space_xxs(), space_xs()]))
                .width(Length::Fill),
            container(controls)
                .padding(Padding::from([0, space_xs(), space_xxs(), space_xs()]))
                .width(Length::Fill),
            divider::horizontal::default(),
            mpris_section,
            camera_section,
            self.transfers_view(),
            divider::horizontal::default(),
            scrollable(content).height(Length::Fill),
        ]
        .width(Length::Fill);

        container(popup_content)
            .padding(0)
            .width(Length::Fixed(360.0))
            .height(Length::Shrink)
            .into()
    }
}
