use aizome_uta::{
    app::Uta,
    instance::{self, Claim},
    service,
    store::Store,
};
use std::sync::Arc;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        // Third-party diagnostics may contain signed URLs or response bodies.
        .filter_module("rustypipe", log::LevelFilter::Off)
        .filter_module("reqwest", log::LevelFilter::Off)
        .filter_module("hyper", log::LevelFilter::Off)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Uta — native YouTube Music for Aizome\n\nUsage: aizome-uta [SEARCH WORDS | YOUTUBE MUSIC LINK]\n       aizome-uta --demo\n       aizome-uta --ignore-credentials [--backend native|python]\n       aizome-uta --benchmark VIDEO_ID [--rounds 3] [--ignore-credentials | --compare-auth]\n       aizome-uta --version\n\nA song link plays it; a playlist or album link opens it.\nConnect your account from the Account screen: Uta finds the browser you're\nsigned in with. No API credentials are required."
        );
        return;
    }
    if args.iter().any(|a| a == "--version") {
        println!(
            "Uta {} (RustyPipe: {})",
            env!("CARGO_PKG_VERSION"),
            cfg!(feature = "rustypipe")
        );
        return;
    }
    let explicit_backend = args.iter().any(|arg| arg == "--backend");
    let mut options = aizome_uta::options::Options::parse(args).unwrap_or_else(|error| {
        eprintln!("Uta: {error}");
        std::process::exit(2);
    });
    if !explicit_backend
        && options.benchmark.is_none()
        && std::env::var("UTA_SERVICE").as_deref() == Ok("python")
    {
        options.python = true;
    }
    if options.benchmark.is_some() {
        #[cfg(feature = "rustypipe")]
        let result = aizome_uta::benchmark::run(&options);
        #[cfg(not(feature = "rustypipe"))]
        let result: Result<(), String> = Err("This build does not include RustyPipe".into());
        if let Err(error) = result {
            eprintln!("Uta: {error}");
            std::process::exit(1);
        }
        return;
    }
    let demo = options.demo;
    let services: Arc<dyn service::Service> = if demo {
        Arc::new(service::Demo)
    } else {
        service::configured(options.python, options.ignore_credentials)
    };
    let mut app = Uta::new(
        services,
        if demo {
            None
        } else {
            Store::user_profile(options.profile())
        },
        demo,
    );
    if !demo {
        match instance::claim_profile(&options.query, options.profile()) {
            Ok(Claim::Handed) => return,
            Ok(Claim::First(instance)) => app.instance = Some(instance),
            Err(error) => {
                eprintln!("Uta: {error}");
                std::process::exit(1);
            }
        }
        app.query = options.query.join(" ");
    }
    aizome_kit::run(app);
}
