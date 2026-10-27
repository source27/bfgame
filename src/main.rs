use bfgame::abi::{CANVAS_H, CANVAS_W};
use bfgame::game::{InputState, Runtime};
use bfgame::options::{self, Parsed};
use bfgame::render;
use macroquad::prelude::*;
use std::path::Path;

fn window_config(title: String) -> macroquad::miniquad::conf::Conf {
    macroquad::miniquad::conf::Conf {
        window_title: title,
        window_width: CANVAS_W as i32,
        window_height: CANVAS_H as i32,
        high_dpi: false,
        window_resizable: true,
        sample_count: 4,
        ..Default::default()
    }
}

fn main() {
    let parsed = match options::parse(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("bfgame: {err}\n\n{}", options::HELP);
            std::process::exit(2);
        }
    };
    let opts = match parsed {
        Parsed::Help => {
            print!("{}", options::HELP);
            return;
        }
        Parsed::Run(opts) => opts,
    };

    let source = if opts.game.is_file() {
        match std::fs::read_to_string(&opts.game) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("bfgame: cannot read {}: {err}", opts.game.display());
                std::process::exit(2);
            }
        }
    } else if let Some(source) = bfgame::builtin::source_for(&opts.game) {
        source.to_string()
    } else {
        eprintln!("bfgame: game file not found: {}", opts.game.display());
        std::process::exit(2);
    };
    let program = match bfgame::vm::parse(&source) {
        Ok(program) => program,
        Err(err) => {
            eprintln!("bfgame: {}: {err}", opts.game.display());
            std::process::exit(2);
        }
    };
    if program.ops.is_empty() {
        eprintln!(
            "bfgame: {} contains no Brainfuck instructions",
            opts.game.display()
        );
        std::process::exit(2);
    }

    let game_name = opts
        .game
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("game");
    let title = format!("BFGame — {game_name}");
    let mut runtime = Runtime::new(program, opts.tick_ms, opts.budget, opts.seed);
    let frames = opts.frames;
    let screenshot = opts.screenshot.clone();

    macroquad::Window::from_config(window_config(title), async move {
        let mut last_time = get_time();
        let mut displayed: u64 = 0;
        let mut error: Option<String> = None;

        loop {
            if is_key_pressed(KeyCode::Escape) {
                break;
            }
            let now = get_time();
            let dt_ms = (((now - last_time) * 1000.0) as u64).min(250);
            last_time = now;

            if error.is_none() {
                let input = InputState {
                    up: is_key_down(KeyCode::Up),
                    down: is_key_down(KeyCode::Down),
                    left: is_key_down(KeyCode::Left),
                    right: is_key_down(KeyCode::Right),
                    space: is_key_down(KeyCode::Space),
                    a: is_key_down(KeyCode::A),
                    d: is_key_down(KeyCode::D),
                    r: is_key_down(KeyCode::R),
                    p: is_key_down(KeyCode::P),
                    up_pressed: is_key_pressed(KeyCode::Up),
                    down_pressed: is_key_pressed(KeyCode::Down),
                    left_pressed: is_key_pressed(KeyCode::Left),
                    right_pressed: is_key_pressed(KeyCode::Right),
                    space_pressed: is_key_pressed(KeyCode::Space),
                    a_pressed: is_key_pressed(KeyCode::A),
                    d_pressed: is_key_pressed(KeyCode::D),
                    r_pressed: is_key_pressed(KeyCode::R),
                    p_pressed: is_key_pressed(KeyCode::P),
                };
                if let Err(err) = runtime.frame(&input, dt_ms) {
                    error = Some(err);
                }
            }

            render::draw(runtime.machine.draw_commands());
            if let Some(err) = &error {
                render::draw_error(err);
            }

            if let Some(limit) = frames {
                if displayed + 1 >= limit {
                    if let Some(path) = &screenshot {
                        save_screenshot(path);
                    }
                    next_frame().await;
                    break;
                }
            }

            next_frame().await;
            displayed += 1;
        }
    });
}

fn save_screenshot(path: &Path) {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                eprintln!("bfgame: cannot create {}: {err}", parent.display());
                return;
            }
        }
    }
    let image = get_screen_data();
    image.export_png(&path.to_string_lossy());
    println!("screenshot: {}", path.display());
}
