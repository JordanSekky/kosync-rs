use clap::Parser;
use kosync_proxy::{app, config::Config, logs::LogBuffer};
use tracing_subscriber::{
    EnvFilter, filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt,
};

#[tokio::main]
async fn main() {
    let config = Config::parse();

    let log_buffer = LogBuffer::new(config.log_buffer_lines);
    tracing_subscriber::registry()
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(log_buffer.clone())
        .init();

    if config.print_openapi {
        println!("{}", app::openapi_json(&config));
        return;
    }

    app::run(config, log_buffer).await;
}
