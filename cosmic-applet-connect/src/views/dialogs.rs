use cosmic::{
    iced::{widget::column, Alignment, Length},
    widget::{button, container, divider, icon, text, text_input},
    Element,
};

use crate::{space_xxs, space_xs, CConnectApplet, Message, ICON_S};

impl CConnectApplet {
    pub fn open_url_dialog_view(&self, device_id: &str) -> Element<'_, Message> {
        use cosmic::iced::widget::row;

        // Get device info
        let device = self.devices.iter().find(|d| d.device.id() == device_id);
        let device_name = device.map(|d| d.device.name()).unwrap_or("Unknown Device");

        let content = column![
            row![
                text::title3("Open on Phone").width(Length::Fill),
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("window-close-symbolic").size(ICON_S))
                        .on_press(Message::CancelOpenUrlDialog)
                        .padding(space_xxs()),
                    "Close",
                    cosmic::widget::tooltip::Position::Bottom,
                )
            ]
            .align_y(Alignment::Center),
            divider::horizontal::default(),
            text::caption(format!("Send a URL to open on {}", device_name)),
            text_input(
                "Enter URL (http://, https://, tel:, mailto:, etc.)",
                &self.open_url_input
            )
            .on_input(Message::OpenUrlInput)
            .on_submit({
                let url = self.open_url_input.clone();
                let id = device_id.to_string();
                move |_| Message::OpenOnPhone(id.clone(), url.clone())
            }),
            row![
                button::text("Cancel")
                    .on_press(Message::CancelOpenUrlDialog)
                    .width(Length::Fill),
                if !self.open_url_input.is_empty() {
                    button::text("Open on Phone")
                        .on_press({
                            let url = self.open_url_input.clone();
                            Message::OpenOnPhone(device_id.to_string(), url)
                        })
                        .class(cosmic::theme::Button::Suggested)
                        .width(Length::Fill)
                } else {
                    button::text("Open on Phone")
                        .class(cosmic::theme::Button::Suggested)
                        .width(Length::Fill)
                },
            ]
            .spacing(space_xxs()),
        ]
        .spacing(space_xs());

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(space_xs())
            .into()
    }

    /// SMS dialog view for sending SMS messages via device
    pub fn sms_dialog_view(&self, device_id: &str) -> Element<'_, Message> {
        use cosmic::iced::widget::row;

        // Get device info
        let device = self.devices.iter().find(|d| d.device.id() == device_id);
        let device_name = device.map(|d| d.device.name()).unwrap_or("Unknown Device");

        let content = column![
            row![
                text::title3("Send SMS").width(Length::Fill),
                cosmic::widget::tooltip(
                    button::icon(icon::from_name("window-close-symbolic").size(ICON_S))
                        .on_press(Message::CancelSmsDialog)
                        .padding(space_xxs()),
                    "Close",
                    cosmic::widget::tooltip::Position::Bottom,
                )
            ]
            .align_y(Alignment::Center),
            divider::horizontal::default(),
            text::caption(format!("Send SMS via {}", device_name)),
            text_input("Phone number", &self.sms_phone_number_input)
                .on_input(Message::UpdateSmsPhoneNumberInput),
            text_input("Message", &self.sms_message_input)
                .on_input(Message::UpdateSmsMessageInput)
                .on_submit({
                    let phone = self.sms_phone_number_input.clone();
                    let msg = self.sms_message_input.clone();
                    let id = device_id.to_string();
                    move |_| Message::SendSms(id.clone(), phone.clone(), msg.clone())
                }),
            row![
                button::text("Cancel")
                    .on_press(Message::CancelSmsDialog)
                    .width(Length::Fill),
                if !self.sms_phone_number_input.is_empty() && !self.sms_message_input.is_empty() {
                    button::text("Send SMS")
                        .on_press({
                            let phone = self.sms_phone_number_input.clone();
                            let msg = self.sms_message_input.clone();
                            Message::SendSms(device_id.to_string(), phone, msg)
                        })
                        .class(cosmic::theme::Button::Suggested)
                        .width(Length::Fill)
                } else {
                    button::text("Send SMS")
                        .class(cosmic::theme::Button::Suggested)
                        .width(Length::Fill)
                },
            ]
            .spacing(space_xxs()),
        ]
        .spacing(space_xs());

        container(content)
            .class(cosmic::theme::Container::Card)
            .padding(space_xs())
            .into()
    }
}
