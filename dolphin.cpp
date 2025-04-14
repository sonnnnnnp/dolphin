#include <iostream>
#include <sstream>
#include <unordered_map>
#include <functional>
#include <vector>
#include <fstream>
#include <string>

#include <SFML/Graphics.hpp>

using namespace std;

class DolphinInterpreter {
private:
    unordered_map<string, string> variables;
    unordered_map<string, function<void(vector<string>&)>> functions;

    string evaluate_expression(const string& expr) {
        size_t pos = expr.find("+");
        if (pos != string::npos) {
            int left = stoi(resolve_variable(trim(expr.substr(0, pos))));
            int right = stoi(resolve_variable(trim(expr.substr(pos + 1))));
            return to_string(left + right);
        }
        pos = expr.find("-");
        if (pos != string::npos) {
            int left = stoi(resolve_variable(trim(expr.substr(0, pos))));
            int right = stoi(resolve_variable(trim(expr.substr(pos + 1))));
            return to_string(left - right);
        }
        pos = expr.find("*");
        if (pos != string::npos) {
            int left = stoi(resolve_variable(trim(expr.substr(0, pos))));
            int right = stoi(resolve_variable(trim(expr.substr(pos + 1))));
            return to_string(left * right);
        }
        pos = expr.find("/");
        if (pos != string::npos) {
            int left = stoi(resolve_variable(trim(expr.substr(0, pos))));
            int right = stoi(resolve_variable(trim(expr.substr(pos + 1))));
            if (right == 0) {
                cerr << "Error: Division by zero." << endl;
                return "";
            }
            return to_string(left / right);
        }
        if (expr.find(">") != string::npos) {
            auto pos = expr.find(">");
            int left = stoi(resolve_variable(trim(expr.substr(0, pos))));
            int right = stoi(resolve_variable(trim(expr.substr(pos + 1))));
            return (left > right) ? "1" : "0";
        }
        if (expr.find("<") != string::npos) {
            auto pos = expr.find("<");
            int left = stoi(resolve_variable(trim(expr.substr(0, pos))));
            int right = stoi(resolve_variable(trim(expr.substr(pos + 1))));
            return (left < right) ? "1" : "0";
        }
        if (expr.find("==") != string::npos) {
            auto pos = expr.find("==");
            int left = stoi(resolve_variable(trim(expr.substr(0, pos))));
            int right = stoi(resolve_variable(trim(expr.substr(pos + 2))));
            return (left == right) ? "1" : "0";
        }
        if (expr.find("!=") != string::npos) {
            auto pos = expr.find("!=");
            int left = stoi(resolve_variable(trim(expr.substr(0, pos))));
            int right = stoi(resolve_variable(trim(expr.substr(pos + 2))));
            return (left != right) ? "1" : "0";
        }
        // <, ==, != とかも後々追加するンゴ
        return resolve_variable(expr);
    }

public:
    DolphinInterpreter() {
        // 組み込み関数 log[]
        functions["log"] = [this](vector<string>& args) {
            args = resolve_variable_array(args);
            if (args.empty()) {
                return;
            }
            cout << args[0];
            for (size_t i = 1; i < args.size(); ++i) {
                cout << " " << args[i];
            }
            cout << endl;
        };
        // 組み込み関数 input[]
        functions["input"] = [this](vector<string>& args) {
            if (args.empty()) {
                cerr << "Error: input function requires at least 1 argument." << endl;
                return;
            }
            string input;
            getline(cin, input);
            // 変数に格納
            declare_variable(args[0].substr(1), input);
        };
        // 組み込み関数 window[]
        functions["window"] = [this](vector<string>& args) {
            args = resolve_variable_array(args);
            if (args.size() < 2) {
                cerr << "Error: draw function requires at least 2 arguments." << endl;
                return;
            }
            // 引数を取得
            int width = stoi(args[0]);
            int height = stoi(args[1]);
            string title = (args.size() > 2) ? args[2] : "";
            // SFMLウィンドウの作成
            sf::RenderWindow window(sf::VideoMode(width, height), title);

            // 描画ループ
            while (window.isOpen()) {
                sf::Event event;
                while (window.pollEvent(event)) {
                    if (event.type == sf::Event::Closed)
                        window.close();
                }
                window.clear();
                // window.draw(rectangle);
                window.display();
            }
        };
    }
    string trim(const string& str) {
        size_t start = str.find_first_not_of(" \t");
        size_t end = str.find_last_not_of(" \t");
        return (start == string::npos) ? "" : str.substr(start, (end - start + 1));
    }
    void declare_variable(const string& name, const string& value) {
        variables[name] = value;
    }
    // void declare_function(const string& code) {}
    string resolve_variable(const string& var_name) {
        // 変数名が '@' で始まる場合、変数として解決
        if (var_name[0] == '@') {
            string name = var_name.substr(1);
            if (variables.count(name)) {
                return variables[name];
            } else {
                cerr << "Error: Variable '" << name << "' is not defined." << endl;
                return "";
            }
        }
        // 変数名が '@' で始まらない場合、文字列として返す
        return var_name;
    }
    vector<string> resolve_variable_array(const vector<string>& var_names) {
        vector<string> resolved_args;
        for (auto& arg : var_names) {
            resolved_args.push_back(evaluate_expression(arg));
        }
        return resolved_args;
    }
    void run_function(const string& name, vector<string> args) {
        // 関数が存在するか確認
        if (functions.find(name) != functions.end()) {
            functions[name](args);
        } else {
            cerr << "Error: Function '" << name << "' is not defined." << endl;
        }
    }
    void execute(const string& code) {
        stringstream ss(code);
        string line;
        
        while (getline(ss, line)) {
            line = trim(line); // 行の前後の空白を削除
            if (line.empty()) continue; // 空行をスキップ
            if (line.find("//") == 0) continue; // コメントアウトをスキップ

            // 変数宣言 (例: @a = 10)
            if (line.find("@") == 0 && line.find("=") != string::npos) {
                size_t pos = line.find("=");
                string var_name = trim(line.substr(1, pos - 1));
                string var_value = trim(line.substr(pos + 1)); // '='の後ろを取得
                var_value = evaluate_expression(var_value); // 変数の演算を解決
                declare_variable(var_name, var_value);
            }

            // 関数呼び出し (例: log(@a))
            if (line.find("[") != string::npos && line.back() == ']') {
                size_t name_end = line.find("[");
                string func_name = line.substr(0, name_end);
                string arg_str = line.substr(name_end + 1, line.size() - name_end - 2); // 括弧内
                // 関数名の前後の空白を削除
                func_name = trim(func_name);
                // 引数無しの場合
                if (arg_str.empty()) {
                    run_function(func_name, {});
                    continue;
                }
                // 引数をカンマ区切りで分割
                vector<string> raw_args;
                stringstream arg_ss(arg_str);
                string item;
                while (getline(arg_ss, item, ',')) {
                    raw_args.push_back(trim(item));
                }

                run_function(func_name, raw_args);
            }
            // 条件分岐 (例: if (@a > 10) { log(@a); })
            if (line.find("if") == 0 && line.find("{") != string::npos) {
                // 条件とブロックの抽出
                size_t cond_start = line.find("if") + 2;
                size_t brace_pos = line.find("{");
            
                string condition = trim(line.substr(cond_start, brace_pos - cond_start));
                string block_line;
                string block = "";
            
                // ブレース内の複数行読み込み（対応: { ... } が別行でもOK）
                if (line.find("}") == string::npos) {
                    while (getline(ss, block_line)) {
                        if (block_line.find("}") != string::npos) break;
                        block += block_line + "\n";
                    }
                } else {
                    // 単一行に } まである場合
                    block = line.substr(brace_pos + 1, line.find("}") - brace_pos - 1);
                }
            
                // 条件を評価
                string cond_result = evaluate_expression(condition);
                if (cond_result == "1" || cond_result == "true") {
                    execute(block); // 再帰的にブロック実行
                }
                continue;
            }
        }
    }
};

int main(int argc, char** argv) {
    DolphinInterpreter dolphin;
    
    if (argc > 1) {
        // ファイル名が指定されている場合
        string filename = argv[1];
        ifstream file(filename);
        if (!file.is_open()) {
            cerr << "ファイルを開けませんでした: " << filename << endl;
            return 1;
        }
        stringstream buffer;
        buffer << file.rdbuf(); // ファイルの内容を読み込む
        dolphin.execute(buffer.str());
    } else {
        // 引数がない場合、REPLモードで対話的に入力を受ける
        cout << "Dolphin REPL モード。終了するには 'exit' を入力してください。" << endl;
        string line;
        while (true) {
            cout << "> ";
            getline(cin, line);
            if (line == "exit") break;
            dolphin.execute(line);
        }
    }
    
    return 0;
}