use cosmic::{
    iced::{
        alignment::Horizontal,
        widget::{column, container, progress_bar, row, scrollable}, Length,
    },
    theme,
    widget::{button, divider, icon, text},
    Element,
};

use crate::{
    horizontal_space, space_s, space_xs, space_xxs, space_xxxs, state::*, theme_accent_color,
    CConnectApplet, Message, ViewMode, ICON_M, ICON_S, ICON_XL, ICON_XS,
    MAX_DISPLAYED_HISTORY_ITEMS,
};

impl CConnectApplet {
    pub fn transfer_queue_view(&self) -> Element<'_, Message> {
        let mut transfers_list = column![].spacing(space_xxs());

        if self.active_transfers.is_empty() {
            transfers_list = transfers_list.push(
                container(
                    column![
                        icon::from_name("folder-download-symbolic").size(ICON_XL),
                        text("No active transfers"),
                    ]
                    .spacing(space_xxs())
                    .align_x(Horizontal::Center),
                )
                .width(Length::Fill)
                .align_x(cosmic::iced::Alignment::Center)
                .padding(space_s()),
            );
        } else {
            for (id, state) in &self.active_transfers {
                let progress = if state.total > 0 {
                    (state.current as f32 / state.total as f32) * 100.0
                } else {
                    0.0
                };

                let transfer_id = id.clone();
                let filename = state.filename.clone();
                let is_receiving = state.direction != "sending";

                // Context menu button
                let menu_open = self.context_menu_transfer.as_ref() == Some(id);
                let menu_message = if menu_open {
                    Message::CloseTransferContextMenu
                } else {
                    Message::ShowTransferContextMenu(transfer_id.clone())
                };

                let menu_button = button::icon(icon::from_name("view-more-symbolic").size(ICON_S))
                    .padding(space_xxxs())
                    .class(cosmic::theme::Button::Transparent)
                    .on_press(menu_message);

                // File metadata
                let file_icon = Self::file_type_icon(&state.filename);
                let file_size = Self::format_file_size(state.total);
                let bytes_transferred = Self::format_file_size(state.current);

                // Build status text with size and time info
                let mut status_text = if state.direction == "sending" {
                    format!("Sending: {} / {}", bytes_transferred, file_size)
                } else {
                    format!("Receiving: {} / {}", bytes_transferred, file_size)
                };

                // Add time estimate if available
                if let Some(time_left) = Self::estimate_time_remaining(state) {
                    status_text.push_str(&format!(" · {}", time_left));
                }

                let mut transfer_row = row![
                    icon::from_name(file_icon).size(ICON_M),
                    column![
                        cosmic::widget::text::body(&state.filename),
                        progress_bar(0.0..=100.0, progress).height(Length::Fixed(6.0)),
                        row![
                            cosmic::widget::text::caption(status_text.clone()),
                            horizontal_space(),
                            cosmic::widget::text::caption(format!("{:.0}%", progress)),
                        ]
                    ]
                    .spacing(space_xxxs())
                    .width(Length::Fill),
                    menu_button,
                ]
                .spacing(space_xxs())
                .align_y(cosmic::iced::Alignment::Center);

                // Show context menu if this transfer's menu is open
                if self.context_menu_transfer.as_ref() == Some(id) {
                    let menu_items =
                        self.build_transfer_context_menu(&transfer_id, &filename, is_receiving);

                    let context_menu = container(column(menu_items).spacing(space_xxxs()))
                        .padding(space_xxxs())
                        .class(cosmic::theme::Container::Secondary);

                    transfer_row = transfer_row.push(context_menu);
                }

                transfers_list = transfers_list.push(
                    container(transfer_row)
                        .padding(space_xxs())
                        .class(cosmic::theme::Container::Card),
                );
            }
        }

        // Build received files history section
        let mut history_section = column![].spacing(space_xxs());

        if !self.received_files_history.is_empty() {
            history_section = history_section.push(
                row![
                    icon::from_name("folder-recent-symbolic").size(ICON_S),
                    cosmic::widget::text::heading("Recently Received"),
                ]
                .spacing(space_xxs())
                .align_y(cosmic::iced::Alignment::Center),
            );

            for received in self
                .received_files_history
                .iter()
                .take(MAX_DISPLAYED_HISTORY_ITEMS)
            {
                let status_icon = if received.success {
                    "emblem-ok-symbolic"
                } else {
                    "emblem-error-symbolic"
                };
                let time_str = Self::format_elapsed(received.timestamp.elapsed());

                let open_button: Element<'_, Message> = if received.success {
                    cosmic::widget::tooltip(
                        button::icon(icon::from_name("document-open-symbolic").size(ICON_S))
                            .padding(space_xxxs())
                            .class(cosmic::theme::Button::Transparent)
                            .on_press(Message::OpenTransferFile(received.filename.clone())),
                        "Open file",
                        cosmic::widget::tooltip::Position::Bottom,
                    )
                    .into()
                } else {
                    cosmic::iced::widget::Space::new(0, 0).into()
                };

                let file_row = row![
                    icon::from_name("document-save-symbolic").size(ICON_S),
                    column![
                        cosmic::widget::text::body(&received.filename),
                        row![
                            icon::from_name(status_icon).size(ICON_XS),
                            cosmic::widget::text::caption(format!(
                                "from {} • {}",
                                received.device_name, time_str
                            )),
                        ]
                        .spacing(space_xxxs())
                        .align_y(cosmic::iced::Alignment::Center),
                    ]
                    .spacing(space_xxxs())
                    .width(Length::Fill),
                    open_button,
                ]
                .spacing(space_xxs())
                .align_y(cosmic::iced::Alignment::Center);

                history_section = history_section.push(
                    container(file_row)
                        .padding(space_xxs())
                        .class(cosmic::theme::Container::Card),
                );
            }
        }

        // Combine active transfers and history
        let mut all_content = column![transfers_list].spacing(space_xs());

        if !self.received_files_history.is_empty() {
            all_content = all_content.push(history_section);
        }

        column![
            row![
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("go-previous-symbolic").size(ICON_S))
                        .on_press(Message::SetViewMode(ViewMode::Devices))
                        .padding(space_xxs()),
                    "Back",
                    cosmic::widget::tooltip::Position::Bottom,
                ),
                cosmic::widget::text::title4("Transfer Queue"),
                horizontal_space(),
            ]
            .spacing(space_xxs())
            .align_y(cosmic::iced::Alignment::Center),
            scrollable(all_content).height(Length::Fill)
        ]
        .spacing(space_xs())
        .padding(space_xs())
        .into()
    }

    pub fn transfers_view(&self) -> Element<'_, Message> {
        if self.active_transfers.is_empty() {
            return Element::from(cosmic::widget::Space::new(0, 0));
        }

        let header = row![
            cosmic::widget::text::body("Active Transfers")
                .class(theme::Text::Color(theme_accent_color()))
                .width(Length::Fill),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("go-next-symbolic").size(ICON_S))
                    .on_press(Message::ShowTransferQueue)
                    .padding(space_xxs()),
                "View Transfer Queue",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .align_y(cosmic::iced::Alignment::Center);

        let mut transfers_col = column![header].spacing(space_xxs());

        for state in self.active_transfers.values() {
            let progress = if state.total > 0 {
                (state.current as f32 / state.total as f32) * 100.0
            } else {
                0.0
            };

            let label = format!(
                "{} {} ({:.0}%)",
                if state.direction == "sending" {
                    "Sending"
                } else {
                    "Receiving"
                },
                state.filename,
                progress
            );

            transfers_col = transfers_col.push(
                column![
                    cosmic::widget::text::caption(label),
                    progress_bar(0.0..=100.0, progress).height(Length::Fixed(6.0))
                ]
                .spacing(space_xxxs()),
            );
        }

        container(transfers_col)
            .padding(space_xs())
            .width(Length::Fill)
            .into()
    }

    /// Formats elapsed time into human-readable format
    pub(crate) fn format_elapsed(elapsed: std::time::Duration) -> String {
        let secs = elapsed.as_secs();
        match secs {
            0..=59 => "Just now".to_string(),
            60..=3599 => format!("{}m ago", secs / 60),
            3600..=86399 => format!("{}h ago", secs / 3600),
            _ => format!("{}d ago", secs / 86400),
        }
    }

    /// Maps file extension to appropriate icon name
    pub(crate) fn file_type_icon(filename: &str) -> &'static str {
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            // Images
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "webp" | "ico" => {
                "image-x-generic-symbolic"
            }
            // Documents
            "pdf" | "doc" | "docx" | "odt" | "txt" | "rtf" => "x-office-document-symbolic",
            // Spreadsheets
            "xls" | "xlsx" | "ods" | "csv" => "x-office-spreadsheet-symbolic",
            // Presentations
            "ppt" | "pptx" | "odp" => "x-office-presentation-symbolic",
            // Audio
            "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "wma" => "audio-x-generic-symbolic",
            // Video
            "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" => "video-x-generic-symbolic",
            // Archives
            "zip" | "tar" | "gz" | "bz2" | "7z" | "rar" | "xz" => "package-x-generic-symbolic",
            // Code
            "rs" | "py" | "js" | "ts" | "java" | "c" | "cpp" | "h" | "hpp" => {
                "text-x-script-symbolic"
            }
            // Default
            _ => "text-x-generic-symbolic",
        }
    }

    /// Formats bytes into human-readable size (KB, MB, GB)
    pub(crate) fn format_file_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    /// Calculates estimated time remaining for a transfer
    pub(crate) fn estimate_time_remaining(state: &TransferState) -> Option<String> {
        if state.total == 0 || state.current >= state.total {
            return None;
        }

        let elapsed = state.last_update.duration_since(state.started_at);
        if elapsed.as_secs() < 1 {
            return Some("Calculating...".to_string());
        }

        let bytes_transferred = state.current;
        let bytes_remaining = state.total.saturating_sub(state.current);

        // Calculate speed based on total elapsed time
        let speed = bytes_transferred as f64 / elapsed.as_secs_f64();

        if speed < 1.0 {
            return Some("Calculating...".to_string());
        }

        let seconds_remaining = (bytes_remaining as f64 / speed) as u64;

        match seconds_remaining {
            0 => Some("Almost done".to_string()),
            1..=59 => Some(format!("{}s left", seconds_remaining)),
            60..=3599 => Some(format!(
                "{}m {}s left",
                seconds_remaining / 60,
                seconds_remaining % 60
            )),
            3600..=86399 => Some(format!(
                "{}h {}m left",
                seconds_remaining / 3600,
                (seconds_remaining % 3600) / 60
            )),
            _ => Some(format!("{}d left", seconds_remaining / 86400)),
        }
    }

    pub(crate) fn build_transfer_context_menu(
        &self,
        transfer_id: &str,
        filename: &str,
        is_receiving: bool,
    ) -> Vec<Element<'_, Message>> {
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

        let mut items = vec![menu_item(
            "process-stop-symbolic",
            "Cancel transfer",
            Message::CancelTransfer(transfer_id.to_string()),
        )];

        if is_receiving {
            items.push(divider::horizontal::default().into());
            items.push(menu_item(
                "document-open-symbolic",
                "Open file",
                Message::OpenTransferFile(filename.to_string()),
            ));
            items.push(menu_item(
                "folder-open-symbolic",
                "Reveal in folder",
                Message::RevealTransferFile(filename.to_string()),
            ));
        }

        items
    }
}
