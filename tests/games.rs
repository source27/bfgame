use bfgame::abi::DrawCmd;
use bfgame::game::{InputState, Runtime};
use bfgame::vm::parse;

fn has_text(rt: &Runtime, needle: &str) -> bool {
    rt.machine.draw_commands().iter().any(|cmd| match cmd {
        DrawCmd::Text { text, .. } => text.contains(needle),
        _ => false,
    })
}

#[test]
fn snake_runs_and_draws() {
    let program = parse(include_str!("../games/snake.bf")).unwrap();
    let mut rt = Runtime::new(program, 120, 40_000_000, 0xBADC0DE);
    for _ in 0..8 {
        rt.frame(&InputState::default(), 16).unwrap();
    }
    assert!(rt.machine.draw_commands().len() > 5);
    assert!(has_text(&rt, "SNAKE"));
    assert!(rt
        .machine
        .draw_commands()
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Grid { .. })));
    assert!(rt
        .machine
        .draw_commands()
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Cell { .. })));
}

#[test]
fn snake_dies_at_wall_and_restarts() {
    let program = parse(include_str!("../games/snake.bf")).unwrap();
    let mut rt = Runtime::new(program, 120, 40_000_000, 7);
    for _ in 0..20 {
        rt.frame(&InputState::default(), 120).unwrap();
    }
    assert!(has_text(&rt, "GAME OVER"));

    let restart = InputState {
        r: true,
        r_pressed: true,
        ..Default::default()
    };
    rt.frame(&restart, 16).unwrap();
    for _ in 0..4 {
        rt.frame(&InputState::default(), 16).unwrap();
    }
    assert!(!has_text(&rt, "GAME OVER"));
    assert!(has_text(&rt, "SNAKE"));
}

#[test]
fn snake_turns_with_input() {
    let program = parse(include_str!("../games/snake.bf")).unwrap();
    let mut rt = Runtime::new(program, 120, 40_000_000, 1);
    for _ in 0..3 {
        rt.frame(&InputState::default(), 16).unwrap();
    }
    assert_eq!((rt.machine.cell(260), rt.machine.cell(261)), (8, 8));
    let up = InputState {
        up: true,
        up_pressed: true,
        ..Default::default()
    };
    rt.frame(&up, 16).unwrap();
    rt.frame(&InputState::default(), 120).unwrap();
    assert_eq!((rt.machine.cell(260), rt.machine.cell(261)), (8, 7));
}

#[test]
fn snake_eats_and_scores() {
    let source = include_str!("../games/snake.bf");
    let mut found = None;
    for seed in 0..400u32 {
        let mut rt = Runtime::new(parse(source).unwrap(), 120, 40_000_000, seed);
        for _ in 0..20 {
            rt.frame(&InputState::default(), 120).unwrap();
        }
        let score = rt.machine.draw_commands().iter().find_map(|cmd| match cmd {
            DrawCmd::Number { value, .. } if *value > 0 => Some(*value),
            _ => None,
        });
        if let Some(score) = score {
            found = Some((seed, score));
            break;
        }
    }
    assert!(
        found.is_some(),
        "some seed should put food on the first path"
    );
}

#[test]
fn tetris_runs_and_draws() {
    let mut rt = Runtime::new(
        parse(include_str!("../games/tetris.bf")).unwrap(),
        120,
        40_000_000,
        3,
    );
    for _ in 0..6 {
        rt.frame(&InputState::default(), 16).unwrap();
    }
    assert!(has_text(&rt, "TETRIS"));
    assert!(rt
        .machine
        .draw_commands()
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Grid { .. })));
}

#[test]
fn tetris_moves_and_rotates() {
    let program = parse(include_str!("../games/tetris.bf")).unwrap();
    let mut rt = Runtime::new(program, 120, 40_000_000, 11);
    for _ in 0..3 {
        rt.frame(&InputState::default(), 16).unwrap();
    }
    assert_eq!(rt.machine.cell(257), 3);
    let left = InputState {
        left: true,
        left_pressed: true,
        ..Default::default()
    };
    rt.frame(&left, 16).unwrap();
    assert_eq!(rt.machine.cell(257), 2);

    let before: Vec<u8> = (263..271).map(|i| rt.machine.cell(i)).collect();
    if rt.machine.cell(256) != 1 {
        let rotate = InputState {
            a: true,
            a_pressed: true,
            ..Default::default()
        };
        rt.frame(&rotate, 16).unwrap();
        let after: Vec<u8> = (263..271).map(|i| rt.machine.cell(i)).collect();
        assert_ne!(before, after);
    }
}

#[test]
fn tetris_clears_a_line() {
    let program = parse(include_str!("../games/tetris.bf")).unwrap();
    let mut rt = Runtime::new(program, 120, 40_000_000, 5);
    for _ in 0..3 {
        rt.frame(&InputState::default(), 16).unwrap();
    }
    for x in 0..10 {
        rt.machine.set_heap_value(x, 19, 7);
    }
    let drop = InputState {
        space: true,
        space_pressed: true,
        ..Default::default()
    };
    rt.frame(&drop, 16).unwrap();
    assert_eq!(rt.machine.cell(262), 1);
    assert_eq!(rt.machine.cell(261), 10);
}

#[test]
fn tetris_clears_two_lines() {
    let program = parse(include_str!("../games/tetris.bf")).unwrap();
    let mut rt = Runtime::new(program, 120, 40_000_000, 5);
    for _ in 0..3 {
        rt.frame(&InputState::default(), 16).unwrap();
    }
    for x in 0..10 {
        rt.machine.set_heap_value(x, 18, 5);
        rt.machine.set_heap_value(x, 19, 7);
    }
    let drop = InputState {
        space: true,
        space_pressed: true,
        ..Default::default()
    };
    rt.frame(&drop, 16).unwrap();
    assert_eq!(rt.machine.cell(262), 2);
    assert_eq!(rt.machine.cell(261), 20);
}

#[test]
fn tetris_stacks_and_restarts() {
    let program = parse(include_str!("../games/tetris.bf")).unwrap();
    let mut rt = Runtime::new(program, 120, 40_000_000, 3);
    let drop = InputState {
        space: true,
        space_pressed: true,
        ..Default::default()
    };
    for _ in 0..40 {
        rt.frame(&drop, 16).unwrap();
    }
    assert!(has_text(&rt, "GAME OVER"));

    let restart = InputState {
        r: true,
        r_pressed: true,
        ..Default::default()
    };
    rt.frame(&restart, 16).unwrap();
    for _ in 0..4 {
        rt.frame(&InputState::default(), 16).unwrap();
    }
    assert!(!has_text(&rt, "GAME OVER"));
    assert!(has_text(&rt, "TETRIS"));
}
