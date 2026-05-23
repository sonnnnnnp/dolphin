use std::collections::HashMap;
use async_recursion::async_recursion;

pub mod builtins;
pub mod graphics;

// ---- ユーティリティ ----

pub fn trim(s: &str) -> &str {
    s.trim_matches(|c| c == ' ' || c == '\t' || c == '\r')
}

fn strip_comment(s: &str) -> &str {
    match s.find("//") {
        Some(pos) => trim(&s[..pos]),
        None => s,
    }
}

// カンマ区切りの引数を分割（ブラケット内のカンマは無視）
pub fn split_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '[' => { depth += 1; current.push(c); }
            ']' => { depth -= 1; current.push(c); }
            ',' if depth == 0 => {
                let t = current.trim().to_string();
                if !t.is_empty() { args.push(t); }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    let t = current.trim().to_string();
    if !t.is_empty() { args.push(t); }
    args
}

fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v >= -1e15 && v <= 1e15 {
        return (v as i64).to_string();
    }
    let s = format!("{}", v);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

// ブラケット外で演算子を探す
// rfind=true で右から、skip_unary=true で単項マイナスをスキップ
fn find_op(expr: &[u8], op: &[u8], rfind: bool, skip_unary: bool) -> Option<usize> {
    let n = expr.len();
    let m = op.len();
    if m == 0 || n < m { return None; }
    let mut depth: i32 = 0;

    if rfind {
        let mut i = (n - m) as isize;
        while i >= 0 {
            let ui = i as usize;
            match expr[ui] {
                b']' => depth += 1,
                b'[' => depth -= 1,
                _ => {}
            }
            if depth == 0 && &expr[ui..ui + m] == op {
                if skip_unary {
                    let mut j = i - 1;
                    while j >= 0 && matches!(expr[j as usize], b' ' | b'\t') {
                        j -= 1;
                    }
                    if j < 0 {
                        i -= 1;
                        continue;
                    }
                    if matches!(expr[j as usize], b'+' | b'-' | b'*' | b'/' | b'%') {
                        i -= 1;
                        continue;
                    }
                }
                return Some(ui);
            }
            i -= 1;
        }
    } else {
        let mut i = 0usize;
        while i + m <= n {
            match expr[i] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            if depth == 0 && &expr[i..i + m] == op {
                return Some(i);
            }
            i += 1;
        }
    }
    None
}

// 複数行コードからブロックを読み取る
// first_line に (content) が全部あれば inline、なければ lines[start..] から読む
pub fn read_block(lines: &[String], start: usize, first_line: &str) -> (String, usize) {
    if let Some(open) = first_line.find('(') {
        if let Some(close) = first_line.rfind(')') {
            if close > open {
                return (first_line[open + 1..close].to_string(), start);
            }
        }
    }

    let mut block = String::new();
    let mut depth = 1i32;
    let mut i = start;
    while i < lines.len() {
        let l = trim(&lines[i]);
        for c in l.chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
        }
        if depth == 0 {
            i += 1;
            break;
        }
        block.push_str(&lines[i]);
        block.push('\n');
        i += 1;
    }
    (block, i)
}

// ---- インタープリター型 ----

#[derive(Clone)]
pub struct UserFunction {
    pub params: Vec<String>,
    pub body: String,
}

pub struct Interpreter {
    pub variables: HashMap<String, String>,
    pub arrays: HashMap<String, Vec<String>>,
    pub user_functions: HashMap<String, UserFunction>,
    pub local_stack: Vec<HashMap<String, String>>,

    // グラフィクス状態
    pub window_open: bool,
    pub bg: [u8; 3],
    pub camera_x: f32,
    pub camera_y: f32,

    // 永続シェイプ（IDで管理）
    pub shapes: Vec<(String, graphics::RectData)>,
    pub shape_index: HashMap<String, usize>,
    pub circles: Vec<(String, graphics::CircleData)>,
    pub circle_index: HashMap<String, usize>,
    pub texts: Vec<(String, graphics::TextData)>,
    pub text_index: HashMap<String, usize>,

    pub sprites: HashMap<String, graphics::SpriteEntry>,
    pub fonts: HashMap<String, macroquad::text::Font>,
    pub sounds: HashMap<String, macroquad::audio::Sound>,

    // フレームごとの描画キュー
    pub sprite_queue: Vec<graphics::SpriteDrawCmd>,
    pub rect_queue: Vec<graphics::RectDrawCmd>,
    pub mouse_clicked: bool,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            arrays: HashMap::new(),
            user_functions: HashMap::new(),
            local_stack: Vec::new(),
            window_open: false,
            bg: [0, 0, 0],
            camera_x: 0.0,
            camera_y: 0.0,
            shapes: Vec::new(),
            shape_index: HashMap::new(),
            circles: Vec::new(),
            circle_index: HashMap::new(),
            texts: Vec::new(),
            text_index: HashMap::new(),
            sprites: HashMap::new(),
            fonts: HashMap::new(),
            sounds: HashMap::new(),
            sprite_queue: Vec::new(),
            rect_queue: Vec::new(),
            mouse_clicked: false,
        }
    }

    pub async fn run(&mut self, code: &str) {
        self.exec(code).await;
    }

    // ---- メイン実行ループ（async、再帰可） ----

    #[async_recursion(?Send)]
    pub async fn exec(&mut self, code: &str) {
        let lines: Vec<String> = code.lines().map(String::from).collect();
        let mut i = 0;

        while i < lines.len() {
            let raw = lines[i].clone();
            i += 1;

            let line = trim(&raw);
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }

            // # expr — return（ユーザー定義関数外では無効）
            if line.starts_with('#') {
                eprintln!("Error: '#' (return) cannot be used outside a function.");
                continue;
            }

            // $var = expr — ローカル変数
            if line.starts_with('$') && line.contains('=') {
                if self.local_stack.is_empty() {
                    eprintln!("Error: '$' variables cannot be used outside a function.");
                    continue;
                }
                let eq = line.find('=').unwrap();
                let lhs = trim(&line[1..eq]).to_string();
                let rhs = trim(&line[eq + 1..]);
                let val = self.eval(rhs);
                self.local_stack.last_mut().unwrap().insert(lhs, val);
                continue;
            }

            // @var = expr / @arr = {..} / @arr[i] = val
            if line.starts_with('@') && line.contains('=') {
                let eq = line.find('=').unwrap();
                let lhs = trim(&line[1..eq]);
                let rhs = trim(&line[eq + 1..]);

                // @arr[idx] = val
                if let Some(bracket) = lhs.find('[') {
                    let arr_name = lhs[..bracket].to_string();
                    let close = lhs.rfind(']').unwrap_or(lhs.len());
                    let idx_str = trim(&lhs[bracket + 1..close]);
                    let idx = self.eval(idx_str).parse::<i64>().unwrap_or(0) as usize;
                    let val = self.eval(rhs);
                    if let Some(arr) = self.arrays.get_mut(&arr_name) {
                        if idx < arr.len() {
                            arr[idx] = val;
                        } else {
                            eprintln!("Error: Array '{}' index {} out of bounds.", arr_name, idx);
                        }
                    }
                    continue;
                }

                // @arr = {1, 2, 3}
                if rhs.starts_with('{') {
                    let close = rhs.rfind('}').unwrap_or(rhs.len());
                    let inner = &rhs[1..close];
                    let elements: Vec<String> = inner
                        .split(',')
                        .map(|e| self.eval(e.trim()))
                        .collect();
                    self.arrays.insert(lhs.to_string(), elements);
                    continue;
                }

                let val = self.eval(rhs);
                self.variables.insert(lhs.to_string(), val);
                continue;
            }

            // ユーザー定義関数: name[$a, $b] (
            if self.is_func_def(line) {
                let bracket = line.find('[').unwrap();
                let close_b = line.find(']').unwrap();
                let name = trim(&line[..bracket]).to_string();
                let param_str = &line[bracket + 1..close_b];
                let params: Vec<String> = if param_str.trim().is_empty() {
                    vec![]
                } else {
                    param_str
                        .split(',')
                        .map(|p| trim(p).trim_start_matches('$').to_string())
                        .filter(|p| !p.is_empty())
                        .collect()
                };
                let (body, new_i) = read_block(&lines, i, line);
                i = new_i;
                self.user_functions.insert(name, UserFunction { params, body });
                continue;
            }

            // gameloop ( ... )
            if line.starts_with("gameloop") && line.contains('(') {
                let (body, new_i) = read_block(&lines, i, line);
                i = new_i;
                graphics::run_gameloop(self, &body).await;
                continue;
            }

            // if 文
            if line.starts_with("if")
                && !line.starts_with("input")
                && line.contains('(')
            {
                let paren = line.find('(').unwrap();
                let cond = trim(&line[2..paren]).to_string();
                let (body, new_i) = read_block(&lines, i, line);
                i = new_i;
                let result = self.eval(&cond) == "1";
                if result {
                    self.exec(&body).await;
                }
                // else チェック
                if i < lines.len() {
                    let next_raw = &lines[i];
                    let next = strip_comment(trim(next_raw));
                    if next.starts_with("else") && next.contains('(') {
                        i += 1;
                        let (else_body, new_i) = read_block(&lines, i, next);
                        i = new_i;
                        if !result {
                            self.exec(&else_body).await;
                        }
                    }
                }
                continue;
            }

            // while 文
            if line.starts_with("while") && line.contains('(') {
                let paren = line.find('(').unwrap();
                let cond = trim(&line[5..paren]).to_string();
                let (body, new_i) = read_block(&lines, i, line);
                i = new_i;
                loop {
                    if self.eval(&cond) != "1" {
                        break;
                    }
                    self.exec(&body).await;
                }
                continue;
            }

            // log[テンプレート]
            if line.starts_with("log[") && line.ends_with(']') {
                let tmpl = &line[4..line.len() - 1];
                println!("{}", self.interpolate(tmpl));
                continue;
            }

            // 関数呼び出し: name[arg1, arg2, ...]
            if line.contains('[') && line.ends_with(']') {
                let bracket = line.find('[').unwrap();
                let name = trim(&line[..bracket]).to_string();
                let arg_str = &line[bracket + 1..line.len() - 1];
                let args: Vec<String> = if arg_str.trim().is_empty() {
                    vec![]
                } else {
                    split_args(arg_str)
                };
                self.call_builtin(&name, args).await;
                continue;
            }
        }
    }

    // ---- 式評価（同期）----

    pub fn eval(&mut self, expr: &str) -> String {
        let expr = expr.trim();

        // 論理演算
        if let Some(pos) = find_op(expr.as_bytes(), b"||", false, false) {
            let l = self.eval(expr[..pos].trim()) == "1";
            let r = self.eval(expr[pos + 2..].trim()) == "1";
            return if l || r { "1" } else { "0" }.to_string();
        }
        if let Some(pos) = find_op(expr.as_bytes(), b"&&", false, false) {
            let l = self.eval(expr[..pos].trim()) == "1";
            let r = self.eval(expr[pos + 2..].trim()) == "1";
            return if l && r { "1" } else { "0" }.to_string();
        }

        // 比較演算（2文字を先にチェック）
        for (op, len) in &[
            ("!=", 2usize), ("==", 2), (">=", 2), ("<=", 2), (">", 1), ("<", 1),
        ] {
            if let Some(pos) = find_op(expr.as_bytes(), op.as_bytes(), false, false) {
                let l = self.eval(expr[..pos].trim());
                let r = self.eval(expr[pos + len..].trim());
                let result = match *op {
                    "!=" => l.parse::<f64>().ok().zip(r.parse::<f64>().ok())
                        .map_or(l != r, |(a, b)| a != b),
                    "==" => l.parse::<f64>().ok().zip(r.parse::<f64>().ok())
                        .map_or(l == r, |(a, b)| a == b),
                    ">=" => l.parse::<f64>().unwrap_or(0.0) >= r.parse::<f64>().unwrap_or(0.0),
                    "<=" => l.parse::<f64>().unwrap_or(0.0) <= r.parse::<f64>().unwrap_or(0.0),
                    ">" => l.parse::<f64>().unwrap_or(0.0) > r.parse::<f64>().unwrap_or(0.0),
                    "<" => l.parse::<f64>().unwrap_or(0.0) < r.parse::<f64>().unwrap_or(0.0),
                    _ => false,
                };
                return if result { "1" } else { "0" }.to_string();
            }
        }

        // 四則演算
        if let Some(pos) = find_op(expr.as_bytes(), b"+", false, false) {
            let l = expr[..pos].trim();
            let r = expr[pos + 1..].trim();
            if !l.is_empty() && !r.is_empty() {
                let lv = self.eval(l).parse::<f64>().unwrap_or(0.0);
                let rv = self.eval(r).parse::<f64>().unwrap_or(0.0);
                return format_number(lv + rv);
            }
        }
        if let Some(pos) = find_op(expr.as_bytes(), b"-", true, true) {
            if pos > 0 {
                let l = expr[..pos].trim();
                let r = expr[pos + 1..].trim();
                if !l.is_empty() && !r.is_empty() {
                    let lv = self.eval(l).parse::<f64>().unwrap_or(0.0);
                    let rv = self.eval(r).parse::<f64>().unwrap_or(0.0);
                    return format_number(lv - rv);
                }
            }
        }
        if let Some(pos) = find_op(expr.as_bytes(), b"*", false, false) {
            let l = expr[..pos].trim();
            let r = expr[pos + 1..].trim();
            if !l.is_empty() && !r.is_empty() {
                let lv = self.eval(l).parse::<f64>().unwrap_or(0.0);
                let rv = self.eval(r).parse::<f64>().unwrap_or(0.0);
                return format_number(lv * rv);
            }
        }
        if let Some(pos) = find_op(expr.as_bytes(), b"%", false, false) {
            let l = expr[..pos].trim();
            let r = expr[pos + 1..].trim();
            if !l.is_empty() && !r.is_empty() {
                let rv = self.eval(r).parse::<f64>().unwrap_or(0.0);
                if rv == 0.0 {
                    eprintln!("Error: Modulo by zero.");
                    return "0".to_string();
                }
                let lv = self.eval(l).parse::<f64>().unwrap_or(0.0);
                return format_number(lv % rv);
            }
        }
        if let Some(pos) = find_op(expr.as_bytes(), b"/", true, false) {
            let l = expr[..pos].trim();
            let r = expr[pos + 1..].trim();
            if !l.is_empty() && !r.is_empty() {
                let rv = self.eval(r).parse::<f64>().unwrap_or(0.0);
                if rv == 0.0 {
                    eprintln!("Error: Division by zero.");
                    return "0".to_string();
                }
                let lv = self.eval(l).parse::<f64>().unwrap_or(0.0);
                return format_number(lv / rv);
            }
        }

        // ユーザー定義関数呼び出し: name[args]
        if let Some(bracket) = expr.find('[') {
            if expr.ends_with(']') {
                let name = trim(&expr[..bracket]);
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    if self.user_functions.contains_key(name) {
                        let arg_str = &expr[bracket + 1..expr.len() - 1];
                        let args: Vec<String> = if arg_str.trim().is_empty() {
                            vec![]
                        } else {
                            split_args(arg_str)
                                .into_iter()
                                .map(|a| self.eval(&a))
                                .collect()
                        };
                        return self.call_user_func(name, &args);
                    }
                }
            }
        }

        self.resolve_var(expr)
    }

    // ---- 変数解決 ----

    pub fn resolve_var(&self, name: &str) -> String {
        if name.starts_with('$') {
            let key = &name[1..];
            if let Some(frame) = self.local_stack.last() {
                if let Some(v) = frame.get(key) {
                    return v.clone();
                }
            }
            eprintln!("Error: Local variable '{}' is not defined.", name);
            return "0".to_string();
        }
        if name.starts_with('@') {
            let key = &name[1..];
            if let Some(bracket) = key.find('[') {
                let arr_name = &key[..bracket];
                let close = key.rfind(']').unwrap_or(key.len());
                let idx_str = trim(&key[bracket + 1..close]);
                // インデックスはリテラルか単純な変数参照のみ対応
                let idx = idx_str.parse::<i64>().unwrap_or_else(|_| {
                    let var_name = idx_str.trim_start_matches('@').trim_start_matches('$');
                    self.variables
                        .get(var_name)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0)
                }) as usize;
                if let Some(arr) = self.arrays.get(arr_name) {
                    if idx < arr.len() {
                        return arr[idx].clone();
                    }
                    eprintln!("Error: Array '{}' index {} out of bounds.", arr_name, idx);
                    return "0".to_string();
                }
                eprintln!("Error: Variable '{}' is not defined.", key);
                return "0".to_string();
            }
            return self.variables.get(key).cloned().unwrap_or_else(|| {
                eprintln!("Error: Variable '{}' is not defined.", key);
                "0".to_string()
            });
        }
        name.to_string()
    }

    pub fn set_var(&mut self, name: &str, value: String) {
        self.variables.insert(name.to_string(), value);
    }

    // ---- 文字列テンプレート展開 ----

    pub fn interpolate(&self, tmpl: &str) -> String {
        let mut result = String::new();
        let chars: Vec<char> = tmpl.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let is_var = (c == '@' || c == '$')
                && i + 1 < chars.len()
                && (chars[i + 1].is_alphabetic() || chars[i + 1] == '_');
            if is_var {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                // 配列アクセス @arr[idx]
                if i < chars.len() && chars[i] == '[' {
                    while i < chars.len() && chars[i] != ']' {
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 1;
                    }
                }
                let name: String = chars[start..i].iter().collect();
                result.push_str(&self.resolve_var(&name));
            } else {
                result.push(c);
                i += 1;
            }
        }
        result
    }

    // ---- ユーザー定義関数 ----

    fn is_func_def(&self, line: &str) -> bool {
        let Some(bracket) = line.find('[') else { return false; };
        let name = trim(&line[..bracket]);
        if name.is_empty() { return false; }
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_') { return false; }
        // 組み込みと同名の場合は関数定義ではない
        if builtins::is_builtin(name) || graphics::is_graphics_builtin(name) { return false; }
        let Some(close_b) = line.find(']') else { return false; };
        let after = trim(&line[close_b + 1..]);
        !after.is_empty() && after.starts_with('(')
    }

    // 同期的なユーザー関数呼び出し（eval 内から利用）
    pub fn call_user_func(&mut self, name: &str, args: &[String]) -> String {
        let func = match self.user_functions.get(name).cloned() {
            Some(f) => f,
            None => return "0".to_string(),
        };

        let mut frame = HashMap::new();
        for (i, param) in func.params.iter().enumerate() {
            if let Some(val) = args.get(i) {
                frame.insert(param.clone(), val.clone());
            }
        }
        self.local_stack.push(frame);
        let ret = self.exec_func_sync(&func.body);
        self.local_stack.pop();
        ret.unwrap_or_else(|| "0".to_string())
    }

    // 同期版の関数ボディ実行（ユーザー定義関数用）
    // Some(v) = return 文で返した値、None = return なし
    fn exec_func_sync(&mut self, code: &str) -> Option<String> {
        let lines: Vec<String> = code.lines().map(String::from).collect();
        let mut i = 0;

        while i < lines.len() {
            let raw = lines[i].clone();
            i += 1;
            let line = trim(&raw);
            if line.is_empty() || line.starts_with("//") { continue; }
            let line = strip_comment(line).trim();
            if line.is_empty() { continue; }

            // return: # expr
            if line.starts_with('#') {
                return Some(self.eval(line[1..].trim()));
            }

            // $var = expr
            if line.starts_with('$') && line.contains('=') {
                if let Some(eq) = line.find('=') {
                    let lhs = trim(&line[1..eq]).to_string();
                    let rhs = trim(&line[eq + 1..]);
                    let val = self.eval(rhs);
                    if let Some(frame) = self.local_stack.last_mut() {
                        frame.insert(lhs, val);
                    }
                }
                continue;
            }

            // @var = expr
            if line.starts_with('@') && line.contains('=') {
                let eq = line.find('=').unwrap();
                let lhs = trim(&line[1..eq]);
                let rhs = trim(&line[eq + 1..]);
                if let Some(bracket) = lhs.find('[') {
                    let arr_name = lhs[..bracket].to_string();
                    let close = lhs.rfind(']').unwrap_or(lhs.len());
                    let idx_str = trim(&lhs[bracket + 1..close]);
                    let idx = self.eval(idx_str).parse::<usize>().unwrap_or(0);
                    let val = self.eval(rhs);
                    if let Some(arr) = self.arrays.get_mut(&arr_name) {
                        if idx < arr.len() { arr[idx] = val; }
                    }
                    continue;
                }
                if rhs.starts_with('{') {
                    let close = rhs.rfind('}').unwrap_or(rhs.len());
                    let elements: Vec<String> = rhs[1..close]
                        .split(',')
                        .map(|e| self.eval(e.trim()))
                        .collect();
                    self.arrays.insert(lhs.to_string(), elements);
                    continue;
                }
                let val = self.eval(rhs);
                self.variables.insert(lhs.to_string(), val);
                continue;
            }

            // if
            if line.starts_with("if") && !line.starts_with("input") && line.contains('(') {
                let paren = line.find('(').unwrap();
                let cond = trim(&line[2..paren]).to_string();
                let (body, new_i) = read_block(&lines, i, line);
                i = new_i;
                let result = self.eval(&cond) == "1";
                if result {
                    if let Some(v) = self.exec_func_sync(&body) { return Some(v); }
                }
                if i < lines.len() {
                    let next = strip_comment(trim(&lines[i]));
                    if next.starts_with("else") && next.contains('(') {
                        i += 1;
                        let (else_body, new_i) = read_block(&lines, i, next);
                        i = new_i;
                        if !result {
                            if let Some(v) = self.exec_func_sync(&else_body) { return Some(v); }
                        }
                    }
                }
                continue;
            }

            // while
            if line.starts_with("while") && line.contains('(') {
                let paren = line.find('(').unwrap();
                let cond = trim(&line[5..paren]).to_string();
                let (body, new_i) = read_block(&lines, i, line);
                i = new_i;
                loop {
                    if self.eval(&cond) != "1" { break; }
                    if let Some(v) = self.exec_func_sync(&body) { return Some(v); }
                }
                continue;
            }

            // log
            if line.starts_with("log[") && line.ends_with(']') {
                let tmpl = &line[4..line.len() - 1];
                println!("{}", self.interpolate(tmpl));
                continue;
            }

            // 関数呼び出し
            if line.contains('[') && line.ends_with(']') {
                let bracket = line.find('[').unwrap();
                let name = trim(&line[..bracket]).to_string();
                let arg_str = &line[bracket + 1..line.len() - 1];
                let args: Vec<String> = if arg_str.trim().is_empty() {
                    vec![]
                } else {
                    split_args(arg_str)
                };
                if builtins::is_builtin(&name) {
                    builtins::call_builtin_sync(self, &name, args);
                } else if self.user_functions.contains_key(&name) {
                    let resolved: Vec<String> = args.iter().map(|a| self.eval(a)).collect();
                    self.call_user_func(&name, &resolved);
                } else {
                    eprintln!("Error: Function '{}' is not defined.", name);
                }
            }
        }

        None
    }

    // ---- ディスパッチ ----

    pub fn eval_args(&mut self, args: &[String]) -> Vec<String> {
        args.iter().map(|a| self.eval(a)).collect()
    }

    pub async fn call_builtin(&mut self, name: &str, args: Vec<String>) {
        if builtins::is_builtin(name) {
            builtins::call_builtin_sync(self, name, args);
        } else if graphics::is_graphics_builtin(name) {
            graphics::call_graphics_builtin(self, name, args).await;
        } else if self.user_functions.contains_key(name) {
            let resolved = self.eval_args(&args);
            self.call_user_func(name, &resolved);
        } else {
            eprintln!("Error: Function '{}' is not defined.", name);
        }
    }
}
