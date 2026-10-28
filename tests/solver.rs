#![cfg(feature = "rustypipe")]
use rustypipe::deobfuscate::{DeobfData, Deobfuscator};
#[test]
fn solver_accepts_only_the_functions_and_returns_transformed_challenges() {
    let data = DeobfData {
        sig_fn: "function deobf_sig(s){return s.split('').reverse().join('')}".into(),
        nsig_fn: "function deobf_nsig(n){return 'solved-'+n.slice(1)}".into(),
        ..Default::default()
    };
    let solver = Deobfuscator::new(&data).unwrap();
    assert_eq!(solver.deobfuscate_sig("abcdef").unwrap(), "fedcba");
    assert_eq!(solver.deobfuscate_nsig("abcdef").unwrap(), "solved-bcdef");
}
#[test]
#[ignore = "requires a public player JavaScript fixture at UTA_TEST_PLAYER_JS"]
fn current_player_ejs_solves_signature_and_throttling_challenges() {
    let path = std::env::var("UTA_TEST_PLAYER_JS").expect("public player fixture path");
    let source = std::fs::read_to_string(path).unwrap();
    let start = std::time::Instant::now();
    let data =
        DeobfData::extract_fns("public-player-fixture", &source).expect("extract current player");
    assert!(
        data.sig_fn.contains("utaSolver"),
        "must exercise EJS fallback"
    );
    assert!(!data.sts.is_empty());
    let preprocess_ms = start.elapsed().as_secs_f64() * 1000.0;
    let load = std::time::Instant::now();
    let solver = Deobfuscator::new(&data).expect("instantiate EJS player");
    let load_ms = load.elapsed().as_secs_f64() * 1000.0;
    let sig =
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz";
    let solved = solver.deobfuscate_sig(sig).expect("signature challenge");
    assert!(!solved.is_empty());
    assert_ne!(solved, sig);
    let n = "abcdefghijklmnopqrstuvwxyz";
    let solved = solver.deobfuscate_nsig(n).expect("throttling challenge");
    assert!(!solved.is_empty());
    assert_ne!(solved, n);
    let warm = std::time::Instant::now();
    let reused = Deobfuscator::new(&data).expect("reuse EJS player");
    assert_eq!(reused.deobfuscate_nsig(n).unwrap(), solved);
    println!(
        "Public player solver: preprocess={preprocess_ms:.1} ms, load={load_ms:.1} ms, warm load+solve={:.1} ms",
        warm.elapsed().as_secs_f64() * 1000.0
    );
}
