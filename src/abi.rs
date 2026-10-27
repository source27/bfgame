//! The ABI shared by Brainfuck programs and the host engine.
//!
//! The first 128 bytes of the Brainfuck tape are memory-mapped I/O.  Game
//! data can be placed from byte 128 onwards.  Arrays that need indexing are
//! kept in the engine's small addressable heap (32 arenas * 256 bytes) and
//! accessed with the `LOAD` / `STORE` commands.

use std::fmt;

// ---------------------------------------------------------------------------
// Input registers (written by the engine before every frame)
// ---------------------------------------------------------------------------

pub const UP: usize = 0;
pub const DOWN: usize = 1;
pub const LEFT: usize = 2;
pub const RIGHT: usize = 3;
pub const SPACE: usize = 4;
pub const KEY_A: usize = 5;
pub const KEY_D: usize = 6;
pub const KEY_R: usize = 7;
pub const KEY_P: usize = 8;

pub const UP_PRESSED: usize = 9;
pub const DOWN_PRESSED: usize = 10;
pub const LEFT_PRESSED: usize = 11;
pub const RIGHT_PRESSED: usize = 12;
pub const SPACE_PRESSED: usize = 13;
pub const A_PRESSED: usize = 14;
pub const D_PRESSED: usize = 15;
pub const R_PRESSED: usize = 16;
pub const P_PRESSED: usize = 17;

pub const TICK: usize = 18;
pub const FRAME: usize = 19;
pub const RANDOM: usize = 20;
pub const RAND_LO: usize = 21;
pub const RAND_HI: usize = 22;
pub const RAND_7: usize = 23;
/// 0 = none, 1 = up, 2 = right, 3 = down, 4 = left.
pub const LAST_DIR: usize = 24;

pub const INPUT_END: usize = 32;

// ---------------------------------------------------------------------------
// Output / command registers
// ---------------------------------------------------------------------------

pub const CMD: usize = 32;
pub const ARG0: usize = 33;
pub const ARG: [usize; 12] = [33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44];
/// Result register for `LOAD`.
pub const VALUE: usize = 45;
/// Write 1 here to execute the command in `CMD`.
pub const EXEC: usize = 46;
/// Reserved for future status flags.
pub const STATUS: usize = 47;

pub const TEXT_BUF: usize = 64;
pub const TEXT_CAP: usize = 64;
pub const RAM_BASE: usize = 128;
pub const TAPE_SIZE: usize = 65_536;

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

pub const CMD_CLEAR: u8 = 1;
pub const CMD_RECT: u8 = 2;
pub const CMD_RECT_LINES: u8 = 3;
pub const CMD_LINE: u8 = 4;
pub const CMD_CIRCLE: u8 = 5;
pub const CMD_TEXT: u8 = 6;
pub const CMD_NUMBER: u8 = 7;
pub const CMD_CELL: u8 = 8;
pub const CMD_GRID: u8 = 9;
pub const CMD_YIELD: u8 = 10;
pub const CMD_LOAD: u8 = 11;
pub const CMD_STORE: u8 = 12;
pub const CMD_FILL: u8 = 13;

/// Logical canvas used by all games.
pub const CANVAS_W: f32 = 960.0;
pub const CANVAS_H: f32 = 640.0;
/// The fixed tile grid used by `CELL`.
pub const CELL_ORIGIN_X: f32 = 80.0;
pub const CELL_ORIGIN_Y: f32 = 80.0;
pub const CELL_SIZE: f32 = 24.0;

pub const ARENA_COUNT: usize = 32;
pub const ARENA_SIZE: usize = 256;

// Palette 0..15.  Kept as bytes so it can also be used by tests/tools.
pub const PALETTE: [[u8; 3]; 16] = [
    [11, 14, 20],    // 0  background
    [21, 27, 38],    // 1  board
    [42, 51, 69],    // 2  grid
    [255, 77, 109],  // 3  food
    [79, 109, 143],  // 4  border
    [57, 217, 138],  // 5  snake body
    [155, 246, 193], // 6  snake head
    [232, 240, 255], // 7  text
    [143, 163, 191], // 8  muted text
    [255, 209, 102], // 9  yellow
    [255, 140, 66],  // 10 orange
    [76, 201, 240],  // 11 blue
    [179, 136, 255], // 12 purple
    [239, 71, 111],  // 13 red
    [255, 255, 255], // 14 white
    [0, 0, 0],       // 15 black
];

pub fn palette_color(index: u8) -> [u8; 3] {
    PALETTE[index as usize % PALETTE.len()]
}

// ---------------------------------------------------------------------------
// Retained display list
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd {
    Clear {
        color: u8,
    },
    Rect {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        color: u8,
    },
    RectLines {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        color: u8,
    },
    Line {
        x1: u16,
        y1: u16,
        x2: u16,
        y2: u16,
        color: u8,
    },
    Circle {
        x: u16,
        y: u16,
        r: u16,
        color: u8,
    },
    Text {
        x: u16,
        y: u16,
        color: u8,
        size: u16,
        text: String,
    },
    Number {
        x: u16,
        y: u16,
        value: u8,
        color: u8,
        size: u16,
    },
    Cell {
        x: u8,
        y: u8,
        color: u8,
        style: u8,
    },
    Grid {
        x: u16,
        y: u16,
        cols: u8,
        rows: u8,
        cell: u8,
        color: u8,
    },
}

impl DrawCmd {
    pub fn kind(&self) -> &'static str {
        match self {
            DrawCmd::Clear { .. } => "clear",
            DrawCmd::Rect { .. } => "rect",
            DrawCmd::RectLines { .. } => "rect_lines",
            DrawCmd::Line { .. } => "line",
            DrawCmd::Circle { .. } => "circle",
            DrawCmd::Text { .. } => "text",
            DrawCmd::Number { .. } => "number",
            DrawCmd::Cell { .. } => "cell",
            DrawCmd::Grid { .. } => "grid",
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    PointerOutOfBounds(i32),
    ProgramCounterOutOfBounds,
    Halted,
    FrameBudgetExceeded,
    ArenaOutOfBounds(u8),
    DrawListTooLarge,
    UnknownCommand(u8),
    ExecWithoutCommand,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::PointerOutOfBounds(p) => {
                write!(
                    f,
                    "Brainfuck data pointer left the 64 KiB tape (pointer={p})"
                )
            }
            EngineError::ProgramCounterOutOfBounds => {
                write!(f, "program counter left the Brainfuck program")
            }
            EngineError::Halted => write!(f, "program halted"),
            EngineError::FrameBudgetExceeded => write!(
                f,
                "program did not reach a YIELD command within the frame instruction budget"
            ),
            EngineError::ArenaOutOfBounds(a) => {
                write!(f, "heap arena {a} is out of bounds (0..31)")
            }
            EngineError::DrawListTooLarge => write!(f, "retained display list grew too large"),
            EngineError::UnknownCommand(c) => write!(f, "unknown engine command {c}"),
            EngineError::ExecWithoutCommand => {
                write!(f, "EXEC was triggered while no command was selected")
            }
        }
    }
}

impl std::error::Error for EngineError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Yielded,
    BudgetExceeded,
}
