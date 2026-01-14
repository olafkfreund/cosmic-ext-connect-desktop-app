use cosmic::app::{Core, Settings, Task};
use cosmic::{Application, Element};

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt::init();
    cosmic::app::run::<KdeConnectApp>(Settings::default(), ())
}

struct KdeConnectApp {
    core: Core,
}

#[derive(Debug, Clone)]
enum Message {}

impl Application for KdeConnectApp {
    type Message = Message;
    type Executor = cosmic::executor::Default;
    type Flags = ();
    const APP_ID: &'static str = "com.system76.CosmicKdeConnect";

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        (Self { core }, Task::none())
    }

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn update(&mut self, _message: Self::Message) -> Task<Self::Message> {
        Task::none()
    }

    fn view(&self) -> Element<Self::Message> {
        cosmic::widget::text("KDE Connect").into()
    }
}
