use cosmic::{
    iced::{
        alignment::Horizontal,
        widget::{column, container, row}, Length, Padding,
    },
    theme,
    widget::{button, divider, icon, text},
    Element,
};

use cosmic_connect_protocol::{ConnectionState, Device, DeviceType, PairingStatus};

use crate::{
    horizontal_space, messages::OperationType, space_xxs, space_xxs_f32,
    space_xs, space_xxxs, state::*, theme_destructive_color, theme_muted_color,
    theme_success_color, theme_warning_color, CConnectApplet, Message, ICON_L, ICON_S, ICON_XL, ICON_XS,
};

/// Device category for grouping in popup
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceCategory {
    Connected,
    Available,
    Offline,
}

impl CConnectApplet {
    pub fn device_details_view(&self, device_id: &str) -> Element<'_, Message> {
        let device_state = self
            .devices
            .iter()
            .find(|d| d.device.info.device_id == device_id);

        let Some(device_state) = device_state else {
            return container(text("Device not found")).padding(space_xs()).into();
        };

        let device = &device_state.device;
        let _config = self.device_configs.get(device_id);

        let header = row![
            cosmic::widget::tooltip(
                button::icon(icon::from_name("go-previous-symbolic").size(ICON_S))
                    .on_press(Message::CloseDeviceDetails)
                    .padding(space_xxs()),
                "Back",
                cosmic::widget::tooltip::Position::Bottom,
            ),
            cosmic::widget::text::title4(&device.info.device_name),
            horizontal_space(),
        ]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Center);

        // Basic Info
        let info_card = column![
            row![
                text("Status:").width(Length::Fixed(100.0)),
                connection_status_styled_text(device.connection_state, device.pairing_status)
            ]
            .spacing(space_xxs()),
            row![
                text("Type:").width(Length::Fixed(100.0)),
                text(format!("{:?}", device.info.device_type))
            ]
            .spacing(space_xxs()),
            row![
                text("IP Address:").width(Length::Fixed(100.0)),
                text(device.host.as_deref().unwrap_or("Unknown"))
            ]
            .spacing(space_xxs()),
            row![
                text("ID:").width(Length::Fixed(100.0)),
                text(if device_id.len() > 20 {
                    format!("{}...", &device_id[..20])
                } else {
                    device_id.to_string()
                })
            ]
            .spacing(space_xxs()),
        ]
        .spacing(space_xxs());

        let mut content = column![
            header,
            container(info_card)
                .padding(space_xs())
                .width(Length::Fill)
                .class(cosmic::theme::Container::Card),
        ]
        .spacing(space_xs());

        // System Info card (if available)
        if let Some(info) = self.system_info.get(device_id) {
            let system_info_card = column![
                row![text("System Information")
                    .size(ICON_S)
                    .class(theme::Text::Color(crate::theme_accent_color()))]
                .spacing(space_xxs()),
                divider::horizontal::default(),
                row![
                    text("CPU Usage:").width(Length::Fixed(120.0)),
                    text(format!("{:.1}%", info.cpu_usage))
                ]
                .spacing(space_xxs()),
                row![
                    text("Memory Usage:").width(Length::Fixed(120.0)),
                    text(format!(
                        "{:.1}% ({} / {} MB)",
                        info.memory_usage,
                        info.used_memory / 1024 / 1024,
                        info.total_memory / 1024 / 1024
                    ))
                ]
                .spacing(space_xxs()),
                row![
                    text("Disk Usage:").width(Length::Fixed(120.0)),
                    text(format!("{:.1}%", info.disk_usage))
                ]
                .spacing(space_xxs()),
                row![
                    text("Uptime:").width(Length::Fixed(120.0)),
                    text(format_uptime(info.uptime))
                ]
                .spacing(space_xxs()),
            ]
            .spacing(space_xxs());

            content = content.push(
                container(system_info_card)
                    .padding(space_xs())
                    .width(Length::Fill)
                    .class(cosmic::theme::Container::Card),
            );
        } else if device.is_connected() {
            // Show button to request system info
            content = content.push(
                container(
                    button::standard("Request System Info")
                        .on_press(Message::RequestSystemInfo(device_id.to_string())),
                )
                .padding(space_xs())
                .width(Length::Fill)
                .class(cosmic::theme::Container::Card),
            );
        }

        content.padding(space_xs()).into()
    }

    pub fn device_row<'a>(
        &'a self,
        device_state: &'a DeviceState,
        device_index: usize,
    ) -> Element<'a, Message> {
        let device = &device_state.device;
        let device_id = &device.info.device_id;

        // Check if this device is focused for keyboard navigation
        let is_focused = matches!(&self.focus_target,
            FocusTarget::Device(idx) |
            FocusTarget::DeviceAction(idx, _)
            if *idx == device_index
        );

        let device_icon = device_type_icon(device.info.device_type);

        // Device name
        let nickname = self
            .device_configs
            .get(device_id)
            .and_then(|c| c.nickname.as_deref());

        let display_name = nickname.unwrap_or(&device.info.device_name);

        // Metadata row: Status • Battery • Last Seen
        let mut metadata_row = row![connection_status_styled_text(
            device.connection_state,
            device.pairing_status
        )]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Center);

        // Add battery if available
        if let Some(level) = device_state.battery_level {
            metadata_row = metadata_row.push(
                text("•")
                    .size(ICON_XS)
                    .class(theme::Text::Color(theme_muted_color())),
            );

            let battery_icon = battery_icon_name(level, device_state.is_charging);
            metadata_row = metadata_row.push(
                row![
                    icon::from_name(battery_icon).size(ICON_XS),
                    cosmic::widget::text::caption(format!("{}%", level)),
                ]
                .spacing(space_xxxs())
                .align_y(cosmic::iced::Alignment::Center),
            );
        } else if self.loading_battery && device.is_connected() {
            metadata_row = metadata_row.push(
                text("•")
                    .size(ICON_XS)
                    .class(theme::Text::Color(theme_muted_color())),
            );
            metadata_row =
                metadata_row.push(icon::from_name("process-working-symbolic").size(ICON_XS));
        }

        // Add last seen if disconnected
        if !device.is_connected() && device.last_seen > 0 {
            let last_seen_text = format_last_seen(device.last_seen);
            metadata_row = metadata_row.push(
                text("•")
                    .size(ICON_XS)
                    .class(theme::Text::Color(theme_muted_color())),
            );
            metadata_row = metadata_row.push(cosmic::widget::text::caption(last_seen_text));
        }

        // Combine Name + Metadata
        let info_col = column![cosmic::widget::text::heading(display_name), metadata_row]
            .spacing(space_xxxs())
            .width(Length::Fill);

        // Pin/favorite button
        let is_pinned = self.pinned_devices_config.is_pinned(device_id);
        let star_icon = if is_pinned {
            "starred-symbolic"
        } else {
            "non-starred-symbolic"
        };
        let star_button = cosmic::widget::tooltip(
            button::icon(icon::from_name(star_icon).size(ICON_S))
                .on_press(Message::ToggleDevicePin(device_id.to_string()))
                .padding(space_xxxs())
                .class(cosmic::theme::Button::Icon),
            if is_pinned {
                "Unpin device"
            } else {
                "Pin device"
            },
            cosmic::widget::tooltip::Position::Bottom,
        );

        // Build actions
        let actions_row = self.build_device_actions(device, device_id);

        // Main device row layout
        let mut content = column![
            row![
                container(icon::from_name(device_icon).size(ICON_XL))
                    .width(Length::Fixed(f32::from(theme::active().cosmic().space_xxl())))
                    .align_x(Horizontal::Center)
                    .padding(Padding::new(space_xxs_f32())),
                info_col,
                star_button,
            ]
            .spacing(space_xxs())
            .align_y(cosmic::iced::Alignment::Center)
            .width(Length::Fill),
            // Actions row below
            container(actions_row)
                .width(Length::Fill)
                .padding(Padding::new(0.0).left(48.0 + space_xxs_f32())) // Indent to align with text
                .align_x(Horizontal::Left),
        ]
        .spacing(space_xxs())
        .padding(space_xs())
        .width(Length::Fill);

        // Add RemoteDesktop settings panel if active
        if self.remotedesktop_settings_device.as_ref() == Some(device_id) {
            if let Some(settings) = self.remotedesktop_settings.get(device_id) {
                content = content.push(
                    container(self.remotedesktop_settings_view(device_id, settings))
                        .padding(Padding::from([0.0, 0.0, 0.0, 48.0 + space_xxs_f32()])),
                );
            }
        }

        // Add FileSync settings panel if active
        if self.file_sync_settings_device.as_ref() == Some(device_id) {
            content = content.push(
                container(self.file_sync_settings_view(device_id)).padding(Padding::from([
                    0.0,
                    0.0,
                    0.0,
                    48.0 + space_xxs_f32(),
                ])),
            );
        }

        // Add RunCommand settings panel if active
        if self.run_command_settings_device.as_ref() == Some(device_id) {
            content = content.push(
                container(self.run_command_settings_view(device_id)).padding(Padding::from([
                    0.0,
                    0.0,
                    0.0,
                    48.0 + space_xxs_f32(),
                ])),
            );
        }

        // Add Camera settings panel if active
        if self.camera_settings_device.as_ref() == Some(device_id) {
            content = content.push(
                container(self.camera_settings_view(device_id)).padding(Padding::from([
                    0.0,
                    0.0,
                    0.0,
                    48.0 + space_xxs_f32(),
                ])),
            );
        }

        // Add context menu if open for this device
        if self.context_menu_device.as_ref() == Some(device_id) {
            content = content.push(
                container(self.device_context_menu_view(device_id, device))
                    .padding(Padding::from([0.0, 0.0, 0.0, 48.0 + space_xxs_f32()])),
            );
        }

        // Check if this device is a valid drop target
        let can_receive_files = device.is_connected()
            && device.is_paired()
            && device.has_incoming_capability("cconnect.share");
        let show_drop_zone = self.dragging_files && can_receive_files;
        let is_drag_target = show_drop_zone && self.drag_hover_device.as_ref() == Some(device_id);

        // Add drop zone indicator when dragging files
        if show_drop_zone {
            // Enhanced drop zone with better visual feedback
            content = content.push(
                container(
                    column![
                        icon::from_name("document-send-symbolic").size(ICON_L),
                        cosmic::widget::text::body("Drop file here"),
                        cosmic::widget::text::caption("Release to send to this device"),
                    ]
                    .spacing(space_xxs())
                    .align_x(Horizontal::Center),
                )
                .padding(space_xs())
                .width(Length::Fill)
                .align_x(Horizontal::Center)
                .class(if is_drag_target {
                    cosmic::theme::Container::Primary
                } else {
                    cosmic::theme::Container::Secondary
                }),
            );
        }

        // Apply focus/drag indicator styling with enhanced visual feedback
        let container_class = if is_focused || is_drag_target {
            cosmic::theme::Container::Primary
        } else {
            cosmic::theme::Container::Card
        };

        // Wrap in button for click-to-select as drop target when dragging
        if show_drop_zone {
            button::custom(
                container(content)
                    .width(Length::Fill)
                    .class(container_class),
            )
            .on_press(Message::SetDragHoverDevice(Some(device_id.to_string())))
            .padding(0)
            .class(cosmic::theme::Button::Transparent)
            .width(Length::Fill)
            .into()
        } else {
            container(content)
                .width(Length::Fill)
                .class(container_class)
                .into()
        }
    }

    pub fn build_device_actions<'a>(
        &self,
        device: &'a Device,
        device_id: &str,
    ) -> cosmic::iced::widget::Row<'a, Message, cosmic::Theme> {
        let mut actions = row![].spacing(space_xxs());

        // Quick actions for connected & paired devices
        if device.is_connected() && device.is_paired() {
            let is_pinging = self
                .pending_operations
                .contains(&(device_id.to_string(), OperationType::Ping));
            actions = actions.push(action_button_with_tooltip_loading(
                "user-available-symbolic",
                "Send ping",
                Message::SendPing(device_id.to_string()),
                is_pinging,
            ));

            if device.has_incoming_capability("cconnect.share") {
                actions = actions
                    .push(action_button_with_tooltip(
                        "document-send-symbolic",
                        "Send file",
                        Message::SendFile(device_id.to_string()),
                    ))
                    .push(action_button_with_tooltip_loading(
                        "insert-text-symbolic",
                        "Share clipboard text",
                        Message::ShareText(device_id.to_string()),
                        self.pending_operations
                            .contains(&(device_id.to_string(), OperationType::ShareText)),
                    ))
                    .push(action_button_with_tooltip_loading(
                        "send-to-symbolic",
                        "Share URL",
                        Message::ShareUrl(device_id.to_string()),
                        self.pending_operations
                            .contains(&(device_id.to_string(), OperationType::ShareUrl)),
                    ))
                    .push(action_button_with_tooltip(
                        "smartphone-symbolic",
                        "Open on Phone (App Continuity)",
                        Message::ShowOpenUrlDialog(device_id.to_string()),
                    ));
            }

            // Add Find My Phone if supported
            if device.has_incoming_capability("cconnect.findmyphone.request") {
                let is_ringing = self
                    .pending_operations
                    .contains(&(device_id.to_string(), OperationType::FindPhone));
                actions = actions.push(action_button_with_tooltip_loading(
                    "find-location-symbolic",
                    "Ring device",
                    Message::FindPhone(device_id.to_string()),
                    is_ringing,
                ));
            }

            // Lock device button
            if device.has_incoming_capability("cconnect.lock.request") {
                actions = actions.push(action_button_with_tooltip(
                    "system-lock-screen-symbolic",
                    "Lock device",
                    Message::LockDevice(device_id.to_string()),
                ));
            }

            // Power control button (shutdown)
            if device.has_incoming_capability("cconnect.power.request") {
                actions = actions.push(action_button_with_tooltip(
                    "system-shutdown-symbolic",
                    "Shutdown device",
                    Message::PowerAction(device_id.to_string(), "shutdown".to_string()),
                ));
            }

            // Wake-on-LAN button (for offline devices)
            if device.has_incoming_capability("cconnect.wol.request") {
                actions = actions.push(action_button_with_tooltip(
                    "network-wired-symbolic",
                    "Wake device",
                    Message::WakeDevice(device_id.to_string()),
                ));
            }

            // System Volume button
            if device.has_incoming_capability("cconnect.systemvolume.request") {
                actions = actions.push(action_button_with_tooltip(
                    "multimedia-volume-control-symbolic",
                    "Control volume",
                    Message::SetDeviceVolume(device_id.to_string(), 0.5),
                ));
            }

            // System Monitor button
            if device.has_incoming_capability("cconnect.systemmonitor.request") {
                actions = actions.push(action_button_with_tooltip(
                    "utilities-system-monitor-symbolic",
                    "Get system info",
                    Message::RequestSystemInfo(device_id.to_string()),
                ));
            }

            // Screenshot button - only for desktop/laptop devices
            // Android devices don't have a screenshot plugin to handle requests
            if device.has_incoming_capability("cconnect.screenshot.request")
                && matches!(
                    device.info.device_type,
                    DeviceType::Desktop | DeviceType::Laptop
                )
            {
                actions = actions.push(action_button_with_tooltip(
                    "camera-photo-symbolic",
                    "Take screenshot",
                    Message::TakeScreenshot(device_id.to_string()),
                ));
            }

            // Telephony - Mute Call button
            if device.has_incoming_capability("cconnect.telephony") {
                let is_muting = self
                    .pending_operations
                    .contains(&(device_id.to_string(), OperationType::MuteCall));
                actions = actions.push(action_button_with_tooltip_loading(
                    "audio-volume-muted-symbolic",
                    "Mute incoming call",
                    Message::MuteCall(device_id.to_string()),
                    is_muting,
                ));
            }

            // SMS button
            if device.has_incoming_capability("cconnect.sms.messages") {
                actions = actions.push(action_button_with_tooltip(
                    "mail-message-new-symbolic",
                    "Send SMS",
                    Message::ShowSmsDialog(device_id.to_string()),
                ));
            }

            // Audio Stream toggle button
            if device.has_incoming_capability("cconnect.audiostream") {
                let is_streaming = self.audio_streaming_devices.contains(device_id);
                let audio_icon = if is_streaming {
                    "audio-volume-high-symbolic"
                } else {
                    "audio-volume-muted-symbolic"
                };
                let audio_tooltip = if is_streaming {
                    "Stop audio streaming"
                } else {
                    "Start audio streaming"
                };
                actions = actions.push(
                    button::icon(icon::from_name(audio_icon).size(ICON_S))
                        .on_press(Message::ToggleAudioStream(device_id.to_string()))
                        .padding(space_xxxs())
                        .tooltip(audio_tooltip),
                );
            }

            // Presenter mode toggle button
            if device.has_incoming_capability("cconnect.presenter") {
                let is_presenting = self.presenter_mode_devices.contains(device_id);
                let presenter_icon = if is_presenting {
                    "x11-cursor-symbolic"
                } else {
                    "input-touchpad-symbolic"
                };
                let presenter_tooltip = if is_presenting {
                    "Stop presenter mode"
                } else {
                    "Start presenter mode"
                };
                actions = actions.push(
                    button::icon(icon::from_name(presenter_icon).size(ICON_S))
                        .on_press(Message::TogglePresenterMode(device_id.to_string()))
                        .padding(space_xxxs())
                        .tooltip(presenter_tooltip),
                );
            }
            // Battery refresh button
            let is_refreshing_battery = self
                .pending_operations
                .contains(&(device_id.to_string(), OperationType::Battery));
            actions = actions.push(action_button_with_tooltip_loading(
                "view-refresh-symbolic",
                "Refresh battery status",
                Message::RequestBatteryUpdate(device_id.to_string()),
                is_refreshing_battery,
            ));

            // Screen Mirroring button
            if device.has_outgoing_capability("cconnect.screenshare") {
                actions = actions.push(action_button_with_tooltip(
                    "video-display-symbolic",
                    "Mirror Screen",
                    Message::LaunchScreenMirror(device_id.to_string()),
                ));
            }

            // Remote Desktop button
            if device.has_incoming_capability("cconnect.remotedesktop.request") {
                actions = actions.push(action_button_with_tooltip(
                    "preferences-desktop-remote-desktop-symbolic",
                    "Remote Desktop",
                    Message::ShowRemoteDesktopSettings(device_id.to_string()),
                ));
            }

            // Camera streaming toggle button
            if device.has_incoming_capability("cconnect.camera") {
                let is_streaming = self
                    .camera_stats
                    .get(device_id)
                    .is_some_and(|s| s.is_streaming);
                actions = actions.push(
                    cosmic::widget::button::icon(if is_streaming {
                        cosmic::widget::icon::from_name("camera-web-symbolic").size(ICON_S)
                    } else {
                        cosmic::widget::icon::from_name("camera-disabled-symbolic").size(ICON_S)
                    })
                    .on_press(Message::ToggleCameraStreaming(device_id.to_string()))
                    .padding(space_xxxs())
                    .tooltip(if is_streaming {
                        "Stop camera streaming"
                    } else {
                        "Start camera streaming"
                    }),
                );
            }

            // Run Commands button
            if device.has_incoming_capability("cconnect.runcommand") {
                actions = actions.push(action_button_with_tooltip(
                    "utilities-terminal-symbolic",
                    "Run Commands",
                    Message::ShowRunCommandSettings(device_id.to_string()),
                ));
            }
        }

        // Device details and manager button (for paired devices)
        if device.is_paired() {
            actions = actions.push(action_button_with_tooltip(
                "document-properties-symbolic",
                "Device Details",
                Message::ShowDeviceDetails(device_id.to_string()),
            ));

            actions = actions.push(action_button_with_tooltip(
                "preferences-system-symbolic",
                "Open Manager",
                Message::LaunchManager(device_id.to_string()),
            ));
        }

        // Pair/Unpair button
        let (label, message, is_loading) = if device.is_paired() {
            (
                "Unpair",
                Message::UnpairDevice(device_id.to_string()),
                self.pending_operations
                    .contains(&(device_id.to_string(), OperationType::Unpair)),
            )
        } else {
            (
                "Pair",
                Message::PairDevice(device_id.to_string()),
                self.pending_operations
                    .contains(&(device_id.to_string(), OperationType::Pair)),
            )
        };

        if is_loading {
            actions = actions.push(cosmic::widget::tooltip(
                button::icon(icon::from_name("process-working-symbolic").size(ICON_S))
                    .padding(space_xxs()),
                if label == "Pair" {
                    "Pairing..."
                } else {
                    "Unpairing..."
                },
                cosmic::widget::tooltip::Position::Bottom,
            ));
        } else {
            actions = actions.push(button::text(label).on_press(message).padding(space_xxs()));
        }

        // Context menu button (more options)
        let is_menu_open = self.context_menu_device.as_ref() == Some(&device_id.to_string());
        actions = actions.push(cosmic::widget::tooltip(
            button::icon(
                icon::from_name(if is_menu_open {
                    "go-up-symbolic"
                } else {
                    "view-more-symbolic"
                })
                .size(ICON_S),
            )
            .on_press(if is_menu_open {
                Message::CloseContextMenu
            } else {
                Message::ShowContextMenu(device_id.to_string())
            })
            .padding(space_xxs())
            .class(if is_menu_open {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Standard
            }),
            "More options",
            cosmic::widget::tooltip::Position::Bottom,
        ));

        actions
    }

    pub fn device_context_menu_view<'a>(
        &'a self,
        device_id: &str,
        device: &'a Device,
    ) -> Element<'a, Message> {
        // Helper to create consistent menu items
        let menu_item = |icon_name: &'a str,
                         label: &'a str,
                         message: Message,
                         style: cosmic::theme::Button|
         -> Element<'a, Message> {
            button::custom(
                row![icon::from_name(icon_name).size(ICON_S), text(label),]
                    .spacing(space_xxs())
                    .align_y(cosmic::iced::Alignment::Center),
            )
            .on_press(message)
            .padding(space_xxs())
            .width(Length::Fill)
            .class(style)
            .into()
        };

        let mut menu_items: Vec<Element<'a, Message>> = Vec::new();

        // Header
        menu_items.push(
            container(cosmic::widget::text::caption("Quick Actions"))
                .padding(Padding::from([space_xxs(), space_xxs()]))
                .into(),
        );
        menu_items.push(divider::horizontal::default().into());

        // Connected device actions
        if device.is_connected() && device.is_paired() {
            menu_items.push(menu_item(
                "document-edit-symbolic",
                "Rename device",
                Message::StartRenaming(device_id.to_string()),
                cosmic::theme::Button::MenuItem,
            ));

            if device.has_incoming_capability("cconnect.share") {
                menu_items.push(menu_item(
                    "document-send-symbolic",
                    "Send file...",
                    Message::SendFile(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
                menu_items.push(menu_item(
                    "folder-documents-symbolic",
                    "Send multiple files...",
                    Message::SendFiles(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            if device.has_incoming_capability("cconnect.findmyphone.request") {
                menu_items.push(menu_item(
                    "find-location-symbolic",
                    "Ring device",
                    Message::FindPhone(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            if device.has_outgoing_capability("cconnect.screenshare") {
                menu_items.push(menu_item(
                    "video-display-symbolic",
                    "Mirror screen",
                    Message::LaunchScreenMirror(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            if device.has_incoming_capability("cconnect.remotedesktop.request") {
                menu_items.push(menu_item(
                    "preferences-desktop-remote-desktop-symbolic",
                    "Remote Desktop",
                    Message::ShowRemoteDesktopSettings(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            if device.has_incoming_capability("cconnect.camera") {
                menu_items.push(menu_item(
                    "camera-web-symbolic",
                    "Toggle camera",
                    Message::ToggleCameraStreaming(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            if device.has_incoming_capability("cconnect.runcommand") {
                menu_items.push(menu_item(
                    "utilities-terminal-symbolic",
                    "Run Commands",
                    Message::ShowRunCommandSettings(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            if device.has_incoming_capability("cconnect.sms.messages") {
                menu_items.push(menu_item(
                    "mail-message-new-symbolic",
                    "SMS Conversations",
                    Message::ShowConversations(device_id.to_string()),
                    cosmic::theme::Button::MenuItem,
                ));
            }

            menu_items.push(divider::horizontal::default().into());
        }

        // Settings section
        if device.is_paired() {
            menu_items.push(menu_item(
                "document-properties-symbolic",
                "Device details",
                Message::ShowDeviceDetails(device_id.to_string()),
                cosmic::theme::Button::MenuItem,
            ));

            menu_items.push(menu_item(
                "preferences-system-symbolic",
                "Open Manager",
                Message::LaunchManager(device_id.to_string()),
                cosmic::theme::Button::MenuItem,
            ));

            menu_items.push(divider::horizontal::default().into());

            menu_items.push(menu_item(
                "edit-delete-symbolic",
                "Unpair device",
                Message::UnpairDevice(device_id.to_string()),
                cosmic::theme::Button::Destructive,
            ));
        } else {
            menu_items.push(menu_item(
                "emblem-default-symbolic",
                "Pair device",
                Message::PairDevice(device_id.to_string()),
                cosmic::theme::Button::Suggested,
            ));
        }

        // Dismiss button for offline, unpaired devices
        if !device.is_connected() && !device.is_paired() {
            menu_items.push(divider::horizontal::default().into());
            menu_items.push(menu_item(
                "user-trash-symbolic",
                "Dismiss device",
                Message::DismissDevice(device_id.to_string()),
                cosmic::theme::Button::Destructive,
            ));
        }

        // Close menu button
        menu_items.push(divider::horizontal::default().into());
        menu_items.push(menu_item(
            "window-close-symbolic",
            "Close menu",
            Message::CloseContextMenu,
            cosmic::theme::Button::MenuItem,
        ));

        container(column(menu_items).spacing(space_xxxs()).width(Length::Fill))
            .padding(space_xxs())
            .width(Length::Fill)
            .class(cosmic::theme::Container::Secondary)
            .into()
    }
}

// Helper functions

/// Creates a small icon button with tooltip
pub(crate) fn action_button_with_tooltip(
    icon_name: &str,
    tooltip_text: &'static str,
    message: Message,
) -> Element<'static, Message> {
    cosmic::widget::tooltip(
        button::icon(icon::from_name(icon_name).size(ICON_S))
            .on_press(message)
            .padding(space_xxs()),
        tooltip_text,
        cosmic::widget::tooltip::Position::Bottom,
    )
    .into()
}

/// Creates a small icon button with tooltip that supports a loading state
pub(crate) fn action_button_with_tooltip_loading(
    icon_name: &str,
    tooltip_text: &'static str,
    message: Message,
    is_loading: bool,
) -> Element<'static, Message> {
    if is_loading {
        cosmic::widget::tooltip(
            button::icon(icon::from_name("process-working-symbolic").size(ICON_S))
                .padding(space_xxs()),
            "Working...",
            cosmic::widget::tooltip::Position::Bottom,
        )
        .into()
    } else {
        action_button_with_tooltip(icon_name, tooltip_text, message)
    }
}

/// Returns the icon name for a device type
pub(crate) fn device_type_icon(device_type: DeviceType) -> &'static str {
    match device_type {
        DeviceType::Phone => "phone-symbolic",
        DeviceType::Tablet => "tablet-symbolic",
        DeviceType::Desktop => "computer-symbolic",
        DeviceType::Laptop => "laptop-symbolic",
        DeviceType::Tv => "tv-symbolic",
    }
}

/// Returns a styled text element with color-coded status text
pub(crate) fn connection_status_styled_text<'a>(
    connection_state: ConnectionState,
    pairing_status: PairingStatus,
) -> Element<'a, Message> {
    let status_text = match (connection_state, pairing_status) {
        (ConnectionState::Connected, _) => "Connected",
        (ConnectionState::Connecting, _) => "Connecting...",
        (ConnectionState::Failed, _) => "Connection failed",
        (ConnectionState::Disconnected, PairingStatus::Paired) => "Disconnected",
        (ConnectionState::Disconnected, _) => "Not paired",
    };

    // Apply color based on connection state using theme-aware colors
    let color = match connection_state {
        ConnectionState::Connected => theme_success_color(),
        ConnectionState::Failed => theme_destructive_color(),
        ConnectionState::Connecting => theme_warning_color(),
        ConnectionState::Disconnected => theme_muted_color(),
    };

    cosmic::widget::text::caption(status_text)
        .class(theme::Text::Color(color))
        .into()
}

/// Returns the appropriate battery icon name based on charge level and charging state
pub(crate) fn battery_icon_name(level: u8, is_charging: bool) -> &'static str {
    if is_charging {
        "battery-good-charging-symbolic"
    } else {
        match level {
            80..=100 => "battery-full-symbolic",
            50..=79 => "battery-good-symbolic",
            20..=49 => "battery-low-symbolic",
            _ => "battery-caution-symbolic",
        }
    }
}

/// Categorize a device based on its state
pub(crate) fn categorize_device(device_state: &DeviceState) -> DeviceCategory {
    let device = &device_state.device;
    if device.is_connected() && device.is_paired() {
        DeviceCategory::Connected
    } else if device.is_reachable() || !device.is_paired() {
        DeviceCategory::Available
    } else {
        DeviceCategory::Offline
    }
}

/// Helper function for pluralization
fn pluralize(count: u64) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

/// Format last seen timestamp to human-readable string
pub(crate) fn format_uptime(uptime_seconds: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    let days = uptime_seconds / DAY;
    let hours = (uptime_seconds % DAY) / HOUR;
    let minutes = (uptime_seconds % HOUR) / MINUTE;

    if days > 0 {
        format!("{}d {}h {}m", days, hours, minutes)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

pub(crate) fn format_last_seen(last_seen: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let elapsed = now.saturating_sub(last_seen);

    match elapsed {
        0..60 => "Just now".to_string(),
        60..3600 => {
            let mins = elapsed / MINUTE;
            format!("{} min{} ago", mins, pluralize(mins))
        }
        3600..86400 => {
            let hours = elapsed / HOUR;
            format!("{} hour{} ago", hours, pluralize(hours))
        }
        _ => {
            let days = elapsed / DAY;
            format!("{} day{} ago", days, pluralize(days))
        }
    }
}
