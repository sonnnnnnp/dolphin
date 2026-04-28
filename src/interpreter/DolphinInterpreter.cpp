#include "DolphinInterpreter.h"
#include <iostream>
#include <string>
#include <cctype>
#include <cmath>

static std::string format_number(double v) {
    if (std::floor(v) == v && v >= -1e15 && v <= 1e15)
        return std::to_string((long long)v);
    std::string s = std::to_string(v);
    s.erase(s.find_last_not_of('0') + 1);
    if (s.back() == '.') s.pop_back();
    return s;
}

DolphinInterpreter::DolphinInterpreter() {
    register_builtins();
    register_graphics_builtins();
}

DolphinInterpreter::~DolphinInterpreter() {
    delete gameWindow;
}

// ---- ブロック読み取り: keyword ( ... ) ----

std::string DolphinInterpreter::read_block(std::istringstream& ss, const std::string& first_line) {
    size_t open  = first_line.find('(');
    size_t close = first_line.rfind(')');
    if (open != std::string::npos && close != std::string::npos && close > open)
        return first_line.substr(open + 1, close - open - 1);

    std::string block;
    int depth = 1;
    std::string line;
    while (std::getline(ss, line)) {
        for (char c : trim(line)) {
            if (c == '(') depth++;
            else if (c == ')') depth--;
        }
        if (depth == 0) break;
        block += line + "\n";
    }
    return block;
}

// ---- メイン実行 ----

void DolphinInterpreter::execute(const std::string& code) {
    std::istringstream ss(code);
    std::string line;

    while (std::getline(ss, line)) {
        line = trim(line);
        if (line.empty() || line.find("//") == 0) continue;

        // # expr — return 文（ローカルスコープ外はエラー）
        if (line[0] == '#') {
            if (local_stack.empty()) {
                std::cerr << "Error: '#' (return) cannot be used outside a function." << std::endl;
                continue;
            }
            throw ReturnException{evaluate_expression(trim(line.substr(1)))};
        }

        // $var = expr — ローカル変数代入
        if (line[0] == '$' && line.find('=') != std::string::npos) {
            if (local_stack.empty()) {
                std::cerr << "Error: '$' variables cannot be used outside a function." << std::endl;
                continue;
            }
            size_t eq       = line.find('=');
            std::string lhs = trim(line.substr(1, eq - 1));
            std::string rhs = trim(line.substr(eq + 1));
            local_stack.back()[lhs] = evaluate_expression(rhs);
            continue;
        }

        // 変数宣言: @var = expr  /  @arr = {1,2,3}  /  @arr[i] = val
        if (line[0] == '@' && line.find('=') != std::string::npos) {
            size_t eq        = line.find('=');
            std::string lhs  = trim(line.substr(1, eq - 1));
            std::string rhs  = trim(line.substr(eq + 1));

            size_t bracket = lhs.find('[');
            if (bracket != std::string::npos) {
                // $arr[idx] = val
                std::string arr_name = lhs.substr(0, bracket);
                std::string idx_expr = lhs.substr(bracket + 1, lhs.rfind(']') - bracket - 1);
                int idx = std::stoi(evaluate_expression(trim(idx_expr)));
                if (arrays.count(arr_name) && idx >= 0 && (size_t)idx < arrays[arr_name].size())
                    arrays[arr_name][idx] = evaluate_expression(rhs);
                else
                    std::cerr << "Error: Array '" << arr_name << "' index " << idx << " out of bounds." << std::endl;
                continue;
            }

            if (!rhs.empty() && rhs[0] == '{') {
                // $arr = {1, 2, 3}
                size_t close = rhs.rfind('}');
                std::string inner = rhs.substr(1, close == std::string::npos ? rhs.size() - 1 : close - 1);
                std::vector<std::string> elements;
                std::istringstream iss(inner);
                std::string elem;
                while (std::getline(iss, elem, ',')) {
                    std::string e = trim(elem);
                    if (!e.empty()) elements.push_back(evaluate_expression(e));
                }
                arrays[lhs] = elements;
                continue;
            }

            declare_variable(lhs, evaluate_expression(rhs));
            continue;
        }

        // ユーザー定義関数: name[$a, $b] (
        if (is_function_def(line)) {
            parse_function_def(ss, line);
            continue;
        }

        // キーワードブロック
        if (line.find('(') != std::string::npos) {
            bool handled = false;
            for (auto& [keyword, handler] : keyword_handlers) {
                if (line.find(keyword) == 0) {
                    handler(read_block(ss, line));
                    handled = true;
                    break;
                }
            }
            if (handled) continue;
        }

        // if / else 文
        if (line.find("if") == 0 && line.find('(') != std::string::npos) {
            size_t paren      = line.find('(');
            std::string cond  = trim(line.substr(2, paren - 2));
            std::string block = read_block(ss, line);
            bool result = evaluate_expression(cond) == "1";
            if (result) execute(block);

            auto saved = ss.tellg();
            std::string peek;
            if (std::getline(ss, peek)) {
                std::string tp = trim(peek);
                if (tp.find("else") == 0 && tp.find('(') != std::string::npos) {
                    std::string else_block = read_block(ss, tp);
                    if (!result) execute(else_block);
                } else {
                    ss.clear();
                    ss.seekg(saved);
                }
            }
            continue;
        }

        // while 文
        if (line.find("while") == 0 && line.find('(') != std::string::npos) {
            size_t paren      = line.find('(');
            std::string cond  = trim(line.substr(5, paren - 5));
            std::string block = read_block(ss, line);
            while (evaluate_expression(cond) == "1")
                execute(block);
            continue;
        }

        // log[テンプレート] ← コンマ分割なし、$/$@ をインライン展開して出力
        if (line.find("log[") == 0 && line.back() == ']') {
            std::cout << interpolate(line.substr(4, line.size() - 5)) << std::endl;
            continue;
        }

        // 関数呼び出し: func[arg1, arg2, ...]
        if (line.find('[') != std::string::npos && line.back() == ']') {
            size_t bracket        = line.find('[');
            std::string func_name = trim(line.substr(0, bracket));
            std::string arg_str   = line.substr(bracket + 1, line.size() - bracket - 2);
            std::vector<std::string> args;
            if (!arg_str.empty()) {
                std::istringstream arg_ss(arg_str);
                std::string item;
                while (std::getline(arg_ss, item, ','))
                    args.push_back(trim(item));
            }
            run_function(func_name, args);
            continue;
        }
    }
}

// ---- 式評価 ----

std::string DolphinInterpreter::evaluate_expression(const std::string& expr) {
    size_t pos;

    // 論理演算（最低優先度から先に分割）
    if ((pos = expr.find("||")) != std::string::npos) {
        bool l = evaluate_expression(trim(expr.substr(0, pos))) == "1";
        bool r = evaluate_expression(trim(expr.substr(pos + 2))) == "1";
        return (l || r) ? "1" : "0";
    }
    if ((pos = expr.find("&&")) != std::string::npos) {
        bool l = evaluate_expression(trim(expr.substr(0, pos))) == "1";
        bool r = evaluate_expression(trim(expr.substr(pos + 2))) == "1";
        return (l && r) ? "1" : "0";
    }

    // 比較演算（2文字演算子を先にチェック）
    using CmpFn = std::function<std::string(std::string, std::string)>;
    for (auto& [op, fn] : std::vector<std::pair<std::string, CmpFn>>{
        {"!=", [](std::string l, std::string r) { try { return std::stod(l) != std::stod(r) ? "1" : "0"; } catch (...) { return l != r ? "1" : "0"; } }},
        {"==", [](std::string l, std::string r) { try { return std::stod(l) == std::stod(r) ? "1" : "0"; } catch (...) { return l == r ? "1" : "0"; } }},
        {">=", [](std::string l, std::string r) { try { return std::stod(l) >= std::stod(r) ? "1" : "0"; } catch (...) { return "0"; } }},
        {"<=", [](std::string l, std::string r) { try { return std::stod(l) <= std::stod(r) ? "1" : "0"; } catch (...) { return "0"; } }},
        {">",  [](std::string l, std::string r) { try { return std::stod(l) >  std::stod(r) ? "1" : "0"; } catch (...) { return "0"; } }},
        {"<",  [](std::string l, std::string r) { try { return std::stod(l) <  std::stod(r) ? "1" : "0"; } catch (...) { return "0"; } }},
    }) {
        if ((pos = expr.find(op)) != std::string::npos) {
            std::string l = evaluate_expression(trim(expr.substr(0, pos)));
            std::string r = evaluate_expression(trim(expr.substr(pos + op.size())));
            return fn(l, r);
        }
    }

    // 四則演算（両辺が空の場合はスキップしてリテラルとして扱う、double で計算）
    if ((pos = expr.find('+')) != std::string::npos) {
        std::string l = trim(expr.substr(0, pos)), r = trim(expr.substr(pos + 1));
        if (!l.empty() && !r.empty())
            return format_number(std::stod(evaluate_expression(l)) + std::stod(evaluate_expression(r)));
    }
    if ((pos = expr.rfind('-')) != std::string::npos && pos > 0) {
        std::string l = trim(expr.substr(0, pos)), r = trim(expr.substr(pos + 1));
        if (!l.empty() && !r.empty())
            return format_number(std::stod(evaluate_expression(l)) - std::stod(evaluate_expression(r)));
    }
    if ((pos = expr.find('*')) != std::string::npos) {
        std::string l = trim(expr.substr(0, pos)), r = trim(expr.substr(pos + 1));
        if (!l.empty() && !r.empty())
            return format_number(std::stod(evaluate_expression(l)) * std::stod(evaluate_expression(r)));
    }
    if ((pos = expr.find('%')) != std::string::npos) {
        std::string l = trim(expr.substr(0, pos)), r = trim(expr.substr(pos + 1));
        if (!l.empty() && !r.empty()) {
            double rv = std::stod(evaluate_expression(r));
            if (rv == 0) { std::cerr << "Error: Modulo by zero." << std::endl; return "0"; }
            return format_number(std::fmod(std::stod(evaluate_expression(l)), rv));
        }
    }
    if ((pos = expr.rfind('/')) != std::string::npos) {
        std::string l = trim(expr.substr(0, pos)), r = trim(expr.substr(pos + 1));
        if (!l.empty() && !r.empty()) {
            double rv = std::stod(evaluate_expression(r));
            if (rv == 0) { std::cerr << "Error: Division by zero." << std::endl; return "0"; }
            return format_number(std::stod(evaluate_expression(l)) / rv);
        }
    }

    // ユーザー定義関数呼び出し: name[arg1, arg2, ...]
    {
        size_t b = expr.find('[');
        if (b != std::string::npos && !expr.empty() && expr.back() == ']') {
            std::string fn_name = trim(expr.substr(0, b));
            bool valid = !fn_name.empty();
            for (char c : fn_name)
                if (!std::isalnum((unsigned char)c) && c != '_') { valid = false; break; }
            if (valid && user_functions.count(fn_name)) {
                std::string arg_str = expr.substr(b + 1, expr.size() - b - 2);
                std::vector<std::string> call_args;
                if (!trim(arg_str).empty()) {
                    std::istringstream as(arg_str);
                    std::string item;
                    while (std::getline(as, item, ','))
                        call_args.push_back(evaluate_expression(trim(item)));
                }
                return call_user_function(fn_name, call_args);
            }
        }
    }

    return resolve_variable(expr);
}

// ---- 変数 / 関数ディスパッチ ----

std::string DolphinInterpreter::resolve_variable(const std::string& name) {
    if (!name.empty() && name[0] == '$') {
        if (local_stack.empty()) {
            std::cerr << "Error: '$' variables cannot be used outside a function." << std::endl;
            return "0";
        }
        std::string key = name.substr(1);
        auto& frame = local_stack.back();
        if (frame.count(key)) return frame[key];
        std::cerr << "Error: Local variable '" << name << "' is not defined." << std::endl;
        return "0";
    }
    if (!name.empty() && name[0] == '@') {
        std::string key = name.substr(1);
        size_t bracket = key.find('[');
        if (bracket != std::string::npos) {
            // $arr[idx] / @arr[idx]
            std::string arr_name = key.substr(0, bracket);
            std::string idx_expr = key.substr(bracket + 1, key.rfind(']') - bracket - 1);
            int idx = std::stoi(evaluate_expression(trim(idx_expr)));
            if (arrays.count(arr_name) && idx >= 0 && (size_t)idx < arrays[arr_name].size())
                return arrays[arr_name][idx];
            std::cerr << "Error: Array '" << arr_name << "' index " << idx << " out of bounds." << std::endl;
            return "0";
        }
        if (variables.count(key)) return variables[key];
        std::cerr << "Error: Variable '" << key << "' is not defined." << std::endl;
        return "0";
    }
    return name;
}

void DolphinInterpreter::declare_variable(const std::string& name, const std::string& value) {
    variables[name] = value;
}

std::vector<std::string> DolphinInterpreter::resolve_variable_array(const std::vector<std::string>& args) {
    std::vector<std::string> resolved;
    for (const auto& a : args)
        resolved.push_back(evaluate_expression(a));
    return resolved;
}

void DolphinInterpreter::run_function(const std::string& name, std::vector<std::string> args) {
    if (functions.count(name)) {
        functions[name](args);
    } else if (user_functions.count(name)) {
        std::vector<std::string> resolved;
        for (auto& a : args) resolved.push_back(evaluate_expression(a));
        call_user_function(name, resolved);
    } else {
        std::cerr << "Error: Function '" << name << "' is not defined." << std::endl;
    }
}

std::string DolphinInterpreter::trim(const std::string& str) {
    size_t s = str.find_first_not_of(" \t\r");
    size_t e = str.find_last_not_of(" \t\r");
    return s == std::string::npos ? "" : str.substr(s, e - s + 1);
}

// ---- 文字列テンプレート展開 ----
// $var / @var (配列アクセス $arr[idx] も対応) を変数値に置換する

std::string DolphinInterpreter::interpolate(const std::string& tmpl) {
    std::string result;
    for (size_t i = 0; i < tmpl.size(); ) {
        char c = tmpl[i];
        if (c == '@' &&
            i + 1 < tmpl.size() &&
            (std::isalpha((unsigned char)tmpl[i + 1]) || tmpl[i + 1] == '_')) {
            size_t start = i++;
            while (i < tmpl.size() && (std::isalnum((unsigned char)tmpl[i]) || tmpl[i] == '_')) i++;
            // 配列アクセス: $arr[idx]
            if (i < tmpl.size() && tmpl[i] == '[') {
                while (i < tmpl.size() && tmpl[i] != ']') i++;
                if (i < tmpl.size()) i++;
            }
            result += resolve_variable(tmpl.substr(start, i - start));
        } else {
            result += tmpl[i++];
        }
    }
    return result;
}

// ---- ユーザー定義関数 ----

bool DolphinInterpreter::is_function_def(const std::string& line) {
    size_t bracket = line.find('[');
    if (bracket == std::string::npos) return false;
    std::string name = trim(line.substr(0, bracket));
    if (name.empty()) return false;
    for (char c : name)
        if (!std::isalnum((unsigned char)c) && c != '_') return false;
    if (functions.count(name) || keyword_handlers.count(name)) return false;
    size_t close = line.find(']', bracket);
    if (close == std::string::npos) return false;
    std::string after = trim(line.substr(close + 1));
    return !after.empty() && after[0] == '(';
}

void DolphinInterpreter::parse_function_def(std::istringstream& ss, const std::string& line) {
    size_t bracket = line.find('[');
    size_t close_b = line.find(']', bracket);
    std::string name       = trim(line.substr(0, bracket));
    std::string param_str  = line.substr(bracket + 1, close_b - bracket - 1);

    std::vector<std::string> params;
    if (!trim(param_str).empty()) {
        std::istringstream ps(param_str);
        std::string p;
        while (std::getline(ps, p, ',')) {
            std::string pt = trim(p);
            if (!pt.empty() && pt[0] == '$')
                params.push_back(pt.substr(1));
        }
    }

    user_functions[name] = {params, read_block(ss, line)};
}

std::string DolphinInterpreter::call_user_function(const std::string& name, const std::vector<std::string>& args) {
    auto& fn = user_functions[name];

    std::unordered_map<std::string, std::string> frame;
    for (size_t i = 0; i < fn.params.size() && i < args.size(); i++)
        frame[fn.params[i]] = args[i];

    local_stack.push_back(std::move(frame));

    std::string ret = "0";
    try {
        execute(fn.body);
    } catch (ReturnException& e) {
        ret = e.value;
    }

    local_stack.pop_back();
    return ret;
}
