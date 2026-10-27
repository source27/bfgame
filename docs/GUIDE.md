# BFGame 指南

> 一个 Rust + macroquad 的迷你游戏引擎，游戏逻辑语言是 Brainfuck。
> 运行时只执行 `+-<>[],.` 八种字符；`snake.bf` / `tetris.bf` 是纯 Brainfuck 程序。

---

## 1. 目标与边界

这个项目的目标不是做通用商业引擎，而是把“**游戏逻辑真的在 BF 里跑**”这件事做完整：

- Rust 负责窗口、键盘、渲染、计时和调试；
- BF 负责所有游戏状态：蛇的方向/身体/食物/死亡，Tetris 的方块、旋转、碰撞、消行、分数；
- 引擎提供一组内存映射寄存器和一个很小的堆，让 BF 能绘图和按下标存取数组；
- 游戏以 `.bf` 文件交付，命令行直接运行：

```powershell
bfgame snake.bf
bfgame tetris.bf
```

BF 没有变量名、函数和结构化控制流。为了可维护性，仓库同时提供 `.bfa` 宏汇编源文件和 `tools/bfasm.py`，把宏展开成纯 BF。**汇编器只参与开发，不参与运行**；`bfgame` 收到的是已经生成的 `.bf`。

---

## 2. 运行

### 2.1 构建

```powershell
cargo build --release --offline
```

离线依赖在 `vendor/`，由 `.cargo/config.toml` 指向。Windows MSVC 已验证。若你的环境可以访问 crates.io，也可以删除 `.cargo/config.toml` 后正常联网构建（GNU Windows 不在支持范围内，因为 `vendor` 里两个 GNU-only 包是空 stub）。

### 2.2 运行

```powershell
# 从仓库根目录
.\target\release\bfgame.exe snake.bf
.\target\release\bfgame.exe tetris.bf

# 任意目录：安装后使用内置游戏
cargo install --path . --offline
bfgame snake.bf
bfgame tetris.bf
```

查找顺序：

1. 参数指定的实际文件；
2. 当前目录下的 `games/<name>`；
3. 编译进二进制的同名内置游戏（`snake.bf` / `tetris.bf`）。

所以从仓库根目录运行时，`snake.bf` 会找到 `games/snake.bf`；安装后从任意目录运行则使用内置版本。

### 2.3 CLI

```text
bfgame <game.bf> [OPTIONS]

--tick-ms N          逻辑 tick 间隔，默认 120 ms
--budget N           每个显示帧最多执行多少条 BF 指令，默认 40,000,000
--seed N             固定 RNG 种子，便于复现
--frames N           显示 N 帧后退出
--screenshot PATH    把最后一帧保存成 PNG（隐含 --frames 3）
--help
```

举例：

```powershell
bfgame snake.bf --seed 7 --frames 30 --screenshot docs/snake.png
bfgame tetris.bf --tick-ms 60
```

> `--screenshot` 在 `next_frame` 之前抓取当前后缓冲，因此保存的是你看到的最后一帧，而不是清空后的窗口。

### 2.4 键盘

| 游戏 | 操作 |
| --- | --- |
| Snake | 方向键；`R` 重开 |
| Tetris | `←`/`→` 移动；`↓` 软降；`A`/`D` 顺时针旋转；`Space` 硬降；`R` 重开 |
| 通用 | `Esc` 或关闭窗口退出 |

引擎每帧读取键盘，把按下/按住状态写进 BF 输入寄存器。`LAST_DIR` 会记住最后一次方向键，避免 Snake 在两次逻辑 tick 之间丢失按键。

---

## 3. 整体架构

```text
┌──────────────────────────── macroquad ────────────────────────────┐
│  window / keyboard / GPU                                           │
└───────────────▲───────────────────────────────────────┬───────────┘
                │ DrawCmd replay                        │ InputState
┌───────────────┴───────────────────────────────────────▼───────────┐
│ Runtime                                                           │
│  - tick accumulator (TICK)     - RNG / RAND_LO / RAND_HI / RAND_7 │
│  - LAST_DIR latch              - frame budget                     │
└───────────────▲───────────────────────────────────────┬───────────┘
                │ YIELD                                 │ MMIO writes
┌───────────────┴───────────────────────────────────────▼───────────┐
│ Brainfuck VM (64 KiB tape, u8 cells, precomputed jumps)           │
│   memory-mapped I/O + 32×256 engine heap + retained display list  │
└───────────────────────────────────────────────────────────────────┘
```

### 3.1 帧协议

1. 宿主更新输入寄存器和 `TICK`/随机数。
2. BF 从上次暂停处继续执行。
3. BF 写 `CMD`/参数，然后写 `EXEC=1`。每条命令同步执行。
4. BF 执行 `CMD=10`（`YIELD`）时 VM 暂停，把控制权交回宿主。
5. 宿主把 retained display list 重放一遍，`next_frame().await`。
6. 下一帧从 BF 暂停处继续。

一个合法游戏通常写成：

```text
seti(run, 1)
loop run {
    ... 每帧处理输入 ...
    ... 必要时重画 ...
    yield()
}
```

`loop run` 是无限循环；只要不把 `run` 清零，BF 程序就会一直跑。每帧必须到达一次 `YIELD`，否则会触发帧预算错误。

### 3.2 为什么是 retained display list

绘制命令不会在下一帧自动消失，只有 `CLEAR` 会重置命令列表。游戏只在状态变化时重画（`dirty` 标志）。这样：

- 静止画面不消耗 BF 指令；
- BF 只需要在 Snake 移动、Tetris 下落/移动/消行时生成一帧绘制命令；
- 宿主每帧重放列表，画面依然稳定。

命令列表上限 200,000 条，超过会报错。

---

## 4. 内存映射

BF 磁带 65,536 个 8 位单元。前 256 个单元是引擎 ABI/保留区：

| 地址 | 用途 |
| --- | --- |
| `0..24` | 输入寄存器（每帧由宿主覆盖） |
| `32` | `CMD` 当前命令 |
| `33..44` | `ARG0..ARG11` 命令参数 |
| `45` | `VALUE`，`LOAD` 的结果 |
| `46` | `EXEC`，写入 1 触发命令；执行后宿主清零 |
| `47` | 保留 |
| `64..127` | 文本缓冲区（64 字节） |
| `128..191` | 汇编器内部 scratch（保留） |
| `256..65535` | 游戏自己的数据 |

**游戏变量必须从 256 开始**（`bfasm` 自动这样做）。不要把数据放在 0..255：输入寄存器每帧会被覆盖，命令区也会被改写。

### 4.1 输入寄存器

| 地址 | 名称 | 含义 |
| --- | --- | --- |
| 0 | `UP` | ↑ 按住 |
| 1 | `DOWN` | ↓ 按住 |
| 2 | `LEFT` | ← 按住 |
| 3 | `RIGHT` | → 按住 |
| 4 | `SPACE` | 空格按住 |
| 5 | `KEY_A` | A 按住 |
| 6 | `KEY_D` | D 按住 |
| 7 | `KEY_R` | R 按住 |
| 8 | `KEY_P` | P 按住 |
| 9..17 | `*_PRESSED` | 对应键“本帧刚按下” |
| 18 | `TICK` | 本帧是一个逻辑 tick |
| 19 | `FRAME` | 帧计数低 8 位 |
| 20 | `RANDOM` | 随机字节 0..255 |
| 21 | `RAND_LO` | 随机低 4 位 0..15 |
| 22 | `RAND_HI` | 随机高 4 位 0..15 |
| 23 | `RAND_7` | 均匀随机 0..6 |
| 24 | `LAST_DIR` | 最后按下的方向：1↑ 2→ 3↓ 4←；0 表示还没有 |

`TICK` 只在到达 `--tick-ms` 的那一帧为 1；下一帧恢复 0。Tetris 用 `TICK` 做重力，用 `*_PRESSED` 做立即移动/旋转。Snake 用 `LAST_DIR` 转向。

### 4.2 命令表

所有多字节坐标都是 **little-endian 16 位**：低字节在前。
`EXEC` 是边沿触发：写 1 立即执行，执行后自动清 0；BF 继续执行 `EXEC` 后面的指令。

| `CMD` | 命令 | 参数 | 说明 |
| --- | --- | --- | --- |
| 0 | NOP | — | 空操作 |
| 1 | `CLEAR` | A=color | 重置显示列表并填充 960×640 画布 |
| 2 | `RECT` | A/B=x16, C/D=y16, E/F=w16, G/H=h16, I=color | 实心矩形 |
| 3 | `RECT_LINES` | 同上 | 矩形描边（3px） |
| 4 | `LINE` | A/B=x1, C/D=y1, E/F=x2, G/H=y2, I=color | 线段（2px） |
| 5 | `CIRCLE` | A/B=x16, C/D=y16, E/F=r16, G=color | 实心圆 |
| 6 | `TEXT` | A/B=x16, C/D=y16, E=color, F=size, G=len | 从文本缓冲区画 ASCII 字符串 |
| 7 | `NUMBER` | A/B=x16, C/D=y16, E=value, F=color, G=size | 画 0..255 的十进制数 |
| 8 | `CELL` | A=gx, B=gy, C=color, D=style | 固定网格上的格子 |
| 9 | `GRID` | A/B=x16, C/D=y16, E=cols, F=rows, G=cell, H=color | 网格背景 + 分隔线 |
| 10 | `YIELD` | — | 暂停，交还宿主一帧 |
| 11 | `LOAD` | A=arena, B=index | 读取堆字节到 `VALUE` |
| 12 | `STORE` | A=arena, B=index, C=value | 写入堆 |
| 13 | `FILL` | A=arena, B=start, C=len, D=value | 填充堆区间 |

`TEXT` 的文本长度最大 64；`len` 超过 64 会被截断。字符串由 BF 提前写进 `TEXT_BUF`（64..127）。

### 4.3 固定格子坐标

`CELL` 是给棋盘游戏准备的快捷绘图命令：

```text
屏幕位置 = (80 + gx * 24, 80 + gy * 24)
格子大小 = 24 × 24
```

`style`：

- `0`：实心方块，带一点顶部高光；
- `1`：圆点（食物）；
- `2`：带两只眼睛的方块（蛇头）；
- `3`：空心方块。

画布逻辑分辨率固定为 **960×640**；窗口缩放时保持比例并居中，周围留黑边。

### 4.4 调色板

| 索引 | 颜色 | 典型用途 |
| --- | --- | --- |
| 0 | 深背景 | 画布背景 |
| 1 | 面板深蓝 | 棋盘/面板 |
| 2 | 网格线 | 网格 |
| 3 | 粉红 | 食物 |
| 4 | 灰蓝 | 边框 |
| 5 | 绿色 | 蛇身 |
| 6 | 亮绿 | 蛇头 |
| 7 | 近白 | 标题/正文 |
| 8 | 灰蓝文字 | 次要文字 |
| 9 | 黄色 | O 块 |
| 10 | 橙色 | J 块 |
| 11 | 蓝色 | I 块 |
| 12 | 紫色 | T 块 |
| 13 | 红色 | Z 块 |
| 14 | 白色 | L 块 |
| 15 | 黑色 | 遮罩 |

### 4.5 堆：按下标访问

标准 BF 只有一条数据指针，按下标访问数组需要运行时移动指针，写起来非常痛苦。引擎提供 32 个 arena × 256 字节的堆：

```text
load(arena, index)          # 结果写入 VALUE
store(arena, index, value)
fill(arena, start, len, value)
```

- `arena`、`index`、`value` 都可以是 BF 单元或常量；
- `arena` 越界会直接报 `ArenaOutOfBounds`；
- Snake 用 arena 0/1 存身体 x/y；
- Tetris 用 arena 0..9 当 10 列，index 是行号 0..19。

这不是“把游戏逻辑搬到 Rust”：碰撞、消行、旋转仍然由 BF 分支/循环决定，堆只是引擎提供的可寻址存储。

---

## 5. VM 细节

`src/vm.rs`：

- 64 KiB tape，单元是 wrapping `u8`；
- 合并连续 `+/-` 和 `>/<`；
- 预计算 `[` / `]` 跳转；
- 指针越界、未知命令、帧预算耗尽都会变成可见错误；
- `.` 是无害 no-op，`,` 读入 0（ABI 不使用标准输入输出，所有 I/O 都走 MMIO）；
- 每条命令执行在写 `EXEC` 的 `+` 指令内同步完成；
- `YIELD` 会立即从 `run_until_yield` 返回，PC 停在 `EXEC` 写入之后。

默认每帧预算 40,000,000 条指令。Tetris 在满棋盘时生成 200 个格子的绘制命令，仍然在这个预算内。若你写自己的复杂游戏，可以用 `--budget` 提高，或优化成脏矩形更新。

错误会显示在画布底部：

```text
ENGINE ERROR
program did not reach a YIELD command within the frame instruction budget
```

---

## 6. 写一个 Brainfuck 游戏

### 6.1 最小纯 BF 程序

理论上你可以直接手写：

- 把 `CMD` 设为 1，参数设颜色，`EXEC=1` 得到背景；
- 把 `CMD` 设为 10，`EXEC=1` 让出一帧；
- 用 `[` / `]` 构成主循环。

但即使画一个矩形也要几十个 `>` 和 `+`，所以项目使用 BF 宏汇编。下面是 `examples/hello.bfa` 的核心：

```text
var run

seti(run, 1)
loop run {
    clear(0)
    draw_rect(64, 64, 832, 512, 1)
    draw_rect_lines(64, 64, 832, 512, 4)
    draw_text(104, 104, 7, 44, "BFGame")
    draw_circle(240, 360, 82, 11)
    draw_rect(390, 286, 130, 148, 5)
    draw_line(580, 286, 830, 434, 9)
    yield()
}
```

编译后 `examples/hello.bf` 只包含 Brainfuck 字符，`bfgame` 可直接运行。

### 6.2 `.bfa` 语法

**变量/数组**

```text
var x
var flag
array table 16      # 当前仅支持静态偏移 table+3，不用于动态下标
```

`bfasm` 从地址 256 开始分配。0..255 是 ABI 保留区。

**宏**

```text
macro inc_twice($cell) {
    inc($cell)
    inc($cell)
}
```

参数以 `$name` 替换；宏可以调用其他宏。宏调用必须是独立一行（`if` 的单行体内请只放内建指令）。

**块**

```text
if flag {
    ...
}

loop count {
    ...
    dec(count)
}
```

- `if cell` 执行一次并把 `cell` 清零（消费条件）；
- `loop cell` 是 BF 的 `[ ... ]`；循环体必须负责把条件清零，否则会无限循环；
- `loop` 不会自动递减；需要计数时自己写 `dec(count)`。

**内建操作**

| 函数 | 作用 |
| --- | --- |
| `seti(cell, n)` | `cell = n` |
| `inc(cell)` / `dec(cell)` | +1 / -1 |
| `copy(dst, src)` | 复制 |
| `move(dst, src)` | 移动（清空源） |
| `addi(cell, n)` / `subi(cell, n)` | 加/减常量 |
| `add(dst, src)` / `sub(dst, src)` | 加/减另一个单元 |
| `eq(dst, a, b)` / `neq(...)` | 相等/不等 |
| `not(dst, src)` | 逻辑非 |
| `and(dst,a,b)` / `or(dst,a,b)` | 逻辑与/或 |
| `at(cell)` | 移动数据指针到指定单元 |
| `raw("+++")` | 原样插入 BF（调试用） |

`eq/and` 等只适合 0..255 的字节；不要指望有符号整数。

**绘图/ABI**

```text
clear(color)
draw_rect(x, y, w, h, color)
draw_rect_lines(x, y, w, h, color)
draw_line(x1, y1, x2, y2, color)
draw_circle(x, y, r, color)
draw_text(x, y, color, size, "ASCII")
draw_number(x, y, value_cell_or_const, color, size)
cell(gx, gy, color_cell_or_const, style)
draw_grid(x, y, cols, rows, cell, color)
yield()
load(arena, index)
store(arena, index, value)
fill(arena, start, len, value)
```

- 位置/大小用**像素常量**；`value`/`gx`/`gy`/`color` 可以是单元；
- `draw_text` 只接受 ASCII，最长 63 字节；
- 编译器会为每条命令设置 `CMD/ARG/EXEC`，你不必手动碰 32..47。

### 6.3 编译与调试

```powershell
python tools\bfasm.py games\src\snake.bfa games\snake.bf
python tools\bfasm.py games\src\tetris.bfa games\tetris.bf
```

生成的是纯 BF，可以用任何标准 Brainfuck 解释器跑（只要它支持 64 KiB tape；不过标准解释器没有游戏引擎的 MMIO，画面不会出现）。

调试模式会在输出中插入 `# L行号: 语句` 注释：

```powershell
python tools\bfasm.py --debug games\src\snake.bfa snake.debug.bf
```

注释不影响 `bfgame`。调试完成后发布纯 `.bf` 即可。

---

## 7. Snake 的实现

源码：`games/src/snake.bfa`（编译产物 `games/snake.bf`，180 KB）。

### 7.1 数据

- 棋盘 16×16，边框是 x/y = 0 或 15，可走 1..14；
- `body_x[]` / `body_y[]` 在 arena 0/1；
- `write_idx` 是环形写指针，`len` 是身体长度；
- 当前尾巴下标 = `write_idx - len`（8 位自然回绕）；
- 蛇头 `head_x/head_y`，食物 `food_x/food_y`，方向 `dir`（1↑ 2→ 3↓ 4←）。

### 7.2 每 tick

1. 根据 `LAST_DIR` 改方向，禁止 180° 反向；
2. 计算 `nx, ny`；
3. 扫描当前有效身体窗口，检查自碰撞；
4. 检查 0/15 边界；
5. 撞了：`alive = 0`，重画 game over；
6. 否则检查食物：
   - 吃到：分数 +1、长度 +1、生成新食物；
   - 没吃到：长度不变；
7. 把新头写入 `write_idx`，递增写指针；
8. 标记 `dirty`，重画整盘。

重画时只画当前身体窗口、食物、蛇头，以及静态 HUD。因为 retained list 会保留到下一次 `CLEAR`，所以没有移动的帧不产生 BF 绘制工作。

### 7.3 食物生成

随机取 `RAND_LO/RAND_HI`，把 0 映射到 1、15 映射到 14，然后：

- 扫描身体判断是否占用；
- 占用就 `food_x++`，到 15 后回到 1、`food_y++`，再扫；
- 空闲就停止。

注意随机坐标只在循环**外**取一次；如果在循环里反复重置成同一个随机位置，食物落在蛇身上时会永远原地打转。

### 7.4 死亡与重开

`R_PRESSED` 设置 `restart`，主循环里执行 `reset()`：

- 清空 arena；
- 重置长度/分数/方向/写指针；
- 重建三段身体 (6,8)(7,8)(8,8)；
- 重新生成食物；
- 下一帧重画。

---

## 8. Tetris 的实现

源码：`games/src/tetris.bfa`（编译产物 `games/tetris.bf`，约 1.7 MB）。

### 8.1 棋盘存储

10 列 × 20 行：

```text
arena 0..9   = 列
index 0..19  = 行
值 0         = 空
值 9..14     = 方块颜色（调色板索引）
```

因此碰撞检测就是：

```text
load(column, row) -> VALUE
```

不需要在 BF tape 上做 `row*10+column` 的动态寻址。

### 8.2 方块表示

每个方块有一个 4×4 包围盒：

```text
origin_x, origin_y
(o0x,o0y) ... (o3x,o3y)   # 相对偏移 0..3
color
type
```

7 种初始形状由 `shape_case()` 宏按 `type` 分支设置，默认是 I。旋转：

```text
顺时针: (x, y) -> (3 - y, x)
```

O 块（type = 1）跳过旋转，否则 2×2 会在 4×4 盒子里平移，看起来像瞬移。

旋转不实现 wall kick：旋转后 4 个格子任意一个越界或占用，就放弃这次旋转。

### 8.3 碰撞

对 4 个格子分别：

1. `ax = origin_x + ox`，`ay = origin_y + oy`；
2. 检查 `ax >= 10`、`ay >= 20`；
3. `load(ax, ay)`，非 0 即碰撞；
4. 任意一个格子碰撞，整体不能移动/旋转。

左移时先把候选 `origin_x` 夹在 0，避免 `-1` 变成 255 导致越界访问。

### 8.4 重力、软降、硬降

- 每 3 个 tick 下降一格（`gravity` 计数）；
- `↓` 立即下降并重置重力计数；
- `Space` 循环下降直到碰撞，然后立即锁定。

锁定：

1. 把 4 个格子写进棋盘（arena=列，index=行）；
2. `clear_lines()`；
3. `spawn_piece()` 出新块；
4. 新块生成位置被占用 → `alive = 0`。

### 8.5 消行

从第 0 行扫到第 19 行：

1. 数当前行 10 列里非 0 的格子；
2. 如果 10 个都满：
   - 对每一列，把 `row` 以上的格子整体下移一格；
   - 顶行补 0；
   - 分数 +10、行数 +1；
   - **不递增 row**，因为下移后同一行可能又是满的；
3. 不满则 `row++`。

测试覆盖单行和双行消除。

### 8.6 绘制

- `draw_grid` 画 10×20 网格；
- 扫描 10×20 棋盘，非空格子用 `cell(gx+7, gy, value, 0)` 绘制；
- 当前方块用 4 个格子叠加绘制；
- HUD：分数、消行数、按键说明；
- game over 时在棋盘中央盖遮罩。

Tetris 只在 `dirty` 时重画；下落/移动/消行/锁定都会置位。

---

## 9. 测试

```powershell
cargo test --offline
```

测试直接实例化 VM 和 `Runtime`，不创建窗口，因此能在 CI/无头环境跑：

- **Snake**：初始化绘制、撞墙 game over、R 重开、随机种子下吃到食物并加分；
- **Tetris**：初始化绘制、左移、旋转、硬降、单行/双行消除、堆叠到 game over、R 重开；
- **VM**：算术循环、跳转表、`LOAD/STORE/FILL`、`EXEC` 自清零；
- **CLI**：帮助和缺少参数。

这些测试会真正执行 `games/*.bf`，不是 mock。

截图冒烟测试：

```powershell
cargo run --release --offline -- snake.bf --frames 3 --screenshot docs\snake.png
cargo run --release --offline -- tetris.bf --frames 3 --screenshot docs\tetris.png
cargo run --release --offline -- examples\hello.bf --frames 3 --screenshot docs\hello.png
```

---

## 10. 性能与预算

- BF 指令数不是“源代码字符数”：`snake.bf` 180 KB，初始化和重画会执行很多 `>`/`<`。
- 每帧预算 40M 条指令，默认 60 FPS；两个游戏在释放构建下都远低于预算。
- Tetris 满盘重画 200 个格子，每个格子要一次 `LOAD` + 一条 `CELL`，是当前最重的绘制路径。
- 如果写更大的游戏：优先减少每帧重画量（脏矩形/只画变化），而不是盲目提高 `--budget`。
- 调试时可以用 `--tick-ms 1` 加速逻辑 tick，以 60 FPS 快速跑完堆叠/消行。

---

## 11. 扩展方向

- 16 位分数：用两个字节做十进制进位；
- 音效：新增 `BEEP` 命令，BF 只写频率/时长；
- next 预览：Tetris 维护 `next_type`，HUD 画 4 个格子；
- 鼠标/手柄：扩展输入寄存器；
- 瓦片贴图/精灵：新增 `SPRITE` 命令；
- 真正的脏矩形：引擎合并绘制命令，BF 只发变化；
- 其他 BF 游戏：Pong、扫雷、Life、推箱子都可以用同一套 ABI。

---

## 12. FAQ

### 这算“游戏逻辑是 Brainfuck”吗？

是。运行时 `bfgame` 只解释 `+-<>[],.`。所有分支、循环、碰撞、分数变化都发生在 `.bf` 的 BF 指令流里。`.bfa` 是开发期的宏汇编，类似汇编器/编译器；最终执行文件是纯 BF。宿主没有 `if snake_hit_wall` 之类的逻辑。

### 为什么提供堆？BF 不是有内存吗？

BF 的 64 KiB tape 是线性的，指针只能顺序移动。按 `row*10+col` 动态寻址在纯 BF 里需要运行时移动指针和保存偏移，代码会难以维护。32×256 的 arena 堆是引擎提供的一块“可寻址 SRAM”，类似硬件外设；`LOAD/STORE` 本身不包含任何游戏规则。

### 为什么 `.bf` 这么大？

BF 没有函数调用，宏全部内联；Tetris 的 7 种形状、碰撞、消行、绘制宏展开后约 1.7 MB 源码。VM 对连续 `+/-`、`>/<` 做了合并，执行效率足够。

### 为什么分数最大 255？

所有单元是 8 位。要支持更大的分数，可以用两个字节（百位/十位/个位或 16 位二进制）并扩展 `NUMBER` 命令。当前 Snake/Tetris 的演示分数不会超过 255。

### 为什么 Tetris 的 O 块不旋转？

简化实现。O 块在 4×4 盒子里旋转会平移，不做特殊处理会“瞬移”；直接跳过旋转更符合玩家预期。

### 可以换成 SDL / wgpu / 自己的渲染器吗？

可以。`DrawCmd` 和 `Runtime` 都不依赖 macroquad；只有 `render.rs` 和 `main.rs` 绑定 macroquad。替换渲染后端只需实现 `DrawCmd` 的绘制。

---

## 13. 文件索引

| 文件 | 说明 |
| --- | --- |
| `src/abi.rs` | 地址常量、命令枚举、palette、`DrawCmd` |
| `src/vm.rs` | BF parser/VM、MMIO、堆、命令执行 |
| `src/game.rs` | 输入、tick、RNG、帧预算、运行时错误 |
| `src/render.rs` | macroquad 绘制与错误层 |
| `src/options.rs` | CLI |
| `src/builtin.rs` | 内置游戏查找 |
| `src/main.rs` | 窗口主循环、截图 |
| `games/src/snake.bfa` | Snake 的 BF 宏汇编源 |
| `games/snake.bf` | Snake 纯 BF 可执行文件 |
| `games/src/tetris.bfa` | Tetris 的 BF 宏汇编源 |
| `games/tetris.bf` | Tetris 纯 BF 可执行文件 |
| `tools/bfasm.py` | `.bfa → .bf` 宏汇编器 |
| `tests/games.rs` | 无窗口游戏逻辑测试 |
| `examples/hello.bfa` / `.bf` | 基础绘图 demo |

---

祝玩得开心。下次有人问“Brainfuck 能写游戏吗？”，把 `snake.bf` 丢给他。
