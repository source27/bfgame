//! A small, deterministic Brainfuck interpreter with a memory-mapped game ABI.
//!
//! Only the eight Brainfuck commands matter; everything else in the source is
//! treated as a comment.  The interpreter uses a 64 KiB tape of wrapping u8
//! cells and pre-computes loop jumps so that game code stays fast.

use crate::abi::*;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Add(i32),
    Move(i32),
    Output,
    Input,
    JumpIfZero(usize),
    JumpIfNotZero(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub ops: Vec<Op>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    UnmatchedOpen(usize),
    UnmatchedClose(usize),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnmatchedOpen(at) => {
                write!(f, "unmatched '[' at Brainfuck instruction {at}")
            }
            ParseError::UnmatchedClose(at) => {
                write!(f, "unmatched ']' at Brainfuck instruction {at}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Compile Brainfuck source into a compact instruction stream.
pub fn parse(source: &str) -> Result<Program, ParseError> {
    let mut ops: Vec<Op> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut add: i32 = 0;
    let mut mov: i32 = 0;
    let mut chars: Vec<(usize, char)> = source
        .chars()
        .filter(|c| matches!(c, '+' | '-' | '<' | '>' | '[' | ']' | '.' | ','))
        .enumerate()
        .collect();

    // We need to report instruction positions, and to patch loop targets.
    // `chars` is indexed by Brainfuck-command number.
    let mut idx = 0usize;
    while idx < chars.len() {
        let c = chars[idx].1;
        match c {
            '+' => {
                if mov != 0 {
                    ops.push(Op::Move(mov));
                    mov = 0;
                }
                add += 1;
            }
            '-' => {
                if mov != 0 {
                    ops.push(Op::Move(mov));
                    mov = 0;
                }
                add -= 1;
            }
            '>' => {
                if add != 0 {
                    ops.push(Op::Add(add));
                    add = 0;
                }
                mov += 1;
            }
            '<' => {
                if add != 0 {
                    ops.push(Op::Add(add));
                    add = 0;
                }
                mov -= 1;
            }
            '[' => {
                if add != 0 {
                    ops.push(Op::Add(add));
                    add = 0;
                }
                if mov != 0 {
                    ops.push(Op::Move(mov));
                    mov = 0;
                }
                stack.push(ops.len());
                ops.push(Op::JumpIfZero(usize::MAX));
            }
            ']' => {
                if add != 0 {
                    ops.push(Op::Add(add));
                    add = 0;
                }
                if mov != 0 {
                    ops.push(Op::Move(mov));
                    mov = 0;
                }
                let open = stack.pop().ok_or(ParseError::UnmatchedClose(idx))?;
                ops.push(Op::JumpIfNotZero(open + 1));
                let after = ops.len();
                ops[open] = Op::JumpIfZero(after);
            }
            '.' => {
                if add != 0 {
                    ops.push(Op::Add(add));
                    add = 0;
                }
                if mov != 0 {
                    ops.push(Op::Move(mov));
                    mov = 0;
                }
                ops.push(Op::Output);
            }
            ',' => {
                if add != 0 {
                    ops.push(Op::Add(add));
                    add = 0;
                }
                if mov != 0 {
                    ops.push(Op::Move(mov));
                    mov = 0;
                }
                ops.push(Op::Input);
            }
            _ => unreachable!(),
        }
        idx += 1;
    }
    if add != 0 {
        ops.push(Op::Add(add));
    }
    if mov != 0 {
        ops.push(Op::Move(mov));
    }
    if let Some(open) = stack.last() {
        return Err(ParseError::UnmatchedOpen(*open));
    }
    // Keep clippy from complaining about the otherwise useful intermediate
    // vector variable, and make the command count available for diagnostics.
    chars.clear();
    Ok(Program { ops })
}

pub struct Machine {
    pub program: Program,
    pub cells: Box<[u8]>,
    pub pc: usize,
    pub ptr: usize,
    heap: Box<[u8]>,
    pub draw: Vec<DrawCmd>,
    pub yielded: bool,
    pub command_count: u64,
}

impl fmt::Debug for Machine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Machine")
            .field("pc", &self.pc)
            .field("ptr", &self.ptr)
            .field("ops", &self.program.ops.len())
            .field("draw", &self.draw.len())
            .field("yielded", &self.yielded)
            .finish()
    }
}

impl Machine {
    pub fn new(program: Program) -> Self {
        Self {
            program,
            cells: vec![0; TAPE_SIZE].into_boxed_slice(),
            pc: 0,
            ptr: 0,
            heap: vec![0; ARENA_COUNT * ARENA_SIZE].into_boxed_slice(),
            draw: Vec::new(),
            yielded: false,
            command_count: 0,
        }
    }

    pub fn reset(&mut self) {
        self.cells.fill(0);
        self.heap.fill(0);
        self.draw.clear();
        self.pc = 0;
        self.ptr = 0;
        self.yielded = false;
        self.command_count = 0;
    }

    pub fn set_cell(&mut self, address: usize, value: u8) {
        if address < self.cells.len() {
            self.cells[address] = value;
        }
    }

    pub fn cell(&self, address: usize) -> u8 {
        self.cells.get(address).copied().unwrap_or(0)
    }

    pub fn heap_value(&self, arena: usize, index: usize) -> u8 {
        if arena < ARENA_COUNT && index < ARENA_SIZE {
            self.heap[arena * ARENA_SIZE + index]
        } else {
            0
        }
    }

    pub fn set_heap_value(&mut self, arena: usize, index: usize, value: u8) {
        if arena < ARENA_COUNT && index < ARENA_SIZE {
            self.heap[arena * ARENA_SIZE + index] = value;
        }
    }

    pub fn draw_commands(&self) -> &[DrawCmd] {
        &self.draw
    }

    /// Run until the program executes a YIELD command or the instruction
    /// budget is exhausted.  The budget bounds one displayed frame; a game
    /// that cannot finish a frame within it is treated as a runaway program.
    pub fn run_until_yield(&mut self, budget: u64) -> Result<RunOutcome, EngineError> {
        self.yielded = false;
        let mut used: u64 = 0;
        while used < budget {
            let op = match self.program.ops.get(self.pc) {
                Some(op) => op.clone(),
                None => return Err(EngineError::Halted),
            };
            self.pc += 1;
            used += 1;
            match op {
                Op::Add(delta) => {
                    let p = self.ptr;
                    let next = (self.cells[p] as i32 + delta).rem_euclid(256) as u8;
                    self.cells[p] = next;
                    if p == EXEC && next == 1 {
                        self.execute_command()?;
                    }
                }
                Op::Move(delta) => {
                    let next = self.ptr as i64 + delta as i64;
                    if !(0..TAPE_SIZE as i64).contains(&next) {
                        return Err(EngineError::PointerOutOfBounds(next as i32));
                    }
                    self.ptr = next as usize;
                }
                Op::Output => {
                    // The ABI uses MMIO, not stdio.  Keep '.' as a harmless
                    // no-op so ordinary Brainfuck sources still parse.
                }
                Op::Input => {
                    // EOF.
                    self.cells[self.ptr] = 0;
                }
                Op::JumpIfZero(target) => {
                    if self.cells[self.ptr] == 0 {
                        if target > self.program.ops.len() {
                            return Err(EngineError::ProgramCounterOutOfBounds);
                        }
                        self.pc = target;
                    }
                }
                Op::JumpIfNotZero(target) => {
                    if self.cells[self.ptr] != 0 {
                        if target > self.program.ops.len() {
                            return Err(EngineError::ProgramCounterOutOfBounds);
                        }
                        self.pc = target;
                    }
                }
            }
            if self.yielded {
                return Ok(RunOutcome::Yielded);
            }
        }
        Ok(RunOutcome::BudgetExceeded)
    }

    fn arg(&self, index: usize) -> u8 {
        if index < ARG.len() {
            self.cells[ARG[index]]
        } else {
            0
        }
    }

    fn arg16(&self, index: usize) -> u16 {
        u16::from(self.arg(index)) | (u16::from(self.arg(index + 1)) << 8)
    }

    fn arena_offset(&self, arena: u8, index: u8) -> Result<usize, EngineError> {
        if arena as usize >= ARENA_COUNT {
            return Err(EngineError::ArenaOutOfBounds(arena));
        }
        Ok(arena as usize * ARENA_SIZE + index as usize)
    }

    fn push_draw(&mut self, cmd: DrawCmd) -> Result<(), EngineError> {
        if self.draw.len() >= 200_000 {
            return Err(EngineError::DrawListTooLarge);
        }
        self.draw.push(cmd);
        Ok(())
    }

    fn execute_command(&mut self) -> Result<(), EngineError> {
        // The engine owns EXEC: a write of 1 is an edge-triggered strobe.
        self.cells[EXEC] = 0;
        let cmd = self.cells[CMD];
        self.command_count += 1;
        match cmd {
            0 => {}
            CMD_CLEAR => {
                self.draw.clear();
                let color = self.arg(0);
                self.push_draw(DrawCmd::Clear { color })?;
            }
            CMD_RECT => {
                self.push_draw(DrawCmd::Rect {
                    x: self.arg16(0),
                    y: self.arg16(2),
                    w: self.arg16(4),
                    h: self.arg16(6),
                    color: self.arg(8),
                })?;
            }
            CMD_RECT_LINES => {
                self.push_draw(DrawCmd::RectLines {
                    x: self.arg16(0),
                    y: self.arg16(2),
                    w: self.arg16(4),
                    h: self.arg16(6),
                    color: self.arg(8),
                })?;
            }
            CMD_LINE => {
                self.push_draw(DrawCmd::Line {
                    x1: self.arg16(0),
                    y1: self.arg16(2),
                    x2: self.arg16(4),
                    y2: self.arg16(6),
                    color: self.arg(8),
                })?;
            }
            CMD_CIRCLE => {
                self.push_draw(DrawCmd::Circle {
                    x: self.arg16(0),
                    y: self.arg16(2),
                    r: self.arg16(4),
                    color: self.arg(6),
                })?;
            }
            CMD_TEXT => {
                let len = (self.arg(6) as usize).min(TEXT_CAP);
                let mut text = String::with_capacity(len);
                for i in 0..len {
                    let b = self.cells[TEXT_BUF + i];
                    if b != 0 {
                        text.push(b as char);
                    }
                }
                self.push_draw(DrawCmd::Text {
                    x: self.arg16(0),
                    y: self.arg16(2),
                    color: self.arg(4),
                    size: u16::from(self.arg(5)),
                    text,
                })?;
            }
            CMD_NUMBER => {
                self.push_draw(DrawCmd::Number {
                    x: self.arg16(0),
                    y: self.arg16(2),
                    value: self.arg(4),
                    color: self.arg(5),
                    size: u16::from(self.arg(6)),
                })?;
            }
            CMD_CELL => {
                self.push_draw(DrawCmd::Cell {
                    x: self.arg(0),
                    y: self.arg(1),
                    color: self.arg(2),
                    style: self.arg(3),
                })?;
            }
            CMD_GRID => {
                self.push_draw(DrawCmd::Grid {
                    x: self.arg16(0),
                    y: self.arg16(2),
                    cols: self.arg(4),
                    rows: self.arg(5),
                    cell: self.arg(6),
                    color: self.arg(7),
                })?;
            }
            CMD_YIELD => {
                self.yielded = true;
            }
            CMD_LOAD => {
                let arena = self.arg(0);
                let index = self.arg(1);
                let offset = self.arena_offset(arena, index)?;
                self.cells[VALUE] = self.heap[offset];
            }
            CMD_STORE => {
                let arena = self.arg(0);
                let index = self.arg(1);
                let value = self.arg(2);
                let offset = self.arena_offset(arena, index)?;
                self.heap[offset] = value;
            }
            CMD_FILL => {
                let arena = self.arg(0);
                if arena as usize >= ARENA_COUNT {
                    return Err(EngineError::ArenaOutOfBounds(arena));
                }
                let start = self.arg(1) as usize;
                let len = self.arg(2) as usize;
                let value = self.arg(3);
                let base = arena as usize * ARENA_SIZE;
                let end = (start + len).min(ARENA_SIZE);
                for i in start..end {
                    self.heap[base + i] = value;
                }
            }
            other => return Err(EngineError::UnknownCommand(other)),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str, budget: u64) -> Machine {
        let program = parse(src).unwrap();
        let mut machine = Machine::new(program);
        let _ = machine.run_until_yield(budget);
        machine
    }

    #[test]
    fn arithmetic_and_loops() {
        // 5 * 3 = 15 in cell 1.
        let m = run("+++++[>+++<-]", 10_000);
        assert_eq!(m.cells[0], 0);
        assert_eq!(m.cells[1], 15);
    }

    #[test]
    fn jump_table() {
        let program = parse("[->+<]").unwrap();
        assert_eq!(program.ops.len(), 6);
        assert_eq!(program.ops[0], Op::JumpIfZero(6));
        assert_eq!(program.ops[5], Op::JumpIfNotZero(1));
    }

    #[test]
    fn mmio_load_store_fill() {
        // store 42 at heap[2][7], load it into tape cell 128, then yield.
        let mut m = Machine::new(parse("").unwrap());
        m.set_cell(ARG[0], 2);
        m.set_cell(ARG[1], 7);
        m.set_cell(ARG[2], 42);
        m.set_cell(CMD, CMD_STORE);
        m.set_cell(EXEC, 1);
        m.execute_command().unwrap();
        assert_eq!(m.heap_value(2, 7), 42);

        m.set_cell(ARG[0], 2);
        m.set_cell(ARG[1], 7);
        m.set_cell(CMD, CMD_LOAD);
        m.set_cell(EXEC, 1);
        m.execute_command().unwrap();
        assert_eq!(m.cell(VALUE), 42);

        m.set_cell(ARG[0], 2);
        m.set_cell(ARG[1], 0);
        m.set_cell(ARG[2], 255);
        m.set_cell(ARG[3], 255);
        m.set_cell(CMD, CMD_FILL);
        m.set_cell(EXEC, 1);
        m.execute_command().unwrap();
        assert_eq!(m.heap_value(2, 0), 255);
        assert_eq!(m.heap_value(2, 254), 255);
        assert_eq!(m.heap_value(2, 255), 0);
    }

    #[test]
    fn exec_strobe_clears_itself() {
        let mut m = Machine::new(parse("").unwrap());
        m.set_cell(CMD, CMD_RECT);
        m.set_cell(ARG[0], 1);
        m.set_cell(ARG[4], 2);
        m.set_cell(ARG[6], 3);
        m.set_cell(ARG[8], 4);
        m.set_cell(EXEC, 1);
        m.execute_command().unwrap();
        assert_eq!(m.cell(EXEC), 0);
        assert_eq!(m.draw.len(), 1);
    }
}
