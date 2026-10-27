#!/usr/bin/env python3
"""bfasm 鈥?a tiny macro assembler that emits pure Brainfuck.

The games in ../games are written as a sequence of Brainfuck-level
operations (memory cells, 8-bit copy/add/compare macros, structured loops),
plus calls into the engine ABI.  This tool expands the macros and writes a
standard Brainfuck program that `bfgame` can run.

Usage:
    python tools/bfasm.py games/src/snake.bfa games/snake.bf
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable

# ---------------------------------------------------------------------------
# ABI symbols
# ---------------------------------------------------------------------------

INPUT_SYMBOLS = [
    "UP", "DOWN", "LEFT", "RIGHT", "SPACE", "KEY_A", "KEY_D", "KEY_R",
    "KEY_P", "UP_PRESSED", "DOWN_PRESSED", "LEFT_PRESSED", "RIGHT_PRESSED",
    "SPACE_PRESSED", "A_PRESSED", "D_PRESSED", "R_PRESSED", "P_PRESSED",
    "TICK", "FRAME", "RANDOM", "RAND_LO", "RAND_HI", "RAND_7", "LAST_DIR",
]

SCRATCH = [
    "_cp", "_eqa", "_eqb", "_eqf", "_and1", "_and2", "_or1", "_or2",
    "_notf", "_add", "_sub", "_rot",
]

ABI_SYMBOLS = {name: i for i, name in enumerate(INPUT_SYMBOLS)}
ABI_SYMBOLS.update({
    "CMD": 32,
    "VALUE": 45,
    "EXEC": 46,
    "TEXT_BUF": 64,
})
for n in range(12):
    ABI_SYMBOLS[f"ARG{n}"] = 33 + n
# Readable aliases used by game sources.
ABI_SYMBOLS["A"] = 33
ABI_SYMBOLS["B"] = 34
ABI_SYMBOLS["C"] = 35
ABI_SYMBOLS["D"] = 36
ABI_SYMBOLS["E"] = 37
ABI_SYMBOLS["F"] = 38
ABI_SYMBOLS["G"] = 39
ABI_SYMBOLS["H"] = 40

DATA_BASE = 256
TAPE_SIZE = 65536
TEXT_CAP = 64

# Reserved compiler scratch, immediately after the text buffer.
SCRATCH_ADDR = {name: 128 + i for i, name in enumerate(SCRATCH)}


def strip_comment(line: str) -> str:
    out = []
    quoted = False
    escape = False
    for ch in line:
        if escape:
            out.append(ch)
            escape = False
            continue
        if ch == "\\" and quoted:
            out.append(ch)
            escape = True
            continue
        if ch == '"':
            quoted = not quoted
            out.append(ch)
            continue
        if ch == "#" and not quoted:
            break
        out.append(ch)
    return "".join(out).strip()


def split_args(text: str) -> list[str]:
    args: list[str] = []
    cur: list[str] = []
    depth = 0
    quoted = False
    escape = False
    for ch in text:
        if escape:
            cur.append(ch)
            escape = False
            continue
        if ch == "\\" and quoted:
            cur.append(ch)
            escape = True
            continue
        if ch == '"':
            quoted = not quoted
            cur.append(ch)
            continue
        if not quoted:
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
            elif ch == "," and depth == 0:
                args.append("".join(cur).strip())
                cur = []
                continue
        cur.append(ch)
    if cur or text.strip():
        args.append("".join(cur).strip())
    return args


def parse_call(line: str) -> tuple[str, list[str]] | None:
    m = re.match(r"^([A-Za-z_]\w*)\s*\((.*)\)\s*$", line)
    if not m:
        return None
    return m.group(1), split_args(m.group(2))


def parse_int(token: str) -> int | None:
    t = token.strip()
    try:
        return int(t, 0)
    except (TypeError, ValueError):
        return None


# ---------------------------------------------------------------------------
# Macro pass + block parser
# ---------------------------------------------------------------------------

@dataclass
class Macro:
    params: list[str]
    body: list[tuple[int, str]]
    line: int


@dataclass
class If:
    cond: str
    body: list["Node"]
    line: int


@dataclass
class Loop:
    cond: str
    body: list["Node"]
    line: int


@dataclass
class Stmt:
    text: str
    line: int


Node = If | Loop | Stmt


class AsmError(Exception):
    pass


def collect_macros(lines: list[tuple[int, str]]) -> tuple[dict[str, Macro], list[tuple[int, str]]]:
    macros: dict[str, Macro] = {}
    main: list[tuple[int, str]] = []
    i = 0
    while i < len(lines):
        lineno, line = lines[i]
        m = re.match(r"^macro\s+([A-Za-z_]\w*)\s*\((.*)\)\s*\{\s*$", line)
        if not m:
            main.append((lineno, line))
            i += 1
            continue
        name = m.group(1)
        params = [p.strip().lstrip("$") for p in split_args(m.group(2)) if p.strip()]
        body: list[tuple[int, str]] = []
        depth = 0
        i += 1
        while i < len(lines):
            bline_no, bline = lines[i]
            if depth == 0 and bline.strip() == "}":
                break
            body.append((bline_no, bline))
            depth += bline.count("{") - bline.count("}")
            if depth < 0:
                raise AsmError(f"line {bline_no}: unexpected '}}' in macro {name}")
            i += 1
        else:
            raise AsmError(f"macro {name} is missing its closing '}}'")
        macros[name] = Macro(params, body, lineno)
        i += 1
    return macros, main


def substitute(line: str, mapping: dict[str, str]) -> str:
    def repl(m: re.Match[str]) -> str:
        key = m.group(1)
        if key not in mapping:
            raise AsmError(f"unknown macro parameter ${key}")
        return mapping[key]

    return re.sub(r"\$([A-Za-z_]\w*)", repl, line)


def expand_lines(
    lines: list[tuple[int, str]],
    macros: dict[str, Macro],
    depth: int = 0,
    active: tuple[str, ...] = (),
) -> list[tuple[int, str]]:
    if depth > 40:
        raise AsmError("macro expansion is too deep (recursive macro?)")
    out: list[tuple[int, str]] = []
    for lineno, raw in lines:
        line = strip_comment(raw)
        if not line:
            continue
        call = parse_call(line)
        if call and call[0] in macros:
            name, args = call
            macro = macros[name]
            if name in active:
                raise AsmError(f"line {lineno}: recursive macro call {name}")
            if len(args) != len(macro.params):
                raise AsmError(
                    f"line {lineno}: macro {name} expects {len(macro.params)} "
                    f"arguments, got {len(args)}"
                )
            mapping = dict(zip(macro.params, args))
            expanded = [(macro.line, substitute(body_line, mapping)) for _, body_line in macro.body]
            out.extend(expand_lines(expanded, macros, depth + 1, active + (name,)))
        else:
            out.append((lineno, line))
    return out


def parse_block(lines: list[tuple[int, str]], index: int = 0, top: bool = True) -> tuple[list[Node], int]:
    nodes: list[Node] = []
    while index < len(lines):
        lineno, line = lines[index]
        if line == "}":
            if top:
                raise AsmError(f"line {lineno}: unexpected '}}'")
            return nodes, index + 1
        m_if = re.match(r"^if\s+(.+?)\s*\{\s*$", line)
        m_loop = re.match(r"^loop\s+(.+?)\s*\{\s*$", line)
        m_if_single = re.match(r"^if\s+(.+?)\s*\{\s*(.+?)\s*\}\s*$", line)
        if m_if_single:
            body = [
                Stmt(part.strip(), lineno)
                for part in m_if_single.group(2).split(";")
                if part.strip()
            ]
            nodes.append(If(m_if_single.group(1).strip(), body, lineno))
            index += 1
            continue
        if m_if or m_loop:
            body, next_index = parse_block(lines, index + 1, top=False)
            if m_if:
                nodes.append(If(m_if.group(1).strip(), body, lineno))
            else:
                nodes.append(Loop(m_loop.group(1).strip(), body, lineno))
            index = next_index
            continue
        if line.endswith("{"):
            raise AsmError(f"line {lineno}: unsupported block syntax: {line}")
        nodes.append(Stmt(line, lineno))
        index += 1
    if not top:
        raise AsmError("missing '}' at end of file")
    return nodes, index


# ---------------------------------------------------------------------------
# Compiler
# ---------------------------------------------------------------------------

class Asm:
    def __init__(self, debug: bool = False) -> None:
        self.symbols: dict[str, int] = dict(ABI_SYMBOLS)
        self.symbols.update(SCRATCH_ADDR)
        self.next_data = DATA_BASE
        self.code: list[str] = []
        self.ptr = 0
        self.current_line = 0
        self.debug = debug

    # -- utilities ---------------------------------------------------------
    def fail(self, message: str) -> None:
        raise AsmError(f"line {self.current_line}: {message}")

    def emit(self, text: str) -> None:
        if text:
            self.code.append(text)

    def move_to(self, addr: int) -> None:
        if addr < 0 or addr >= TAPE_SIZE:
            self.fail(f"address {addr} is outside the 64 KiB tape")
        delta = addr - self.ptr
        if delta > 0:
            self.emit(">" * delta)
        elif delta < 0:
            self.emit("<" * (-delta))
        self.ptr = addr

    def addr(self, token: str | int) -> int:
        if isinstance(token, int):
            if token < 0 or token >= TAPE_SIZE:
                self.fail(f"address {token} is outside the tape")
            return token
        token = token.strip()
        m = re.fullmatch(r"([A-Za-z_]\w*)([+-]\d+)?", token)
        if m and m.group(1) in self.symbols:
            base = self.symbols[m.group(1)]
            if m.group(2):
                base += int(m.group(2))
            if base < 0 or base >= TAPE_SIZE:
                self.fail(f"address of {token} is outside the tape")
            return base
        value = parse_int(token)
        if value is not None:
            if value < 0 or value >= TAPE_SIZE:
                self.fail(f"address {value} is outside the tape")
            return value
        self.fail(f"unknown cell/address: {token}")

    def is_cell(self, token: str) -> bool:
        token = token.strip()
        m = re.fullmatch(r"([A-Za-z_]\w*)([+-]\d+)?", token)
        if m and m.group(1) in self.symbols:
            return True
        return False

    def const(self, token: str, what: str = "constant") -> int:
        value = parse_int(token)
        if value is None:
            if token in self.symbols and token.startswith("_const_"):
                return self.symbols[token]
            self.fail(f"expected {what}, got {token!r}")
        return value

    def u8(self, token: str, what: str = "byte") -> int:
        value = self.const(token, what)
        if not 0 <= value <= 255:
            self.fail(f"{what} {value} is outside 0..255")
        return value

    def u16(self, token: str, what: str = "word") -> int:
        value = self.const(token, what)
        if not 0 <= value <= 0xFFFF:
            self.fail(f"{what} {value} is outside 0..65535")
        return value

    def alloc(self, name: str, size: int = 1) -> None:
        if name in self.symbols:
            self.fail(f"symbol already defined: {name}")
        if self.next_data + size > TAPE_SIZE:
            self.fail("out of tape memory")
        self.symbols[name] = self.next_data
        self.next_data += size

    # -- low-level ops -----------------------------------------------------
    def seti(self, cell: str, value: int) -> None:
        addr = self.addr(cell)
        self.move_to(addr)
        self.emit("[-]")
        if value:
            self.emit("+" * (value % 256))

    def inc(self, cell: str) -> None:
        self.move_to(self.addr(cell))
        self.emit("+")

    def dec(self, cell: str) -> None:
        self.move_to(self.addr(cell))
        self.emit("-")

    def copy(self, dst: str, src: str) -> None:
        d = self.addr(dst)
        s = self.addr(src)
        if d == s:
            return
        cp = SCRATCH_ADDR["_cp"]
        if d == cp or s == cp:
            self.fail("copy destination/source cannot be the internal copy scratch")
        self.move_to(d)
        self.emit("[-]")
        self.move_to(s)
        self.emit("[")
        self.emit("-")
        self.move_to(d)
        self.emit("+")
        self.move_to(cp)
        self.emit("+")
        self.move_to(s)
        self.emit("]")
        self.move_to(cp)
        self.emit("[")
        self.emit("-")
        self.move_to(s)
        self.emit("+")
        self.move_to(cp)
        self.emit("]")

    def move_cell(self, dst: str, src: str) -> None:
        d = self.addr(dst)
        s = self.addr(src)
        if d == s:
            self.fail("move source and destination are the same")
        self.move_to(d)
        self.emit("[-]")
        self.move_to(s)
        self.emit("[")
        self.emit("-")
        self.move_to(d)
        self.emit("+")
        self.move_to(s)
        self.emit("]")

    def copy_operand(self, dst: str, token: str) -> None:
        """Copy a cell, or set a constant, into dst."""
        value = parse_int(token)
        if value is not None:
            self.seti(dst, value & 0xFF)
        elif self.is_cell(token):
            self.copy(dst, token)
        else:
            self.fail(f"expected a cell or constant, got {token!r}")

    def add(self, dst: str, src: str) -> None:
        add = SCRATCH_ADDR["_add"]
        self.copy(add, src)
        d = self.addr(dst)
        self.move_to(add)
        self.emit("[")
        self.emit("-")
        self.move_to(d)
        self.emit("+")
        self.move_to(add)
        self.emit("]")

    def sub(self, dst: str, src: str) -> None:
        sub = SCRATCH_ADDR["_sub"]
        self.copy(sub, src)
        d = self.addr(dst)
        self.move_to(sub)
        self.emit("[")
        self.emit("-")
        self.move_to(d)
        self.emit("-")
        self.move_to(sub)
        self.emit("]")

    def eq_impl(self, dst: str, a: str, b: str) -> None:
        qa = SCRATCH_ADDR["_eqa"]
        qb = SCRATCH_ADDR["_eqb"]
        qf = SCRATCH_ADDR["_eqf"]
        self.seti(dst, 0)
        self.copy_operand(qa, a)
        self.copy_operand(qb, b)
        # qa -= qb
        self.move_to(qb)
        self.emit("[")
        self.emit("-")
        self.move_to(qa)
        self.emit("-")
        self.move_to(qb)
        self.emit("]")
        self.seti(qf, 1)
        self.move_to(qa)
        self.emit("[")
        self.seti(qf, 0)
        self.seti(qa, 0)
        self.emit("]")
        self.copy(dst, qf)

    def neq(self, dst: str, a: str, b: str) -> None:
        self.eq_impl(dst, a, b)
        self.not_(dst, dst)

    def not_(self, dst: str, src: str) -> None:
        nf = SCRATCH_ADDR["_notf"]
        self.copy(nf, src)
        self.seti(dst, 1)
        self.move_to(nf)
        self.emit("[")
        self.seti(dst, 0)
        self.seti(nf, 0)
        self.emit("]")

    def and_(self, dst: str, a: str, b: str) -> None:
        a1 = SCRATCH_ADDR["_and1"]
        a2 = SCRATCH_ADDR["_and2"]
        self.copy_operand(a1, a)
        self.copy_operand(a2, b)
        self.seti(dst, 0)
        self.move_to(a1)
        self.emit("[")
        self.move_to(a2)
        self.emit("[")
        self.seti(dst, 1)
        self.seti(a2, 0)
        self.emit("]")
        self.seti(a1, 0)
        self.emit("]")

    def or_(self, dst: str, a: str, b: str) -> None:
        o1 = SCRATCH_ADDR["_or1"]
        o2 = SCRATCH_ADDR["_or2"]
        self.copy_operand(o1, a)
        self.copy_operand(o2, b)
        self.seti(dst, 0)
        self.move_to(o1)
        self.emit("[")
        self.seti(dst, 1)
        self.seti(o1, 0)
        self.emit("]")
        self.move_to(o2)
        self.emit("[")
        self.seti(dst, 1)
        self.seti(o2, 0)
        self.emit("]")

    # -- blocks ------------------------------------------------------------
    def compile_block(self, nodes: list[Node]) -> None:
        for node in nodes:
            self.current_line = getattr(node, "line", self.current_line)
            if isinstance(node, Stmt):
                self.compile_stmt(node.text)
            elif isinstance(node, If):
                addr = self.addr(node.cond)
                self.move_to(addr)
                self.emit("[")
                self.compile_block(node.body)
                self.move_to(addr)
                self.emit("[-]")
                self.emit("]")
            elif isinstance(node, Loop):
                addr = self.addr(node.cond)
                self.move_to(addr)
                self.emit("[")
                self.compile_block(node.body)
                self.move_to(addr)
                self.emit("]")

    # -- ABI commands ------------------------------------------------------
    def command(self, op: int) -> None:
        self.seti("CMD", op)

    def exec_command(self) -> None:
        self.seti("EXEC", 1)

    def arg(self, n: int, token: str) -> None:
        self.copy_operand(f"ARG{n}", token)

    def arg16(self, n: int, token: str) -> None:
        value = self.u16(token)
        self.seti(f"ARG{n}", value & 0xFF)
        self.seti(f"ARG{n + 1}", (value >> 8) & 0xFF)

    def cmd(self, op: int, args: Iterable[tuple[int, str]]) -> None:
        self.command(op)
        for n, token in args:
            self.arg(n, token)
        self.exec_command()

    def compile_stmt(self, line: str) -> None:
        call = parse_call(line)
        if not call:
            self.fail(f"not a statement: {line}")
        name, args = call
        if self.debug:
            self.code.append(f"\n# L{self.current_line}: {line}\n")
        try:
            self.compile_call(name, args)
        except AsmError:
            raise
        except Exception as exc:  # pragma: no cover - defensive
            raise AsmError(f"line {self.current_line}: {exc}") from exc

    def compile_call(self, name: str, args: list[str]) -> None:
        # -- declarations / pointer ---------------------------------------
        if name == "at":
            self.require(args, 1, name)
            self.move_to(self.addr(args[0]))
        elif name == "raw":
            self.require(args, 1, name)
            text = self.unquote(args[0])
            if any(c not in "+-<>[].," for c in text):
                self.fail("raw() only accepts Brainfuck commands")
            self.emit(text)
        elif name == "seti":
            self.require(args, 2, name)
            self.seti(args[0], self.u8(args[1]))
        elif name == "inc":
            self.require(args, 1, name)
            self.inc(args[0])
        elif name == "dec":
            self.require(args, 1, name)
            self.dec(args[0])
        elif name == "copy":
            self.require(args, 2, name)
            self.copy(args[0], args[1])
        elif name == "move":
            self.require(args, 2, name)
            self.move_cell(args[0], args[1])
        elif name == "addi":
            self.require(args, 2, name)
            self.addi(args[0], self.u8(args[1]))
        elif name == "subi":
            self.require(args, 2, name)
            self.subi(args[0], self.u8(args[1]))
        elif name == "add":
            self.require(args, 2, name)
            self.add(args[0], args[1])
        elif name == "sub":
            self.require(args, 2, name)
            self.sub(args[0], args[1])
        elif name == "eq":
            self.require(args, 3, name)
            self.eq_impl(args[0], args[1], args[2])
        elif name == "neq":
            self.require(args, 3, name)
            self.neq(args[0], args[1], args[2])
        elif name == "not":
            self.require(args, 2, name)
            self.not_(args[0], args[1])
        elif name == "and":
            self.require(args, 3, name)
            self.and_(args[0], args[1], args[2])
        elif name == "or":
            self.require(args, 3, name)
            self.or_(args[0], args[1], args[2])
        # -- graphics ------------------------------------------------------
        elif name == "clear":
            self.require(args, 1, name)
            self.command(1)
            self.seti("ARG0", self.u8(args[0]))
            self.exec_command()
        elif name == "draw_rect":
            self.require(args, 5, name)
            self.command(2)
            self.arg16(0, args[0])
            self.arg16(2, args[1])
            self.arg16(4, args[2])
            self.arg16(6, args[3])
            self.seti("ARG8", self.u8(args[4]))
            self.exec_command()
        elif name == "draw_rect_lines":
            self.require(args, 5, name)
            self.command(3)
            self.arg16(0, args[0])
            self.arg16(2, args[1])
            self.arg16(4, args[2])
            self.arg16(6, args[3])
            self.seti("ARG8", self.u8(args[4]))
            self.exec_command()
        elif name == "draw_line":
            self.require(args, 5, name)
            self.command(4)
            self.arg16(0, args[0])
            self.arg16(2, args[1])
            self.arg16(4, args[2])
            self.arg16(6, args[3])
            self.seti("ARG8", self.u8(args[4]))
            self.exec_command()
        elif name == "draw_circle":
            self.require(args, 4, name)
            self.command(5)
            self.arg16(0, args[0])
            self.arg16(2, args[1])
            self.arg16(4, args[2])
            self.seti("ARG6", self.u8(args[3]))
            self.exec_command()
        elif name == "draw_text":
            self.require(args, 5, name)
            text = self.unquote(args[4])
            raw = text.encode("ascii", "strict")
            if len(raw) >= TEXT_CAP:
                self.fail("draw_text string is longer than 63 bytes")
            for i, byte in enumerate(raw):
                self.seti(f"TEXT_BUF+{i}", byte)
            self.command(6)
            self.arg16(0, args[0])
            self.arg16(2, args[1])
            self.seti("ARG4", self.u8(args[2]))
            self.seti("ARG5", self.u8(args[3]))
            self.seti("ARG6", len(raw))
            self.exec_command()
        elif name == "draw_number":
            self.require(args, 5, name)
            self.command(7)
            self.arg16(0, args[0])
            self.arg16(2, args[1])
            self.copy_operand("ARG4", args[2])
            self.seti("ARG5", self.u8(args[3]))
            self.seti("ARG6", self.u8(args[4]))
            self.exec_command()
        elif name == "cell":
            self.require(args, 4, name)
            self.command(8)
            self.copy_operand("ARG0", args[0])
            self.copy_operand("ARG1", args[1])
            self.copy_operand("ARG2", args[2])
            self.seti("ARG3", self.u8(args[3]))
            self.exec_command()
        elif name == "draw_grid":
            self.require(args, 6, name)
            self.command(9)
            self.arg16(0, args[0])
            self.arg16(2, args[1])
            self.seti("ARG4", self.u8(args[2]))
            self.seti("ARG5", self.u8(args[3]))
            self.seti("ARG6", self.u8(args[4]))
            self.seti("ARG7", self.u8(args[5]))
            self.exec_command()
        elif name == "yield":
            self.require(args, 0, name)
            self.command(10)
            self.exec_command()
        # -- heap ----------------------------------------------------------
        elif name == "load":
            self.require(args, 2, name)
            self.command(11)
            self.copy_operand("ARG0", args[0])
            self.copy_operand("ARG1", args[1])
            self.exec_command()
        elif name == "store":
            self.require(args, 3, name)
            self.command(12)
            self.copy_operand("ARG0", args[0])
            self.copy_operand("ARG1", args[1])
            self.copy_operand("ARG2", args[2])
            self.exec_command()
        elif name == "fill":
            self.require(args, 4, name)
            self.command(13)
            self.copy_operand("ARG0", args[0])
            self.copy_operand("ARG1", args[1])
            self.copy_operand("ARG2", args[2])
            self.copy_operand("ARG3", args[3])
            self.exec_command()
        else:
            self.fail(f"unknown function/macro: {name}")

    def addi(self, cell: str, value: int) -> None:
        self.move_to(self.addr(cell))
        if value:
            self.emit("+" * (value % 256))

    def subi(self, cell: str, value: int) -> None:
        self.move_to(self.addr(cell))
        if value:
            self.emit("-" * (value % 256))

    def require(self, args: list[str], count: int, name: str) -> None:
        if len(args) != count:
            self.fail(f"{name} expects {count} argument(s), got {len(args)}")

    def unquote(self, token: str) -> str:
        token = token.strip()
        if len(token) >= 2 and token[0] == '"' and token[-1] == '"':
            # Minimal escape handling.
            body = token[1:-1]
            return bytes(body, "utf-8").decode("unicode_escape")
        self.fail(f"expected a quoted string, got {token!r}")


# ---------------------------------------------------------------------------
# Top-level compile
# ---------------------------------------------------------------------------

DIRECTIVE_RE = re.compile(r"^(var|array)\s+([A-Za-z_]\w*)(?:\s+(\d+))?\s*$")


def read_source(path: Path) -> list[tuple[int, str]]:
    out: list[tuple[int, str]] = []
    for i, raw in enumerate(path.read_text(encoding="utf-8-sig").splitlines(), 1):
        line = strip_comment(raw)
        if line:
            out.append((i, line))
    return out


def compile_source(path: Path, debug: bool = False) -> str:
    raw_lines = read_source(path)
    macros, main_lines = collect_macros(raw_lines)
    expanded = expand_lines(main_lines, macros)
    nodes, _ = parse_block(expanded)

    asm = Asm(debug=debug)
    # Declarations are processed in source order.  They are not executable.
    executable: list[Node] = []
    for node in nodes:
        if isinstance(node, Stmt):
            m = DIRECTIVE_RE.match(node.text)
            if m:
                kind, name, size_text = m.group(1), m.group(2), m.group(3)
                asm.current_line = node.line
                asm.alloc(name, 1 if kind == "var" else int(size_text or "1"))
                continue
        executable.append(node)

    asm.compile_block(executable)
    return "".join(asm.code)


def main(argv: list[str]) -> int:
    debug = False
    rest: list[str] = []
    for arg in argv[1:]:
        if arg == "--debug":
            debug = True
        else:
            rest.append(arg)
    if len(rest) != 2:
        print("usage: bfasm.py [--debug] <input.bfa> <output.bf>", file=sys.stderr)
        return 2
    src = Path(rest[0])
    dst = Path(rest[1])
    try:
        program = compile_source(src, debug=debug)
    except AsmError as err:
        print(f"bfasm: {src}: {err}", file=sys.stderr)
        return 1
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_text(program, encoding="ascii")
    print(f"bfasm: {src} -> {dst} ({len(program)} Brainfuck instructions)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

