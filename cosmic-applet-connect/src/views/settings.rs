use cosmic::{
    iced::{
        alignment::Horizontal,
        widget::{column, container, row},
        Alignment, Length,
    },
    theme,
    widget::{button, divider, icon, radio, text, text_input},
    Element,
};

use crate::{
    dbus_client, horizontal_space, messages::OperationType, space_xxs, space_xs, space_xxxs,
    theme_destructive_color, theme_muted_color, CConnectApplet, Message, ICON_14, ICON_S,
};

impl CConnectApplet {
    pub fn file_sync_settings_view(&self, device_id: &str) -> Element<'_, Message> {
        // Header with close button
        let header = row![
            cosmic::widget::text::body("File Sync Settings"),
            horizontal_space(),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_14))
                    .on_press(Message::CloseFileSyncSettings)
                    .padding(space_xxxs()),
                "Close settings",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .width(Length::Fill)
        .align_y(cosmic::iced::Alignment::Center);

        let mut content = column![header].spacing(space_xs());

        // List existing sync folders
        if let Some(folders) = self.sync_folders.get(device_id) {
            if !folders.is_empty() {
                let mut list = column![].spacing(space_xxs());
                for folder in folders {
                    let strategy_text = match folder.strategy.as_str() {
                        "LastModifiedWins" => "Last Modified",
                        "KeepBoth" => "Keep Both",
                        "Manual" => "Manual",
                        s => s,
                    };

                    let row = row![
                        column![
                            cosmic::widget::text::body(&folder.folder_id),
                            cosmic::widget::text::caption(&folder.path),
                            cosmic::widget::text::caption(format!("Conflict: {}", strategy_text)),
                        ]
                        .spacing(space_xxxs()),
                        horizontal_space(),
                        cosmic::widget::tooltip(
                            button::icon(icon::from_name("user-trash-symbolic").size(ICON_S))
                                .on_press(Message::RemoveSyncFolder(
                                    device_id.to_string(),
                                    folder.folder_id.clone(),
                                ))
                                .padding(space_xxs()),
                            "Remove sync folder",
                            cosmic::widget::tooltip::Position::Bottom,
                        )
                    ]
                    .align_y(cosmic::iced::Alignment::Center)
                    .width(Length::Fill);

                    list = list.push(
                        container(row)
                            .padding(space_xxs())
                            .class(cosmic::theme::Container::Card),
                    );
                }
                content = content.push(list);
            } else {
                content = content.push(
                    container(cosmic::widget::text::caption("No sync folders configured"))
                        .padding(space_xxs())
                        .width(Length::Fill)
                        .align_x(Horizontal::Center),
                );
            }
        } else {
            content = content.push(
                container(
                    row![
                        icon::from_name("process-working-symbolic").size(ICON_S),
                        cosmic::widget::text::caption("Loading..."),
                    ]
                    .spacing(space_xxs())
                    .align_y(cosmic::iced::Alignment::Center),
                )
                .padding(space_xxs())
                .width(Length::Fill)
                .align_x(Horizontal::Center),
            );
        }

        content = content.push(divider::horizontal::default());

        // Add New Folder Form
        if self.add_sync_folder_device.as_deref() == Some(device_id) {
            let strategy_idx = match self.add_sync_folder_strategy.as_str() {
                "last_modified_wins" => 0,
                "keep_both" => 1,
                "manual" => 2,
                _ => 0,
            };

            let form = column![
                cosmic::widget::text::title3("Add Sync Folder"),
                text_input("Local Path", &self.add_sync_folder_path)
                    .on_input(Message::UpdateSyncFolderPathInput),
                text_input("Folder ID", &self.add_sync_folder_id)
                    .on_input(Message::UpdateSyncFolderIdInput),
                row![
                    cosmic::widget::text::body("Conflict Strategy:"),
                    cosmic::widget::dropdown(
                        &["Last Modified", "Keep Both", "Manual"],
                        Some(strategy_idx),
                        |idx| {
                            let s = match idx {
                                0 => "last_modified_wins",
                                1 => "keep_both",
                                2 => "manual",
                                _ => "last_modified_wins",
                            }
                            .to_string();
                            Message::UpdateSyncFolderStrategy(s)
                        }
                    )
                ]
                .spacing(space_xxs())
                .align_y(cosmic::iced::Alignment::Center),
                row![
                    button::text("Cancel").on_press(Message::CancelAddSyncFolder),
                    horizontal_space(),
                    if self
                        .pending_operations
                        .contains(&(device_id.to_string(), OperationType::AddSyncFolder))
                    {
                        button::text("Adding...")
                    } else {
                        button::text("Add Folder")
                            .on_press(Message::AddSyncFolder(device_id.to_string()))
                    }
                ]
                .spacing(space_xs())
                .spacing(space_xs())
            ]
            .spacing(space_xxs()); // Distinct background for form

            content = content.push(
                container(form)
                    .padding(space_xxs())
                    .class(cosmic::theme::Container::Card),
            );
        } else {
            content = content.push(
                button::text("Add Synced Folder")
                    .on_press(Message::StartAddSyncFolder(device_id.to_string()))
                    .width(Length::Fill),
            );
        }

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(space_xs())
            .into()
    }

    pub fn run_command_settings_view(&self, device_id: &str) -> Element<'_, Message> {
        let mut content = column![row![
            text::title3("Run Commands").width(Length::Fill),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_S))
                    .on_press(Message::CloseRunCommandSettings)
                    .padding(space_xxs()),
                "Close settings",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .align_y(Alignment::Center)]
        .spacing(space_xs());

        // List existing commands
        if let Some(commands) = self.run_commands.get(device_id) {
            if !commands.is_empty() {
                let mut list = column![].spacing(space_xxs());
                // Sort by name
                let mut sorted_cmds: Vec<_> = commands.iter().collect();
                sorted_cmds.sort_by(|a, b| a.1.name.cmp(&b.1.name));

                for (cmd_id, cmd) in sorted_cmds {
                    let row = row![
                        column![
                            text::body(&cmd.name),
                            text::caption(&cmd.command)
                                .class(cosmic::theme::Text::Color(theme_muted_color(),)),
                        ]
                        .width(Length::Fill),
                        cosmic::widget::tooltip(
                            button::icon(icon::from_name("user-trash-symbolic").size(ICON_S))
                                .on_press(Message::RemoveRunCommand(
                                    device_id.to_string(),
                                    cmd_id.clone(),
                                ))
                                .padding(space_xxs())
                                .class(cosmic::theme::Button::Destructive),
                            "Remove command",
                            cosmic::widget::tooltip::Position::Bottom,
                        )
                    ]
                    .align_y(Alignment::Center)
                    .width(Length::Fill);

                    list = list.push(
                        container(row)
                            .padding(space_xxs())
                            .class(cosmic::theme::Container::Card),
                    );
                }
                content = content.push(list);
            } else {
                content = content.push(
                    container(text::caption("No run commands configured"))
                        .padding(space_xxs())
                        .width(Length::Fill)
                        .align_x(Horizontal::Center),
                );
            }
        } else {
            content = content.push(
                container(text::caption("Loading..."))
                    .padding(space_xxs())
                    .width(Length::Fill)
                    .align_x(Horizontal::Center),
            );
        }

        content = content.push(divider::horizontal::default());

        // Add New Command Form
        if self.add_run_command_device.as_deref() == Some(device_id) {
            let form = column![
                text::title3("Add New Command"),
                text_input("Name (e.g. Lock Screen)", &self.add_run_command_name)
                    .on_input(Message::UpdateRunCommandNameInput),
                text_input(
                    "Command (e.g. loginctl lock-session)",
                    &self.add_run_command_cmd
                )
                .on_input(Message::UpdateRunCommandCmdInput)
                .on_submit({
                    let id = device_id.to_string();
                    move |_| Message::AddRunCommand(id.clone())
                }),
                row![
                    button::text("Cancel")
                        .on_press(Message::CancelAddRunCommand)
                        .width(Length::Fill),
                    if self
                        .pending_operations
                        .contains(&(device_id.to_string(), OperationType::AddRunCommand))
                    {
                        button::text("Adding...")
                            .class(cosmic::theme::Button::Suggested)
                            .width(Length::Fill)
                    } else {
                        button::text("Add Command")
                            .on_press(Message::AddRunCommand(device_id.to_string()))
                            .class(cosmic::theme::Button::Suggested)
                            .width(Length::Fill)
                    }
                ]
                .spacing(space_xxs())
            ]
            .spacing(space_xxs());

            content = content.push(
                container(form)
                    .padding(space_xxs())
                    .class(cosmic::theme::Container::Card),
            );
        } else {
            content = content.push(
                button::text("Add Command")
                    .on_press(Message::StartAddRunCommand(device_id.to_string()))
                    .width(Length::Fill),
            );
        }

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(space_xs())
            .into()
    }

    pub fn remotedesktop_settings_view(
        &self,
        device_id: &str,
        settings: &dbus_client::RemoteDesktopSettings,
    ) -> Element<'_, Message> {
        // Header with close button
        let header = row![
            cosmic::widget::text::body("Remote Desktop Settings"),
            horizontal_space(),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_14))
                    .on_press(Message::CloseRemoteDesktopSettings)
                    .padding(space_xxxs()),
                "Close settings",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .width(Length::Fill)
        .align_y(cosmic::iced::Alignment::Center);

        // Quality dropdown
        let quality_idx = match settings.quality.as_str() {
            "low" => 0,
            "medium" => 1,
            "high" => 2,
            _ => 1,
        };

        let quality_row = row![
            text("Quality:").width(Length::Fixed(120.0)),
            cosmic::widget::dropdown(&["Low", "Medium", "High"], Some(quality_idx), {
                let device_id = device_id.to_string();
                move |idx| {
                    let quality = match idx {
                        0 => "low",
                        1 => "medium",
                        2 => "high",
                        _ => "medium",
                    }
                    .to_string();
                    Message::UpdateRemoteDesktopQuality(device_id.clone(), quality)
                }
            })
        ]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Center);

        // FPS dropdown
        let fps_idx = match settings.fps {
            15 => 0,
            30 => 1,
            60 => 2,
            _ => 1,
        };

        let fps_row = row![
            text("Frame Rate:").width(Length::Fixed(120.0)),
            cosmic::widget::dropdown(&["15 FPS", "30 FPS", "60 FPS"], Some(fps_idx), {
                let device_id = device_id.to_string();
                move |idx| {
                    let fps = match idx {
                        0 => 15,
                        1 => 30,
                        2 => 60,
                        _ => 30,
                    };
                    Message::UpdateRemoteDesktopFps(device_id.clone(), fps)
                }
            })
        ]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Center);

        // Resolution mode radio buttons
        let is_native = settings.resolution_mode == "native";
        let resolution_radios = column![
            radio(
                "Native Resolution",
                "native",
                Some(settings.resolution_mode.as_str()).filter(|_| is_native),
                {
                    let device_id = device_id.to_string();
                    move |_| {
                        Message::UpdateRemoteDesktopResolution(
                            device_id.clone(),
                            "native".to_string(),
                        )
                    }
                }
            ),
            radio(
                "Custom Resolution",
                "custom",
                Some(settings.resolution_mode.as_str()).filter(|_| !is_native),
                {
                    let device_id = device_id.to_string();
                    move |_| {
                        Message::UpdateRemoteDesktopResolution(
                            device_id.clone(),
                            "custom".to_string(),
                        )
                    }
                }
            ),
        ]
        .spacing(space_xxxs());

        let resolution_row = row![
            text("Resolution:").width(Length::Fixed(120.0)),
            resolution_radios
        ]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Start);

        // Build content
        let mut content = column![
            header,
            divider::horizontal::default(),
            quality_row,
            fps_row,
            resolution_row,
        ]
        .spacing(space_xs());

        // Add custom resolution inputs if mode is "custom"
        if settings.resolution_mode == "custom" {
            let width_input =
                cosmic::widget::text_input("Width (e.g. 1920)", &self.remotedesktop_width_input)
                    .on_input({
                        let device_id = device_id.to_string();
                        move |s| Message::UpdateRemoteDesktopCustomWidth(device_id.clone(), s)
                    });

            let height_input =
                cosmic::widget::text_input("Height (e.g. 1080)", &self.remotedesktop_height_input)
                    .on_input({
                        let device_id = device_id.to_string();
                        move |s| Message::UpdateRemoteDesktopCustomHeight(device_id.clone(), s)
                    });

            let inputs_row = row![
                column![cosmic::widget::text::caption("Width"), width_input]
                    .spacing(space_xxxs())
                    .width(Length::FillPortion(1)),
                column![cosmic::widget::text::caption("Height"), height_input]
                    .spacing(space_xxxs())
                    .width(Length::FillPortion(1)),
            ]
            .spacing(space_xs());

            content = content.push(inputs_row);
        }

        content = content.push(divider::horizontal::default());

        // Error message if any
        if let Some(error) = &self.remotedesktop_error {
            content = content.push(
                cosmic::widget::text::caption(error)
                    .class(theme::Text::Color(theme_destructive_color())),
            );
        }

        // Apply button (disabled if error)
        let mut apply_btn = button::text("Apply Settings").padding(space_xxs());

        if self.remotedesktop_error.is_none() {
            apply_btn =
                apply_btn.on_press(Message::SaveRemoteDesktopSettings(device_id.to_string()));
        }

        content = content.push(apply_btn);

        container(content).padding(space_xs()).into()
    }

    /// Camera settings view with camera selection, resolution, and streaming controls
    pub fn camera_settings_view(&self, device_id: &str) -> Element<'_, Message> {
        // Header with close button
        let header = row![
            cosmic::widget::text::body("Camera Settings"),
            horizontal_space(),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_14))
                    .on_press(Message::CloseCameraSettings)
                    .padding(space_xxxs()),
                "Close settings",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .width(Length::Fill)
        .align_y(cosmic::iced::Alignment::Center);

        // Get camera stats if available
        let stats = self.camera_stats.get(device_id);
        let is_streaming = stats.is_some_and(|s| s.is_streaming);

        // Camera selection dropdown
        let camera_idx = stats.map_or(0, |s| if s.camera_id == 0 { 0 } else { 1 });
        let camera_row = row![
            text("Camera:").width(Length::Fixed(120.0)),
            cosmic::widget::dropdown(&["Back Camera", "Front Camera"], Some(camera_idx), {
                let device_id = device_id.to_string();
                move |idx| {
                    let camera_id = if idx == 0 { 0 } else { 1 };
                    Message::SelectCamera(device_id.clone(), camera_id)
                }
            })
        ]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Center);

        // Resolution dropdown
        let resolution_idx = stats.map_or(1, |s| match s.resolution.as_str() {
            "480p" => 0,
            "720p" => 1,
            "1080p" => 2,
            _ => 1,
        });

        let resolution_row = row![
            text("Resolution:").width(Length::Fixed(120.0)),
            cosmic::widget::dropdown(&["480p", "720p", "1080p"], Some(resolution_idx), {
                let device_id = device_id.to_string();
                move |idx| {
                    let resolution = match idx {
                        0 => "480p",
                        1 => "720p",
                        2 => "1080p",
                        _ => "720p",
                    }
                    .to_string();
                    Message::SelectCameraResolution(device_id.clone(), resolution)
                }
            })
        ]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Center);

        // Build content
        let mut content = column![
            header,
            divider::horizontal::default(),
            camera_row,
            resolution_row
        ]
        .spacing(space_xs());

        // Statistics section (only show when streaming)
        if is_streaming {
            if let Some(stats) = stats {
                content = content.push(divider::horizontal::default());

                let stats_section = column![
                    cosmic::widget::text::caption("Stream Statistics:"),
                    row![
                        column![
                            cosmic::widget::text::caption("FPS:"),
                            text(format!("{}", stats.fps)),
                        ]
                        .spacing(space_xxxs())
                        .width(Length::FillPortion(1)),
                        column![
                            cosmic::widget::text::caption("Bitrate:"),
                            text(format!("{} kbps", stats.bitrate)),
                        ]
                        .spacing(space_xxxs())
                        .width(Length::FillPortion(1)),
                        column![
                            cosmic::widget::text::caption("Current:"),
                            text(&stats.resolution),
                        ]
                        .spacing(space_xxxs())
                        .width(Length::FillPortion(1)),
                    ]
                    .spacing(space_xs()),
                ]
                .spacing(space_xxs());

                content = content.push(stats_section);
            }
        }

        content = content.push(divider::horizontal::default());

        // Start/Stop streaming button
        let streaming_button = if is_streaming {
            button::text("Stop Streaming")
                .on_press(Message::ToggleCameraStreaming(device_id.to_string()))
                .padding(space_xxs())
                .class(cosmic::theme::Button::Destructive)
        } else {
            button::text("Start Streaming")
                .on_press(Message::ToggleCameraStreaming(device_id.to_string()))
                .padding(space_xxs())
                .class(cosmic::theme::Button::Suggested)
        };

        content = content.push(streaming_button);

        // Helper text
        let helper_text = if is_streaming {
            cosmic::widget::text::caption("Camera is available at /dev/video10")
        } else {
            cosmic::widget::text::caption("Start streaming to use phone camera as webcam")
        };
        content = content.push(helper_text);

        container(content).padding(space_xs()).into()
    }
}
