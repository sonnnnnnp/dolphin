#include "DolphinInterpreter.h"
#include <iostream>
#include <string>

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

        // 変数宣言: @var = expr  /  @arr = {1,2,3}  /  @arr[i] = val
        if (line[0] == '@' && line.find('=') != std::string::npos) {
            size_t eq        = line.find('=');
            std::string lhs  = trim(line.substr(1, eq - 1));
            std::string rhs  = trim(line.substr(eq + 1));

            size_t bracket = lhs.find('[');
            if (bracket != std::string::npos) {
                // @arr[idx] = val
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
                // @arr = {1, 2, 3}
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

        // if 文
        if (line.find("if") == 0 && line.find('(') != std::string::npos) {
            size_t paren     = line.find('(');
            std::string cond  = trim(line.substr(2, paren - 2));
            std::string block = read_block(ss, line);
            if (evaluate_expression(cond) == "1")
                execute(block);
            continue;
        }

        // while 文
        if (line.find("while") == 0 && line.find('(') != std::string::npos) {
            size_t paren     = line.find('(');
            std::string cond  = trim(line.substr(5, paren - 5));
            std::string block = read_block(ss, line);
            while (evaluate_expression(cond) == "1")
                execute(block);
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
        {"!=", [](std::string l, std::string r) { try { return std::stoi(l) != std::stoi(r) ? "1" : "0"; } catch (...) { return l != r ? "1" : "0"; } }},
        {"==", [](std::string l, std::string r) { try { return std::stoi(l) == std::stoi(r) ? "1" : "0"; } catch (...) { return l == r ? "1" : "0"; } }},
        {">=", [](std::string l, std::string r) { return std::stoi(l) >= std::stoi(r) ? "1" : "0"; }},
        {"<=", [](std::string l, std::string r) { return std::stoi(l) <= std::stoi(r) ? "1" : "0"; }},
        {">",  [](std::string l, std::string r) { return std::stoi(l) >  std::stoi(r) ? "1" : "0"; }},
        {"<",  [](std::string l, std::string r) { return std::stoi(l) <  std::stoi(r) ? "1" : "0"; }},
    }) {
        if ((pos = expr.find(op)) != std::string::npos) {
            std::string l = evaluate_expression(trim(expr.substr(0, pos)));
            std::string r = evaluate_expression(trim(expr.substr(pos + op.size())));
            return fn(l, r);
        }
    }

    // 四則演算
    if ((pos = expr.find('+')) != std::string::npos)
        return std::to_string(std::stoi(evaluate_expression(trim(expr.substr(0, pos)))) +
                              std::stoi(evaluate_expression(trim(expr.substr(pos + 1)))));
    if ((pos = expr.rfind('-')) != std::string::npos && pos > 0)
        return std::to_string(std::stoi(evaluate_expression(trim(expr.substr(0, pos)))) -
                              std::stoi(evaluate_expression(trim(expr.substr(pos + 1)))));
    if ((pos = expr.find('*')) != std::string::npos)
        return std::to_string(std::stoi(evaluate_expression(trim(expr.substr(0, pos)))) *
                              std::stoi(evaluate_expression(trim(expr.substr(pos + 1)))));
    if ((pos = expr.rfind('/')) != std::string::npos) {
        int r = std::stoi(evaluate_expression(trim(expr.substr(pos + 1))));
        if (r == 0) { std::cerr << "Error: Division by zero." << std::endl; return "0"; }
        return std::to_string(std::stoi(evaluate_expression(trim(expr.substr(0, pos)))) / r);
    }

    return resolve_variable(expr);
}

// ---- 変数 / 関数ディスパッチ ----

std::string DolphinInterpreter::resolve_variable(const std::string& name) {
    if (!name.empty() && name[0] == '@') {
        std::string key = name.substr(1);
        size_t bracket = key.find('[');
        if (bracket != std::string::npos) {
            // @arr[idx]
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
    if (functions.count(name))
        functions[name](args);
    else
        std::cerr << "Error: Function '" << name << "' is not defined." << std::endl;
}

std::string DolphinInterpreter::trim(const std::string& str) {
    size_t s = str.find_first_not_of(" \t\r");
    size_t e = str.find_last_not_of(" \t\r");
    return s == std::string::npos ? "" : str.substr(s, e - s + 1);
}
