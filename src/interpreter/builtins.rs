use super::{trim, Interpreter};

pub fn is_builtin(name: &str) -> bool {
    matches!(name, "input" | "arr_len" | "arr_set" | "arr_push" | "str_concat" | "str_len")
}

pub fn call_builtin_sync(interp: &mut Interpreter, name: &str, args: Vec<String>) {
    match name {
        "input" => builtin_input(interp, &args),
        "arr_len" => builtin_arr_len(interp, &args),
        "arr_set" => builtin_arr_set(interp, &args),
        "arr_push" => builtin_arr_push(interp, &args),
        "str_concat" => builtin_str_concat(interp, &args),
        "str_len" => builtin_str_len(interp, &args),
        _ => {}
    }
}

fn strip_sigil(s: &str) -> &str {
    s.trim_start_matches('@').trim_start_matches('$')
}

// input[@var]
pub fn builtin_input(interp: &mut Interpreter, args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: input requires 1 argument.");
        return;
    }
    let mut val = String::new();
    std::io::stdin().read_line(&mut val).ok();
    let val = val.trim_end_matches('\n').trim_end_matches('\r').to_string();
    interp.set_var(strip_sigil(trim(&args[0])), val);
}

// arr_len[arr_name, @result]
pub fn builtin_arr_len(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 {
        eprintln!("Error: arr_len requires arr_name and var.");
        return;
    }
    let arr_name = trim(&args[0]).to_string();
    let var_name = strip_sigil(trim(&args[1])).to_string();
    let len = interp.arrays.get(&arr_name).map_or(0, |a| a.len());
    interp.set_var(&var_name, len.to_string());
}

// arr_set[arr_name, index, value]
pub fn builtin_arr_set(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 3 {
        eprintln!("Error: arr_set requires arr_name, index, value.");
        return;
    }
    let arr_name = trim(&args[0]).to_string();
    let idx = interp.eval(trim(&args[1])).parse::<usize>().unwrap_or(0);
    let val = interp.eval(trim(&args[2]));
    if let Some(arr) = interp.arrays.get_mut(&arr_name) {
        if idx < arr.len() {
            arr[idx] = val;
        } else {
            eprintln!("Error: arr_set '{}' index {} out of bounds.", arr_name, idx);
        }
    }
}

// arr_push[arr_name, value]
pub fn builtin_arr_push(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 {
        eprintln!("Error: arr_push requires arr_name and value.");
        return;
    }
    let arr_name = trim(&args[0]).to_string();
    let val = interp.eval(trim(&args[1]));
    interp.arrays.entry(arr_name).or_default().push(val);
}

// str_concat[@result, val1, val2]
pub fn builtin_str_concat(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 3 {
        eprintln!("Error: str_concat requires result val1 val2.");
        return;
    }
    let var_name = strip_sigil(trim(&args[0])).to_string();
    let a = interp.resolve_var(trim(&args[1]));
    let b = interp.resolve_var(trim(&args[2]));
    interp.set_var(&var_name, a + &b);
}

// str_len[str_or_var, @result]
pub fn builtin_str_len(interp: &mut Interpreter, args: &[String]) {
    if args.len() < 2 {
        eprintln!("Error: str_len requires str and result_var.");
        return;
    }
    let s = interp.resolve_var(trim(&args[0]));
    let var_name = strip_sigil(trim(&args[1])).to_string();
    interp.set_var(&var_name, s.len().to_string());
}
