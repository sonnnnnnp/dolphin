#include "DolphinInterpreter.h"
#include <iostream>
#include <string>

using namespace std;

DolphinInterpreter::DolphinInterpreter() {
    register_builtins();
    register_graphics_builtins();
}

DolphinInterpreter::~DolphinInterpreter() {
    delete gameWindow;
}

// ---- ブロック読み取り: keyword ( ... ) ----
// 対応する ) まで読み込んでブロック文字列を返す。goto を使わずに depth で管理。

string DolphinInterpreter::read_block(istringstream& ss, const string& first_line) {
    size_t open  = first_line.find('(');
    size_t close = first_line.rfind(')');
    if (open != string::npos && close != string::npos && close > open)
        return first_line.substr(open + 1, close - open - 1);

    string block;
    int depth = 1;
    string line;
    while (getline(ss, line)) {
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

void DolphinInterpreter::execute(const string& code) {
    istringstream ss(code);
    string line;

    while (getline(ss, line)) {
        line = trim(line);
        if (line.empty() || line.find("//") == 0) continue;

        // 変数宣言: @var = expr  /  @arr = {1,2,3}  /  @arr[i] = val
        if (line[0] == '@' && line.find('=') != string::npos) {
            size_t eq  = line.find('=');
            string lhs = trim(line.substr(1, eq - 1));
            string rhs = trim(line.substr(eq + 1));

            size_t bracket = lhs.find('[');
            if (bracket != string::npos) {
                // @arr[idx] = val
                string arr_name = lhs.substr(0, bracket);
                string idx_expr = lhs.substr(bracket + 1, lhs.rfind(']') - bracket - 1);
                int idx = stoi(evaluate_expression(trim(idx_expr)));
                if (arrays.count(arr_name) && idx >= 0 && (size_t)idx < arrays[arr_name].size())
                    arrays[arr_name][idx] = evaluate_expression(rhs);
                else
                    cerr << "Error: Array '" << arr_name << "' index " << idx << " out of bounds." << endl;
                continue;
            }

            if (!rhs.empty() && rhs[0] == '{') {
                // @arr = {1, 2, 3}
                size_t close = rhs.rfind('}');
                string inner = rhs.substr(1, close == string::npos ? rhs.size() - 1 : close - 1);
                vector<string> elements;
                istringstream iss(inner);
                string elem;
                while (getline(iss, elem, ',')) {
                    string e = trim(elem);
                    if (!e.empty()) elements.push_back(evaluate_expression(e));
                }
                arrays[lhs] = elements;
                continue;
            }

            declare_variable(lhs, evaluate_expression(rhs));
            continue;
        }

        // キーワードブロック: keyword ( ... )  ← keyword_handlers に登録されたものを処理
        if (line.find('(') != string::npos) {
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
        if (line.find("if") == 0 && line.find('(') != string::npos) {
            size_t paren = line.find('(');
            string cond  = trim(line.substr(2, paren - 2));
            string block = read_block(ss, line);
            if (evaluate_expression(cond) == "1")
                execute(block);
            continue;
        }

        // while 文
        if (line.find("while") == 0 && line.find('(') != string::npos) {
            size_t paren = line.find('(');
            string cond  = trim(line.substr(5, paren - 5));
            string block = read_block(ss, line);
            while (evaluate_expression(cond) == "1")
                execute(block);
            continue;
        }

        // 関数呼び出し: func[arg1, arg2, ...]
        if (line.find('[') != string::npos && line.back() == ']') {
            size_t bracket   = line.find('[');
            string func_name = trim(line.substr(0, bracket));
            string arg_str   = line.substr(bracket + 1, line.size() - bracket - 2);
            vector<string> args;
            if (!arg_str.empty()) {
                istringstream arg_ss(arg_str);
                string item;
                while (getline(arg_ss, item, ','))
                    args.push_back(trim(item));
            }
            run_function(func_name, args);
            continue;
        }
    }
}

// ---- 式評価 ----

string DolphinInterpreter::evaluate_expression(const string& expr) {
    size_t pos;

    // 論理演算（最低優先度から先に分割）
    if ((pos = expr.find("||")) != string::npos) {
        bool l = evaluate_expression(trim(expr.substr(0, pos))) == "1";
        bool r = evaluate_expression(trim(expr.substr(pos + 2))) == "1";
        return (l || r) ? "1" : "0";
    }
    if ((pos = expr.find("&&")) != string::npos) {
        bool l = evaluate_expression(trim(expr.substr(0, pos))) == "1";
        bool r = evaluate_expression(trim(expr.substr(pos + 2))) == "1";
        return (l && r) ? "1" : "0";
    }

    // 比較演算（2文字演算子を先にチェック）
    for (auto& [op, fn] : (vector<pair<string, function<string(string, string)>>>{
        {"!=", [](string l, string r) { try { return stoi(l) != stoi(r) ? "1" : "0"; } catch (...) { return l != r ? "1" : "0"; } }},
        {"==", [](string l, string r) { try { return stoi(l) == stoi(r) ? "1" : "0"; } catch (...) { return l == r ? "1" : "0"; } }},
        {">=", [](string l, string r) { return stoi(l) >= stoi(r) ? "1" : "0"; }},
        {"<=", [](string l, string r) { return stoi(l) <= stoi(r) ? "1" : "0"; }},
        {">",  [](string l, string r) { return stoi(l) >  stoi(r) ? "1" : "0"; }},
        {"<",  [](string l, string r) { return stoi(l) <  stoi(r) ? "1" : "0"; }},
    })) {
        if ((pos = expr.find(op)) != string::npos) {
            string l = evaluate_expression(trim(expr.substr(0, pos)));
            string r = evaluate_expression(trim(expr.substr(pos + op.size())));
            return fn(l, r);
        }
    }

    // 四則演算
    if ((pos = expr.find('+')) != string::npos)
        return to_string(stoi(evaluate_expression(trim(expr.substr(0, pos)))) +
                         stoi(evaluate_expression(trim(expr.substr(pos + 1)))));
    if ((pos = expr.rfind('-')) != string::npos && pos > 0)
        return to_string(stoi(evaluate_expression(trim(expr.substr(0, pos)))) -
                         stoi(evaluate_expression(trim(expr.substr(pos + 1)))));
    if ((pos = expr.find('*')) != string::npos)
        return to_string(stoi(evaluate_expression(trim(expr.substr(0, pos)))) *
                         stoi(evaluate_expression(trim(expr.substr(pos + 1)))));
    if ((pos = expr.rfind('/')) != string::npos) {
        int r = stoi(evaluate_expression(trim(expr.substr(pos + 1))));
        if (r == 0) { cerr << "Error: Division by zero." << endl; return "0"; }
        return to_string(stoi(evaluate_expression(trim(expr.substr(0, pos)))) / r);
    }

    return resolve_variable(expr);
}

// ---- 変数 / 関数ディスパッチ ----

string DolphinInterpreter::resolve_variable(const string& name) {
    if (!name.empty() && name[0] == '@') {
        string key = name.substr(1);
        size_t bracket = key.find('[');
        if (bracket != string::npos) {
            // @arr[idx]
            string arr_name = key.substr(0, bracket);
            string idx_expr = key.substr(bracket + 1, key.rfind(']') - bracket - 1);
            int idx = stoi(evaluate_expression(trim(idx_expr)));
            if (arrays.count(arr_name) && idx >= 0 && (size_t)idx < arrays[arr_name].size())
                return arrays[arr_name][idx];
            cerr << "Error: Array '" << arr_name << "' index " << idx << " out of bounds." << endl;
            return "0";
        }
        if (variables.count(key)) return variables[key];
        cerr << "Error: Variable '" << key << "' is not defined." << endl;
        return "0";
    }
    return name;
}

void DolphinInterpreter::declare_variable(const string& name, const string& value) {
    variables[name] = value;
}

vector<string> DolphinInterpreter::resolve_variable_array(const vector<string>& args) {
    vector<string> resolved;
    for (const auto& a : args)
        resolved.push_back(evaluate_expression(a));
    return resolved;
}

void DolphinInterpreter::run_function(const string& name, vector<string> args) {
    if (functions.count(name))
        functions[name](args);
    else
        cerr << "Error: Function '" << name << "' is not defined." << endl;
}

string DolphinInterpreter::trim(const string& str) {
    size_t s = str.find_first_not_of(" \t\r");
    size_t e = str.find_last_not_of(" \t\r");
    return s == string::npos ? "" : str.substr(s, e - s + 1);
}
