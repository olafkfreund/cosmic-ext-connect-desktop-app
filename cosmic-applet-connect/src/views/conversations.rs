use cosmic::{
    iced::{
        widget::{column, row, scrollable},
        Alignment, Length,
    },
    widget::{button, container, divider, icon, text},
    Element,
};

use crate::{space_xxs, space_xs, space_xxxs, CConnectApplet, Message, ICON_S};

impl CConnectApplet {
    /// Conversations list view — shows all SMS threads for a device
    pub fn conversations_list_view(&self, device_id: &str) -> Element<'_, Message> {
        let device = self.devices.iter().find(|d| d.device.id() == device_id);
        let device_name = device.map(|d| d.device.name()).unwrap_or("Unknown Device");

        let header = row![
            text::title3("Conversations").width(Length::Fill),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_S))
                    .on_press(Message::CloseConversations)
                    .padding(space_xxs()),
                "Close",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .align_y(Alignment::Center);

        let subtitle = text::caption(format!("SMS conversations on {}", device_name));

        let conversations = self
            .conversations_cache
            .get(device_id)
            .cloned()
            .unwrap_or_default();

        let list: Element<'_, Message> = if conversations.is_empty() {
            container(
                column![
                    text::body("No conversations yet"),
                    text::caption("Conversations will appear when your phone syncs SMS data."),
                ]
                .spacing(space_xxxs())
                .align_x(Alignment::Center),
            )
            .center(Length::Fill)
            .padding(space_xs())
            .into()
        } else {
            let mut items = column![].spacing(space_xxxs());
            for conv in conversations {
                let unread_badge = if conv.unread_count > 0 {
                    format!(" ({})", conv.unread_count)
                } else {
                    String::new()
                };
                let preview: String = conv.preview.chars().take(40).collect();
                let time_str = format!("{}{}", format_timestamp(conv.timestamp), unread_badge);
                let thread_id = conv.thread_id;
                let dev_id = device_id.to_string();

                items = items.push(
                    button::custom(
                        column![
                            row![
                                text::body(conv.address).width(Length::Fill),
                                text::caption(time_str),
                            ]
                            .align_y(Alignment::Center),
                            text::caption(preview),
                        ]
                        .spacing(space_xxxs()),
                    )
                    .on_press(Message::SelectConversation(dev_id, thread_id))
                    .width(Length::Fill)
                    .padding(space_xxxs()),
                );
            }
            scrollable(items).height(Length::Fill).into()
        };

        let content = column![header, subtitle, divider::horizontal::default(), list]
            .spacing(space_xxs());

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(space_xs())
            .into()
    }

    /// Conversation detail view — shows messages in a single thread
    pub fn conversation_detail_view(
        &self,
        device_id: &str,
        thread_id: i64,
    ) -> Element<'_, Message> {
        // Find conversation summary for title
        let conv_address = self
            .conversations_cache
            .get(device_id)
            .and_then(|convs| convs.iter().find(|c| c.thread_id == thread_id))
            .map(|c| c.address.as_str())
            .unwrap_or("Unknown");

        let header = row![
            button::icon(icon::from_name("go-previous-symbolic").size(ICON_S))
                .on_press(Message::CloseConversation)
                .padding(space_xxxs()),
            text::title4(conv_address).width(Length::Fill),
            cosmic::widget::tooltip(
                button::icon(icon::from_name("window-close-symbolic").size(ICON_S))
                    .on_press(Message::CloseConversations)
                    .padding(space_xxs()),
                "Close",
                cosmic::widget::tooltip::Position::Bottom,
            )
        ]
        .align_y(Alignment::Center)
        .spacing(space_xxxs());

        // Reply row
        let mut send_button =
            button::icon(icon::from_name("mail-send-symbolic").size(ICON_S)).padding(space_xxxs());
        if !self.sms_message_input.is_empty() {
            send_button = send_button.on_press(Message::SendSms(
                device_id.to_string(),
                conv_address.to_string(),
                self.sms_message_input.clone(),
            ));
        }

        let reply_row = row![
            cosmic::widget::text_input("Type a message...", &self.sms_message_input)
                .on_input(Message::UpdateSmsMessageInput)
                .width(Length::Fill),
            send_button,
        ]
        .spacing(space_xxxs())
        .align_y(Alignment::Center);

        let placeholder = container(
            text::caption("Messages will appear here when loaded from your phone."),
        )
        .center(Length::Fill)
        .padding(space_xs());

        let content = column![
            header,
            divider::horizontal::default(),
            placeholder,
            divider::horizontal::default(),
            reply_row,
        ]
        .spacing(space_xxs());

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(space_xs())
            .into()
    }
}

/// Format a timestamp (ms since epoch) into a short display string
fn format_timestamp(timestamp_ms: i64) -> String {
    // Simple relative time display
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let diff_secs = (now_ms - timestamp_ms) / 1000;

    if diff_secs < 60 {
        "now".to_string()
    } else if diff_secs < 3600 {
        format!("{}m", diff_secs / 60)
    } else if diff_secs < 86400 {
        format!("{}h", diff_secs / 3600)
    } else {
        format!("{}d", diff_secs / 86400)
    }
}
