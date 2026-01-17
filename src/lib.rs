use wit_bindgen::FutureReader;

use crate::exports::astrobox::psys_plugin::{
    event::{self, EventType},
    lifecycle,
};

pub mod logger;
pub mod state;
pub mod transfer;
pub mod ui;
pub mod utils;

wit_bindgen::generate!({
    path: "wit",
    world: "psys-world",
    generate_all,
});

struct MyPlugin;

impl event::Guest for MyPlugin {
    #[allow(async_fn_in_trait)]
    fn on_event(event_type: EventType, event_payload: _rt::String) -> FutureReader<String> {
        let (writer, reader) = wit_future::new::<String>(|| "".to_string());

        match event_type {
            EventType::PluginMessage => {}
            EventType::InterconnectMessage => {
                transfer::handle_interconnect_message(&event_payload);
            }
            EventType::DeviceAction => {}
            EventType::ProviderAction => {}
            EventType::DeeplinkAction => {}
            EventType::TransportPacket => {}
        };

        tracing::info!("event_payload: {}", event_payload);

        wit_bindgen::rt::async_support::block_on(async move {
            let _ = writer.write("".to_string()).await;
        });

        reader
    }

    fn on_ui_event(
        event_id: _rt::String,
        event: event::Event,
        _event_payload: _rt::String,
    ) -> wit_bindgen::rt::async_support::FutureReader<_rt::String> {
        let (writer, reader) = wit_future::new::<String>(|| "".to_string());

        wit_bindgen::rt::async_support::block_on(async move {
            ui::ui_event_processor(event, &event_id).await;
            let _ = writer.write("".to_string()).await;
        });

        reader
    }

    fn on_ui_render(element_id: _rt::String) -> wit_bindgen::rt::async_support::FutureReader<()> {
        let (writer, reader) = wit_future::new::<()>(|| ());

        ui::render_main_ui(&element_id);

        wit_bindgen::rt::async_support::block_on(async move {
            let _ = writer.write(()).await;
        });

        reader
    }

    fn on_card_render(_card_id: _rt::String) -> wit_bindgen::rt::async_support::FutureReader<()> {
        let (writer, reader) = wit_future::new::<()>(|| ());

        wit_bindgen::rt::async_support::block_on(async move {
            let _ = writer.write(()).await;
        });

        reader
    }
}

impl lifecycle::Guest for MyPlugin {
    #[allow(async_fn_in_trait)]
    fn on_load() -> () {
        logger::init();
        tracing::info!("Hello AstroBox V2 Plugin!");
    }
}

export!(MyPlugin);
