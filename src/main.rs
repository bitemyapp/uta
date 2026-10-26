use aizome_uta::{
    app::Uta,
    instance::{self, Claim},
    service,
    store::Store,
};
use std::sync::Arc;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Uta — native YouTube Music for Aizome\n\nUsage: aizome-uta [SEARCH WORDS]\n       aizome-uta --demo\n       aizome-uta --version\n\nConnect your browser from the Account screen; no API credentials are required."
        );
        return;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Uta {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let demo = args == ["--demo"];
    let services: Arc<dyn service::Service> = if demo {
        Arc::new(service::Demo)
    } else {
        service::live()
    };
    let mut app = Uta::new(services, if demo { None } else { Store::user() }, demo);
    if !demo {
        match instance::claim(&args) {
            Ok(Claim::Handed) => return,
            Ok(Claim::First(instance)) => app.instance = Some(instance),
            Err(error) => {
                eprintln!("Uta: {error}");
                std::process::exit(1);
            }
        }
        app.query = args.join(" ");
    }
    aizome_kit::run(app);
}
