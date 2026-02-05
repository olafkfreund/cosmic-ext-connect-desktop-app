use cosmic::{
    iced::{
        widget::{column, container, row},
        Padding, Length,
    },
    widget::{button, divider, icon},
    Element,
};

use crate::{
    dbus_client, horizontal_space, space_xxs, space_xs, space_xxxs, CConnectApplet, Message,
    ICON_L, ICON_S,
};

impl CConnectApplet {
    pub fn mpris_controls_view(&self) -> Element<'_, Message> {
        // If no players available, return empty space
        if self.mpris_players.is_empty() {
            return Element::from(cosmic::iced::widget::Space::new(0, 0));
        }

        let Some(selected_player) = &self.selected_player else {
            return Element::from(cosmic::iced::widget::Space::new(0, 0));
        };

        // Player name/label with context menu button
        let menu_message = if self.context_menu_mpris {
            Message::CloseMprisContextMenu
        } else {
            Message::ShowMprisContextMenu
        };

        let menu_button = button::icon(icon::from_name("view-more-symbolic").size(ICON_S))
            .padding(space_xxxs())
            .class(cosmic::theme::Button::Transparent)
            .on_press(menu_message);

        let player_name = row![
            icon::from_name("multimedia-player-symbolic").size(ICON_S),
            cosmic::widget::text::body(selected_player),
            horizontal_space(),
            menu_button,
        ]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Center);

        // Metadata display
        let mut metadata_col = column![];

        let state = self.mpris_states.get(selected_player);

        if let Some(state) = state {
            if let Some(title) = &state.metadata.title {
                metadata_col = metadata_col.push(cosmic::widget::text::body(title));
            }
            if let Some(artist) = &state.metadata.artist {
                metadata_col = metadata_col.push(cosmic::widget::text::body(artist));
            }
            if let Some(album) = &state.metadata.album {
                metadata_col = metadata_col.push(cosmic::widget::text::caption(album));
            }
            // Use album art if available (placeholder for now/Url later)
            // If using libcosmic image support
        } else {
            metadata_col = metadata_col.push(cosmic::widget::text::body("Unknown"));
        }

        // Playback controls
        let status = state
            .map(|s| s.playback_status)
            .unwrap_or(dbus_client::PlaybackStatus::Stopped);

        let (play_icon, play_action) = match status {
            dbus_client::PlaybackStatus::Playing => ("media-playback-pause-symbolic", "Pause"),
            _ => ("media-playback-start-symbolic", "Play"),
        };

        let controls = row![
            cosmic::widget::tooltip(
                button::icon(icon::from_name("media-skip-backward-symbolic").size(ICON_S))
                    .on_press(Message::MprisControl(
                        selected_player.clone(),
                        "Previous".to_string(),
                    ))
                    .padding(space_xxs()),
                "Previous",
                cosmic::widget::tooltip::Position::Bottom,
            ),
            cosmic::widget::tooltip(
                button::icon(icon::from_name(play_icon).size(ICON_S))
                    .on_press(Message::MprisControl(
                        selected_player.clone(),
                        play_action.to_string()
                    ))
                    .padding(space_xxs()),
                play_action,
                cosmic::widget::tooltip::Position::Bottom,
            ),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("media-playback-stop-symbolic").size(ICON_S))
                    .on_press(Message::MprisControl(
                        selected_player.clone(),
                        "Stop".to_string(),
                    ))
                    .padding(space_xxs()),
                "Stop",
                cosmic::widget::tooltip::Position::Bottom,
            ),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("media-skip-forward-symbolic").size(ICON_S))
                    .on_press(Message::MprisControl(
                        selected_player.clone(),
                        "Next".to_string(),
                    ))
                    .padding(space_xxs()),
                "Next",
                cosmic::widget::tooltip::Position::Bottom,
            ),
        ]
        .spacing(space_xxxs())
        .align_y(cosmic::iced::Alignment::Center);

        let art_handle = self.mpris_album_art.get(selected_player);

        let info_row = if let Some(handle) = art_handle {
            row![
                cosmic::widget::image(handle.clone())
                    .width(Length::Fixed(50.0))
                    .height(Length::Fixed(50.0))
                    .content_fit(cosmic::iced::ContentFit::Cover),
                metadata_col
            ]
            .spacing(space_xs())
            .align_y(cosmic::iced::Alignment::Center)
        } else {
            row![
                container(icon::from_name("audio-x-generic-symbolic").size(ICON_L))
                    .width(Length::Fixed(50.0))
                    .align_x(cosmic::iced::Alignment::Center),
                metadata_col
            ]
            .spacing(space_xs())
            .align_y(cosmic::iced::Alignment::Center)
        };

        let mut content = column![player_name, info_row, controls]
            .spacing(space_xxs())
            .padding(Padding::from([space_xxs(), space_xs()]));

        // Show context menu if open
        if self.context_menu_mpris {
            let menu_item = |icon_name: &'static str,
                             label: &'static str,
                             message: Message|
             -> Element<'_, Message> {
                button::custom(
                    row![
                        icon::from_name(icon_name).size(ICON_S),
                        cosmic::widget::text::body(label),
                    ]
                    .spacing(space_xxs())
                    .align_y(cosmic::iced::Alignment::Center),
                )
                .width(Length::Fill)
                .padding([space_xxxs(), space_xxs()])
                .class(cosmic::theme::Button::MenuItem)
                .on_press(message)
                .into()
            };

            let menu_items = vec![
                menu_item(
                    "dialog-information-symbolic",
                    "Show track info",
                    Message::ShowMprisTrackInfo,
                ),
                menu_item(
                    "window-maximize-symbolic",
                    "Open player",
                    Message::RaiseMprisPlayer,
                ),
            ];

            let context_menu = container(column(menu_items).spacing(space_xxxs()))
                .padding(space_xxxs())
                .class(cosmic::theme::Container::Secondary);

            content = content.push(context_menu);
        }

        container(content)
            .width(Length::Fill)
            .class(cosmic::theme::Container::Card)
            .into()
    }

    /// Camera streaming controls view
    pub fn camera_controls_view(&self) -> Element<'_, Message> {
        // Check if v4l2loopback is available
        if !self.v4l2loopback_available {
            return Element::from(cosmic::iced::widget::Space::new(0, 0));
        }

        // Find devices with camera capability
        let camera_devices: Vec<_> = self
            .devices
            .iter()
            .filter(|d| {
                d.device
                    .info
                    .incoming_capabilities
                    .iter()
                    .any(|cap| cap == "cconnect.camera")
            })
            .collect();

        if camera_devices.is_empty() {
            return Element::from(cosmic::iced::widget::Space::new(0, 0));
        }

        // DESIGN DECISION: Single-device camera UI
        // Currently displays only the first camera-capable device. This is an intentional
        // MVP scope limitation, not a bug.
        //
        // Rationale:
        // - Most users connect 1-2 devices, rarely >1 with camera capability
        // - Single-device UI is simpler and avoids cluttering the applet panel
        // - Camera streaming is resource-intensive (decode + v4l2loopback kernel module)
        // - Streaming from multiple phones simultaneously would strain system resources
        //
        // FUTURE ENHANCEMENT: Multi-device camera support
        // If multiple camera devices are common, consider these UI approaches:
        //
        // Option A: Dropdown selector (Recommended)
        //   - Add a dropdown above camera controls: "Source: [Device Name ▼]"
        //   - Switch active camera device on selection
        //   - Only one camera streams at a time
        //
        // Option B: Tabbed interface
        //   - Tab per camera device
        //   - Allows independent controls but increases complexity
        //
        // Option C: Multiple panels (Not recommended)
        //   - Stack camera panels vertically
        //   - Takes too much screen space in applet
        //
        // Implementation notes:
        // - Store selected_camera_device_id in AppState
        // - Add Message::SelectCameraDevice(String) for dropdown
        // - Ensure only one camera streams at a time (stop others on start)
        // - Consider adding device labels (e.g., "Phone", "Tablet") for clarity
        let device_state = camera_devices[0];
        let device_id = &device_state.device.info.device_id;
        let device_name = &device_state.device.info.device_name;

        // Get camera stats if available
        let stats = self.camera_stats.get(device_id);
        let is_streaming = stats.is_some_and(|s| s.is_streaming);

        // Camera header with toggle
        let camera_header = row![
            icon::from_name("camera-web-symbolic").size(ICON_S),
            cosmic::widget::text::body(format!("Camera: {}", device_name)),
            horizontal_space(),
            cosmic::widget::toggler(is_streaming)
                .on_toggle(move |_| Message::ToggleCameraStreaming(device_id.clone()))
        ]
        .spacing(space_xxs())
        .align_y(cosmic::iced::Alignment::Center);

        let mut content_col = column![camera_header].spacing(space_xxs());

        // Show controls only when streaming
        if is_streaming {
            if let Some(stats) = stats {
                // Camera selection dropdown (mock for now)
                let camera_label = row![
                    cosmic::widget::text::caption("Camera:"),
                    horizontal_space(),
                    cosmic::widget::text::caption(if stats.camera_id == 0 {
                        "Back"
                    } else {
                        "Front"
                    }),
                ]
                .spacing(space_xxs());

                // Resolution display
                let resolution_label = row![
                    cosmic::widget::text::caption("Resolution:"),
                    horizontal_space(),
                    cosmic::widget::text::caption(&stats.resolution),
                ]
                .spacing(space_xxs());

                // Statistics
                let stats_row = row![
                    column![
                        cosmic::widget::text::caption("FPS:"),
                        cosmic::widget::text::body(format!("{}", stats.fps)),
                    ]
                    .spacing(space_xxxs()),
                    horizontal_space(),
                    column![
                        cosmic::widget::text::caption("Bitrate:"),
                        cosmic::widget::text::body(format!("{} kbps", stats.bitrate)),
                    ]
                    .spacing(space_xxxs()),
                ]
                .spacing(space_xs());

                content_col = content_col.push(divider::horizontal::default());
                content_col = content_col.push(camera_label);
                content_col = content_col.push(resolution_label);
                content_col = content_col.push(stats_row);
            }
        } else {
            // Show helper text when not streaming
            let helper_text = cosmic::widget::text::caption(
                "Toggle to use phone camera as webcam (/dev/video10)",
            );
            content_col = content_col.push(helper_text);
        }

        container(content_col.padding(Padding::from([space_xxs(), space_xs()])))
            .width(Length::Fill)
            .class(cosmic::theme::Container::Card)
            .into()
    }
}
