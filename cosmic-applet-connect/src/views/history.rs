use cosmic::{
    iced::{widget::scrollable, Length},
    iced::widget::{column, container, row},
    Element,
};

use crate::{horizontal_space, space_s, space_xxxs, space_xxs, CConnectApplet, Message};

impl CConnectApplet {
    pub fn history_view(&self) -> Element<'_, Message> {
        let mut history_list = column![].spacing(space_xxxs());

        if self.history.is_empty() {
            history_list = history_list.push(
                container(cosmic::widget::text::body("No history events"))
                    .width(Length::Fill)
                    .align_x(cosmic::iced::Alignment::Center)
                    .padding(space_s()),
            );
        } else {
            // In reverse order (newest first)
            for event in self.history.iter().rev() {
                let row = row![
                    column![
                        cosmic::widget::text::body(&event.event_type),
                        cosmic::widget::text::caption(&event.device_name),
                    ],
                    horizontal_space(),
                    cosmic::widget::text::caption(&event.details).width(Length::Fixed(150.0)),
                ]
                .width(Length::Fill)
                .align_y(cosmic::iced::Alignment::Center);

                history_list = history_list.push(
                    container(row)
                        .padding(space_xxs())
                        .class(cosmic::theme::Container::Card),
                );
            }
        }

        scrollable(history_list).into()
    }
}
