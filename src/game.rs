//! Host-side frame loop and input plumbing for a Brainfuck game.

use crate::abi::*;
use crate::vm::{Machine, Program};

pub const DEFAULT_BUDGET: u64 = 40_000_000;
pub const DEFAULT_TICK_MS: u64 = 120;

#[derive(Clone, Copy, Debug, Default)]
pub struct InputState {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub space: bool,
    pub a: bool,
    pub d: bool,
    pub r: bool,
    pub p: bool,
    pub up_pressed: bool,
    pub down_pressed: bool,
    pub left_pressed: bool,
    pub right_pressed: bool,
    pub space_pressed: bool,
    pub a_pressed: bool,
    pub d_pressed: bool,
    pub r_pressed: bool,
    pub p_pressed: bool,
}

pub struct Runtime {
    pub machine: Machine,
    tick_ms: u64,
    accum_ms: u64,
    frame: u32,
    budget: u64,
    rng: u32,
    last_dir: u8,
    pub error: Option<String>,
    tick_count: u64,
}

impl Runtime {
    pub fn new(program: Program, tick_ms: u64, budget: u64, seed: u32) -> Self {
        Self {
            machine: Machine::new(program),
            tick_ms: tick_ms.max(1),
            accum_ms: 0,
            frame: 0,
            budget,
            rng: seed | 1,
            last_dir: 0,
            error: None,
            tick_count: 0,
        }
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    pub fn frame_count(&self) -> u32 {
        self.frame
    }

    fn next_u32(&mut self) -> u32 {
        // xorshift32: good enough for food and piece selection.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn write_inputs(&mut self, input: &InputState, tick: bool) {
        if input.up_pressed {
            self.last_dir = 1;
        }
        if input.right_pressed {
            self.last_dir = 2;
        }
        if input.down_pressed {
            self.last_dir = 3;
        }
        if input.left_pressed {
            self.last_dir = 4;
        }

        let random = self.next_u32();
        let rand7 = (random % 7) as u8;
        let values: [(usize, u8); 25] = [
            (UP, input.up as u8),
            (DOWN, input.down as u8),
            (LEFT, input.left as u8),
            (RIGHT, input.right as u8),
            (SPACE, input.space as u8),
            (KEY_A, input.a as u8),
            (KEY_D, input.d as u8),
            (KEY_R, input.r as u8),
            (KEY_P, input.p as u8),
            (UP_PRESSED, input.up_pressed as u8),
            (DOWN_PRESSED, input.down_pressed as u8),
            (LEFT_PRESSED, input.left_pressed as u8),
            (RIGHT_PRESSED, input.right_pressed as u8),
            (SPACE_PRESSED, input.space_pressed as u8),
            (A_PRESSED, input.a_pressed as u8),
            (D_PRESSED, input.d_pressed as u8),
            (R_PRESSED, input.r_pressed as u8),
            (P_PRESSED, input.p_pressed as u8),
            (TICK, tick as u8),
            (FRAME, (self.frame & 0xff) as u8),
            (RANDOM, (random & 0xff) as u8),
            (RAND_LO, (random & 0x0f) as u8),
            (RAND_HI, ((random >> 4) & 0x0f) as u8),
            (RAND_7, rand7),
            (LAST_DIR, self.last_dir),
        ];
        for (address, value) in values {
            self.machine.set_cell(address, value);
        }
    }

    /// Advance one host frame.  `dt_ms` is the elapsed wall-clock time since
    /// the previous call.  The Brainfuck program is resumed until it yields.
    pub fn frame(&mut self, input: &InputState, dt_ms: u64) -> Result<(), String> {
        if let Some(err) = &self.error {
            return Err(err.clone());
        }
        self.accum_ms = self.accum_ms.saturating_add(dt_ms);
        let tick = if self.accum_ms >= self.tick_ms {
            self.accum_ms %= self.tick_ms;
            self.tick_count += 1;
            true
        } else {
            false
        };
        self.frame = self.frame.wrapping_add(1);
        self.write_inputs(input, tick);
        match self.machine.run_until_yield(self.budget) {
            Ok(RunOutcome::Yielded) => Ok(()),
            Ok(RunOutcome::BudgetExceeded) => {
                let message = format!(
                    "frame budget exhausted after {} M instructions (YIELD not reached)",
                    self.budget / 1_000_000
                );
                self.error = Some(message.clone());
                Err(message)
            }
            Err(err) => {
                let message = err.to_string();
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::parse;

    #[test]
    fn runtime_ticks_and_yields() {
        // An infinite loop which strobes EXEC every iteration.  CMD is 10
        // (YIELD), so every iteration hands control back to the host.
        // Use cell 128 as the outer loop counter: cells 0..24 are input
        // registers and are overwritten by the host on every frame.
        let src = format!(
            "{}{}[{}[-]{}{}[-]+{}]",
            ">".repeat(128),
            "+",
            "<".repeat(128 - CMD),
            "+".repeat(CMD_YIELD as usize),
            ">".repeat(EXEC - CMD),
            ">".repeat(128 - EXEC),
        );
        let program = parse(&src).unwrap();
        let mut rt = Runtime::new(program, 100, 1_000_000, 7);
        let input = InputState::default();
        rt.frame(&input, 16).unwrap();
        assert_eq!(rt.tick_count(), 0);
        rt.frame(&input, 90).unwrap();
        assert_eq!(rt.tick_count(), 1);
    }

    const CMD_YIELD: u8 = 10;
}
