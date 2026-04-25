#include "DolphinInterpreter.h"
#include <iostream>
#include <sstream>
#include <string>

using namespace std;

// ---- キー名変換 ----

sf::Keyboard::Key DolphinInterpreter::str_to_key(const string& name) {
    if (name == "Left")  return sf::Keyboard::Left;
    if (name == "Right") return sf::Keyboard::Right;
    if (name == "Up")    return sf::Keyboard::Up;
    if (name == "Down")  return sf::Keyboard::Down;
    if (name == "Space") return sf::Keyboard::Space;
    if (name == "Enter") return sf::Keyboard::Return;
    if (name == "Z")     return sf::Keyboard::Z;
    if (name == "X")     return sf::Keyboard::X;
    if (name == "A")     return sf::Keyboard::A;
    if (name == "D")     return sf::Keyboard::D;
    if (name == "W")     return sf::Keyboard::W;
    if (name == "S")     return sf::Keyboard::S;
    return sf::Keyboard::Unknown;
}

// ---- コンストラクタ / デストラクタ ----

DolphinInterpreter::DolphinInterpreter() {
    // log[arg1, arg2, ...]
    functions["log"] = [this](vector<string>& args) {
        args = resolve_variable_array(args);
        if (args.empty()) return;
        cout << args[0];
        for (size_t i = 1; i < args.size(); ++i)
            cout << " " << args[i];
        cout << endl;
    };

    // input[@var]
    functions["input"] = [this](vector<string>& args) {
        if (args.empty()) { cerr << "Error: input requires 1 argument." << endl; return; }
        string input;
        getline(cin, input);
        declare_variable(args[0].substr(1), input);
    };

    // window[width, height, title?] — ノンブロッキング
    functions["window"] = [this](vector<string>& args) {
        args = resolve_variable_array(args);
        if (args.size() < 2) { cerr << "Error: window requires width and height." << endl; return; }
        int width  = stoi(args[0]);
        int height = stoi(args[1]);
        string title = (args.size() > 2) ? args[2] : "dolphin";
        delete gameWindow;
        gameWindow = new sf::RenderWindow(sf::VideoMode(width, height), title);
        gameWindow->setFramerateLimit(60);
    };

    // rect_create[id, x, y, w, h, r, g, b]
    functions["rect_create"] = [this](vector<string>& args) {
        if (args.size() < 8) { cerr << "Error: rect_create requires id x y w h r g b." << endl; return; }
        string id = args[0];
        vector<string> nums(args.begin() + 1, args.end());
        nums = resolve_variable_array(nums);

        sf::RectangleShape rect(sf::Vector2f(stof(nums[2]), stof(nums[3])));
        rect.setPosition(stof(nums[0]), stof(nums[1]));
        rect.setFillColor(sf::Color(stoi(nums[4]), stoi(nums[5]), stoi(nums[6])));

        if (shape_index.count(id)) {
            shape_list[shape_index[id]].second = rect;
        } else {
            shape_index[id] = shape_list.size();
            shape_list.push_back({id, rect});
        }
    };

    // rect_set[id, x, y]
    functions["rect_set"] = [this](vector<string>& args) {
        if (args.size() < 3) { cerr << "Error: rect_set requires id x y." << endl; return; }
        string id = args[0];
        vector<string> nums(args.begin() + 1, args.end());
        nums = resolve_variable_array(nums);
        if (!shape_index.count(id)) { cerr << "Error: rect '" << id << "' not found." << endl; return; }
        shape_list[shape_index[id]].second.setPosition(stof(nums[0]), stof(nums[1]));
    };

    // key_check[KeyName, @var]
    functions["key_check"] = [this](vector<string>& args) {
        if (args.size() < 2) { cerr << "Error: key_check requires key_name and @var." << endl; return; }
        string key_name = resolve_variable(trim(args[0]));
        string var_name = trim(args[1]).substr(1);
        declare_variable(var_name, sf::Keyboard::isKeyPressed(str_to_key(key_name)) ? "1" : "0");
    };

    // bg[r, g, b] — ウィンドウのクリアカラー
    functions["bg"] = [this](vector<string>& args) {
        args = resolve_variable_array(args);
        if (args.size() < 3) return;
        clearColor = sf::Color(stoi(args[0]), stoi(args[1]), stoi(args[2]));
    };
}

DolphinInterpreter::~DolphinInterpreter() {
    delete gameWindow;
}

// ---- ブロック読み取り ((...) を対応する ) まで収集) ----

string DolphinInterpreter::read_block(istringstream& ss, const string& first_line) {
    string block;
    // 1行に ( ) が両方ある場合
    size_t open  = first_line.find('(');
    size_t close = first_line.rfind(')');
    if (open != string::npos && close != string::npos && close > open)
        return first_line.substr(open + 1, close - open - 1);

    // 複数行ブロック
    int depth = 1;
    string line;
    while (getline(ss, line)) {
        string t = trim(line);
        for (char c : t) {
            if (c == '(') depth++;
            if (c == ')') { if (--depth == 0) goto done; }
        }
        block += line + "\n";
    }
done:
    return block;
}

// ---- メイン実行 ----

void DolphinInterpreter::execute(const string& code) {
    istringstream ss(code);
    string line;

    while (getline(ss, line)) {
        line = trim(line);
        if (line.empty() || line.find("//") == 0) continue;

        // 変数宣言: @var = expr
        if (line[0] == '@' && line.find('=') != string::npos) {
            size_t pos     = line.find('=');
            string var_name  = trim(line.substr(1, pos - 1));
            string var_value = trim(line.substr(pos + 1));
            declare_variable(var_name, evaluate_expression(var_value));
            continue;
        }

        // gameloop ( ... )
        if (line.find("gameloop") == 0 && line.find('(') != string::npos) {
            string block = read_block(ss, line);
            if (!gameWindow) { cerr << "Error: call window[] before gameloop." << endl; continue; }
            while (gameWindow->isOpen()) {
                sf::Event event;
                while (gameWindow->pollEvent(event))
                    if (event.type == sf::Event::Closed) gameWindow->close();
                if (!gameWindow->isOpen()) break;
                execute(block);
                gameWindow->clear(clearColor);
                for (auto& [id, shape] : shape_list)
                    gameWindow->draw(shape);
                gameWindow->display();
            }
            continue;
        }

        // if文: if cond ( ... )
        if (line.find("if") == 0 && line.find('(') != string::npos) {
            size_t brace = line.find('(');
            string cond  = trim(line.substr(2, brace - 2));
            string block = read_block(ss, line);
            if (evaluate_expression(cond) == "1")
                execute(block);
            continue;
        }

        // 関数呼び出し: func[arg1, arg2, ...]
        if (line.find('[') != string::npos && line.back() == ']') {
            size_t name_end = line.find('[');
            string func_name = trim(line.substr(0, name_end));
            string arg_str   = line.substr(name_end + 1, line.size() - name_end - 2);
            if (arg_str.empty()) {
                run_function(func_name, {});
            } else {
                vector<string> raw_args;
                istringstream arg_ss(arg_str);
                string item;
                while (getline(arg_ss, item, ','))
                    raw_args.push_back(trim(item));
                run_function(func_name, raw_args);
            }
            continue;
        }
    }
}

// ---- 式評価 ----

string DolphinInterpreter::evaluate_expression(const string& expr) {
    // 論理 OR
    size_t pos = expr.find("||");
    if (pos != string::npos) {
        bool l = (evaluate_expression(trim(expr.substr(0, pos))) == "1");
        bool r = (evaluate_expression(trim(expr.substr(pos + 2))) == "1");
        return (l || r) ? "1" : "0";
    }
    // 論理 AND
    pos = expr.find("&&");
    if (pos != string::npos) {
        bool l = (evaluate_expression(trim(expr.substr(0, pos))) == "1");
        bool r = (evaluate_expression(trim(expr.substr(pos + 2))) == "1");
        return (l && r) ? "1" : "0";
    }
    // != と ==（先に 2文字演算子を確認）
    pos = expr.find("!=");
    if (pos != string::npos) {
        string l = evaluate_expression(trim(expr.substr(0, pos)));
        string r = evaluate_expression(trim(expr.substr(pos + 2)));
        try { return (stoi(l) != stoi(r)) ? "1" : "0"; } catch (...) { return (l != r) ? "1" : "0"; }
    }
    pos = expr.find("==");
    if (pos != string::npos) {
        string l = evaluate_expression(trim(expr.substr(0, pos)));
        string r = evaluate_expression(trim(expr.substr(pos + 2)));
        try { return (stoi(l) == stoi(r)) ? "1" : "0"; } catch (...) { return (l == r) ? "1" : "0"; }
    }
    // >= と <=
    pos = expr.find(">=");
    if (pos != string::npos) {
        int l = stoi(evaluate_expression(trim(expr.substr(0, pos))));
        int r = stoi(evaluate_expression(trim(expr.substr(pos + 2))));
        return (l >= r) ? "1" : "0";
    }
    pos = expr.find("<=");
    if (pos != string::npos) {
        int l = stoi(evaluate_expression(trim(expr.substr(0, pos))));
        int r = stoi(evaluate_expression(trim(expr.substr(pos + 2))));
        return (l <= r) ? "1" : "0";
    }
    // > と <
    pos = expr.find('>');
    if (pos != string::npos) {
        int l = stoi(evaluate_expression(trim(expr.substr(0, pos))));
        int r = stoi(evaluate_expression(trim(expr.substr(pos + 1))));
        return (l > r) ? "1" : "0";
    }
    pos = expr.find('<');
    if (pos != string::npos) {
        int l = stoi(evaluate_expression(trim(expr.substr(0, pos))));
        int r = stoi(evaluate_expression(trim(expr.substr(pos + 1))));
        return (l < r) ? "1" : "0";
    }
    // 四則演算
    pos = expr.find('+');
    if (pos != string::npos) {
        int l = stoi(evaluate_expression(trim(expr.substr(0, pos))));
        int r = stoi(evaluate_expression(trim(expr.substr(pos + 1))));
        return to_string(l + r);
    }
    // - は単項マイナスと区別するため pos > 0 のみ
    pos = expr.rfind('-');
    if (pos != string::npos && pos > 0) {
        int l = stoi(evaluate_expression(trim(expr.substr(0, pos))));
        int r = stoi(evaluate_expression(trim(expr.substr(pos + 1))));
        return to_string(l - r);
    }
    pos = expr.find('*');
    if (pos != string::npos) {
        int l = stoi(evaluate_expression(trim(expr.substr(0, pos))));
        int r = stoi(evaluate_expression(trim(expr.substr(pos + 1))));
        return to_string(l * r);
    }
    pos = expr.rfind('/');
    if (pos != string::npos) {
        int l = stoi(evaluate_expression(trim(expr.substr(0, pos))));
        int r = stoi(evaluate_expression(trim(expr.substr(pos + 1))));
        if (r == 0) { cerr << "Error: Division by zero." << endl; return "0"; }
        return to_string(l / r);
    }
    return resolve_variable(expr);
}

// ---- ユーティリティ ----

string DolphinInterpreter::resolve_variable(const string& var_name) {
    if (!var_name.empty() && var_name[0] == '@') {
        string name = var_name.substr(1);
        if (variables.count(name)) return variables[name];
        cerr << "Error: Variable '" << name << "' is not defined." << endl;
        return "0";
    }
    return var_name;
}

string DolphinInterpreter::trim(const string& str) {
    size_t start = str.find_first_not_of(" \t\r");
    size_t end   = str.find_last_not_of(" \t\r");
    return (start == string::npos) ? "" : str.substr(start, end - start + 1);
}

void DolphinInterpreter::declare_variable(const string& name, const string& value) {
    variables[name] = value;
}

vector<string> DolphinInterpreter::resolve_variable_array(const vector<string>& var_names) {
    vector<string> resolved;
    for (const auto& v : var_names)
        resolved.push_back(evaluate_expression(v));
    return resolved;
}

void DolphinInterpreter::run_function(const string& name, vector<string> args) {
    if (functions.count(name)) {
        functions[name](args);
    } else {
        cerr << "Error: Function '" << name << "' is not defined." << endl;
    }
}
