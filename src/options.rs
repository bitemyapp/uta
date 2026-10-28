#[derive(Debug, Default, PartialEq)]
pub struct Options {
    pub ignore_credentials: bool,
    pub python: bool,
    pub demo: bool,
    pub benchmark: Option<String>,
    pub compare_auth: bool,
    pub rounds: usize,
    pub query: Vec<String>,
}
impl Options {
    pub fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut result = Self {
            rounds: 3,
            ..Self::default()
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--ignore-credentials" => result.ignore_credentials = true,
                "--demo" => result.demo = true,
                "--backend" => {
                    result.python = match args.next().as_deref() {
                        Some("python") => true,
                        Some("native") => false,
                        _ => return Err("--backend requires native or python".into()),
                    }
                }
                "--benchmark" => {
                    let id = args.next().ok_or("--benchmark requires a video ID")?;
                    if !crate::model::valid_video_id(&id) {
                        return Err("Invalid benchmark video ID".into());
                    }
                    result.benchmark = Some(id);
                }
                "--rounds" => {
                    result.rounds = args
                        .next()
                        .and_then(|v| v.parse().ok())
                        .filter(|v| (1..=20).contains(v))
                        .ok_or("--rounds requires 1–20")?
                }
                "--compare-auth" => result.compare_auth = true,
                "--" => {
                    result.query.extend(args);
                    break;
                }
                _ if arg.starts_with('-') => return Err(format!("Unknown option: {arg}")),
                _ => result.query.push(arg),
            }
        }
        if result.compare_auth && (result.ignore_credentials || result.benchmark.is_none()) {
            return Err(
                "--compare-auth requires --benchmark and access to saved credentials".into(),
            );
        }
        if result.benchmark.is_some() && (result.demo || !result.query.is_empty() || result.python)
        {
            return Err("The benchmark selects its own backends; use a video ID and optional --ignore-credentials".into());
        }
        Ok(result)
    }
    pub fn profile(&self) -> &'static str {
        match (self.python, self.ignore_credentials) {
            (false, false) => "default",
            (false, true) => "anonymous",
            (true, false) => "python",
            (true, true) => "python-anonymous",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().map(|s| s.to_string()).collect())
    }
    #[test]
    fn modes_isolate_instances_and_keep_flags_out_of_queries() {
        let args = parse(&[
            "--ignore-credentials",
            "--backend",
            "python",
            "Bill",
            "Evans",
        ])
        .unwrap();
        assert_eq!(args.profile(), "python-anonymous");
        assert_eq!(args.query, ["Bill", "Evans"]);
        assert_eq!(parse(&[]).unwrap().profile(), "default");
        assert_ne!(
            parse(&["--ignore-credentials"]).unwrap().profile(),
            "default"
        );
    }
    #[test]
    fn invalid_benchmarks_fail_before_any_account_access() {
        for args in [
            vec!["--compare-auth"],
            vec!["--rounds", "0"],
            vec![
                "--benchmark",
                "abcdefghijk",
                "--compare-auth",
                "--ignore-credentials",
            ],
            vec!["--benchmark", "bad"],
            vec!["--backend", "typo"],
        ] {
            assert!(parse(&args).is_err());
        }
    }
}
