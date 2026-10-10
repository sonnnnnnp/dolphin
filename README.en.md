# 🐬 Dolphin

[日本語](README.md)

A small interpreted language written in Rust, where variables are declared with `@`. Source files use the `.dol` extension.

## About

I am building my own interpreted language to understand how interpreted languages work. Since I am making it myself anyway, I gave it a unique syntax.

I am building it for learning, for the romance of it, and for my own satisfaction. It is not meant for practical use, but the goal is for it to be good enough that you could actually use it, just for the fun of it. The goal is a language you can also use to build backend APIs and desktop apps.

```
@name = "Dolphin"
log["Hello, @name!"]

add[$a, $b] (
    # $a + $b
)

@i = 1
while @i <= 3 (
    log["count: @i"]
    @i = add[@i, 1]
)

@user = {"name": @name, "langs": {"Rust", "C++"}}
log[@user]
```

Output:

```
Hello, Dolphin!
count: 1
count: 2
count: 3
{"name": "Dolphin", "langs": {"Rust", "C++"}}
```

## Why `@`?

<!-- TODO -->

## Syntax at a glance

| Syntax | Meaning |
|---|---|
| `@x = 1` | Global variable |
| `$x = 1` | Local variable (inside functions only) |
| `name[a, b]` | Function call |
| `name[$a, $b] ( … )` | Function definition |
| `# value` | Return a value from a function |
| `if cond ( … )` / `else ( … )` / `else if` | Conditionals |
| `while cond ( … )` / `break` / `continue` | Loops |
| `{1, 2, 3}` / `@a[0]` | Arrays |
| `{"key": value}` / `@m["key"]` / `{:}` | Maps (`{:}` is an empty map) |
| `"Hi, @name"` / `"$x"` | Strings. Variables inside are expanded |
| `// comment` | Comment to end of line |

The full specification is in [docs/design.md](docs/design.md) (Japanese), and the list of built-in functions is in section 5.5 of that document.

## Features

The stages correspond to section 2.1 of [docs/design.md](docs/design.md).

- [x] **Stage 1: Core language**
  - Variables (`@` global / `$` local), arithmetic, modulo, comparison, logical operators (short-circuit), unary `-` `!`, grouping with `( )`
  - `if` / `else` / `else if`, `while`, `break` / `continue`
  - Functions (callable before their definition, recursion, `#` to return)
  - Arrays, expansion of `@x` and `$x` inside strings, end-of-line comments
- [x] **Stage 2: Practical use**
  - Error messages with line, column, and call trace
  - Maps (associative arrays)
  - Built-in functions for strings, number conversion, and math
  - Interactive mode (REPL)
- [ ] **Stage 3**: Multiple files (import), file I/O
- [ ] **Stage 4**: HTTP server, JSON → backend APIs
- [ ] **Stage 5**: Windows, drawing, input → desktop apps (already supported with SFML in the old C++ implementation)

Errors point to where they happened:

```
runtime_error.dol:5:12: エラー: 添字 5 は範囲外です（要素は 2 個）
  |
5 |     # $arr[5]
  |            ^
  = inner の中（2:7 から呼び出し）
  = outer の中（8:1 から呼び出し）
```

(Error messages are currently in Japanese.)

## Architecture

```
Source → Lexer → Parser → AST → Resolver → Interpreter
```

| Stage | Role |
|---|---|
| Lexer | Splits the source string into tokens |
| Parser | Parses the tokens with recursive descent and builds the AST (syntax tree) |
| Resolver | Checks for undefined functions, wrong argument counts, misplaced variables, and so on before running |
| Interpreter | Evaluates the AST by walking the tree |

The overall design is described in [docs/architecture.md](docs/architecture.md) (Japanese). The core language has no external crate dependencies.

## Build and Run

Requirements: [Rust](https://rustup.rs) (stable)

```sh
# Run a script
cargo run -- examples/hello.dol

# Start interactive mode with no arguments (type exit to quit)
cargo run

# Inspect intermediate results
cargo run -- --tokens examples/hello.dol   # tokens
cargo run -- --ast examples/hello.dol      # syntax tree (S-expressions)

# Tests
cargo test
```

`cargo build --release` writes the executable to `target/release/dolphin` (`dolphin.exe` on Windows).

## Branches

| Branch | Contents |
|---|---|
| `main` | The rebuilt implementation (Rust) |
| [`archive/cpp`](https://github.com/sonnnnnnp/dolphin/tree/archive/cpp) | Initial implementation (C++17 + SFML). Sample scripts are in `my_scripts/` |
| [`archive/rust`](https://github.com/sonnnnnnp/dolphin/tree/archive/rust) | Port of the initial implementation to Rust + macroquad |
