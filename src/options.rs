use std::path::PathBuf;

pub const HELP: &str = "\
BFGame — a tiny game engine whose game logic is Brainfuck

USAGE:
    bfgame <game.bf> [OPTIONS]

OPTIONS:
    --tick-ms N          Logic tick length in milliseconds (default: 120)
    --budget N           Max Brainfuck instructions per displayed frame
                         (default: 40,000,000)
    --seed N             Deterministic RNG seed
    --frames N           Exit after N displayed frames
    --screenshot PATH    Save the last displayed frame as a PNG
    --help, -h           Show this help

EXAMPLES:
    bfgame snake.bf
    bfgame tetris.bf --tick-ms 100
    bfgame snake.bf --frames 3 --screenshot docs/snake.png
";

#[derive(Debug, Clone)]
pub struct Options {
    pub game: PathBuf,
    pub tick_ms: u64,
    pub budget: u64,
    pub seed: u32,
    pub frames: Option<u64>,
    pub screenshot: Option<PathBuf>,
}

pub enum Parsed {
    Run(Options),
    Help,
}

pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Parsed, String> {
    let mut game: Option<PathBuf> = None;
    let mut tick_ms = crate::game::DEFAULT_TICK_MS;
    let mut budget = crate::game::DEFAULT_BUDGET;
    let mut seed = 0xBADC0DEu32;
    let mut frames: Option<u64> = None;
    let mut screenshot: Option<PathBuf> = None;
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(Parsed::Help),
            "--tick-ms" => {
                let value = iter.next().ok_or("--tick-ms requires a value")?;
                tick_ms = value
                    .parse()
                    .map_err(|_| format!("invalid --tick-ms value: {value}"))?;
                if tick_ms == 0 {
                    return Err("--tick-ms must be greater than zero".into());
                }
            }
            "--budget" => {
                let value = iter.next().ok_or("--budget requires a value")?;
                budget = value
                    .parse()
                    .map_err(|_| format!("invalid --budget value: {value}"))?;
                if budget < 10_000 {
                    return Err("--budget must be at least 10000".into());
                }
            }
            "--seed" => {
                let value = iter.next().ok_or("--seed requires a value")?;
                seed = value
                    .parse()
                    .map_err(|_| format!("invalid --seed value: {value}"))?;
            }
            "--frames" => {
                let value = iter.next().ok_or("--frames requires a value")?;
                let n: u64 = value
                    .parse()
                    .map_err(|_| format!("invalid --frames value: {value}"))?;
                if n == 0 {
                    return Err("--frames must be greater than zero".into());
                }
                frames = Some(n);
            }
            "--screenshot" => {
                let value = iter.next().ok_or("--screenshot requires a value")?;
                screenshot = Some(PathBuf::from(value));
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ => {
                if game.is_some() {
                    return Err(format!("unexpected extra argument: {arg}"));
                }
                game = Some(PathBuf::from(arg));
            }
        }
    }

    let game = game.ok_or("missing <game.bf> (try --help)")?;
    let game = crate::builtin::resolve(game);
    if !game.is_file() && !crate::builtin::is_builtin_name(&game) {
        return Err(format!("game file not found: {}", game.display()));
    }
    if !game
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("bf"))
        .unwrap_or(false)
    {
        return Err(format!(
            "game file must have a .bf extension: {}",
            game.display()
        ));
    }
    let frames = match (frames, &screenshot) {
        (Some(n), _) => Some(n),
        (None, Some(_)) => Some(3),
        (None, None) => None,
    };
    Ok(Parsed::Run(Options {
        game,
        tick_ms,
        budget,
        seed,
        frames,
        screenshot,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn help() {
        assert!(matches!(parse(args(&["--help"])), Ok(Parsed::Help)));
    }

    #[test]
    fn missing_game() {
        assert!(parse(args(&[])).is_err());
    }
}
