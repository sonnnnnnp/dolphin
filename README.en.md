# 🐬 Dolphin

[日本語](README.md)

A small interpreted language written in C++, where variables are declared with `@`. Source files use the `.dol` extension.

## About

I am building my own interpreted language to understand how interpreted languages work. Since I am making it myself anyway, I gave it a unique syntax.

I am building it for learning, for the romance of it, and for my own satisfaction. It is not meant for practical use, but the goal is for it to be good enough that you could actually use it, just for the fun of it. The goal is a language you can also use to build backend APIs and desktop apps.

```
@name = Dolphin
log[Hello, @name!]
add[$a, $b] (
    # $a + $b
)
@i = 1
while @i <= 3 (
    log[count: @i]
    @i = add[@i, 1]
)
```

Output:

```
Hello, Dolphin!
count: 1
count: 2
count: 3
```

## Why `@`?

<!-- TODO -->

## Features

- [x] Declaring, assigning, and referencing variables with `@` (numbers and strings)
- [x] Arithmetic and modulo (`+` `-` `*` `/` `%`)
- [x] Comparison (`==` `!=` `>` `<` `>=` `<=`) and logical operators (`&&` `||`)
- [x] `if` / `else`
- [x] `while`
- [x] Arrays (`@nums = {10, 20, 30}`, read and assign with `@nums[0]`)
- [x] Output with `log[...]` (expands `@variables` inside the text)
- [x] Standard input with `input[@var]`
- [x] User-defined functions (`$` for parameters and local variables, `#` to return a value)
- [x] Built-in array and string functions (`arr_len` `arr_set` `arr_push` `str_concat` `str_len`)
- [x] Window rendering with SFML (rectangles, circles, images, text, keyboard input, mouse input, sound)
- [x] `//` comments at the start of a line
- [x] REPL mode when started without arguments
- [ ] Grouping with parentheses in expressions (e.g. `(@a + @b) * 2`)
- [ ] End-of-line comments
- [ ] Line numbers in error messages
- [ ] HTTP server features for building backend APIs

The full list of built-in functions and syntax details are in [docs/README.md on `archive/cpp`](https://github.com/sonnnnnnp/dolphin/blob/archive/cpp/docs/README.md) (Japanese).

## Architecture

```
Source → Lexer → Parser → AST → Evaluator
```

| Stage | Role |
|---|---|
| Lexer | Splits the source string into tokens |
| Parser | Parses the token stream |
| AST | Represents statements and expressions as a tree |
| Evaluator | Evaluates the AST by walking the tree |

The initial implementation (`archive/cpp`) does not build an AST. It reads the source line by line and evaluates expressions by splitting them as strings. The main files are:

| File | Contents |
|---|---|
| `src/main.cpp` | Entry point for running files and the REPL |
| `src/interpreter/DolphinInterpreter.cpp` | Statement execution, expression evaluation, user-defined functions |
| `src/interpreter/Builtins.cpp` | Built-ins for I/O, arrays, and strings |
| `src/interpreter/GraphicsBuiltins.cpp` | Built-ins for rendering, input, and sound using SFML |

## Build and Run

Requirements: CMake 3.16 or later and a C++17 compiler. SFML 2.6.2 is downloaded automatically during the build.

```sh
git switch archive/cpp
cmake -S . -B build
cmake --build build
```

```sh
# Run a script
./build/dolphin my_scripts/test.dol

# Start the REPL with no arguments (type exit to quit)
./build/dolphin
```

With a Visual Studio generator, the executable is written to `build/Debug/dolphin.exe`.

## Branches

| Branch | Contents |
|---|---|
| `main` | Implementation being rebuilt |
| [`archive/cpp`](https://github.com/sonnnnnnp/dolphin/tree/archive/cpp) | Initial implementation (C++17 + SFML). Sample scripts are in `my_scripts/` |
| [`archive/rust`](https://github.com/sonnnnnnp/dolphin/tree/archive/rust) | Port of the initial implementation to Rust + macroquad |
