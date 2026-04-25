#include "DolphinInterpreter.h"
#include <iostream>

// シジル ($/@) を除いた変数名を返す
static std::string strip_sigil(const std::string& s) {
    if (!s.empty() && (s[0] == '@' || s[0] == '$')) return s.substr(1);
    return s;
}

void DolphinInterpreter::register_builtins() {
    // input[$var] / input[@var]
    functions["input"] = [this](std::vector<std::string>& args) {
        if (args.empty()) { std::cerr << "Error: input requires 1 argument." << std::endl; return; }
        std::string input_val;
        std::getline(std::cin, input_val);
        declare_variable(strip_sigil(args[0]), input_val);
    };

    // arr_len[arr_name, $var]
    functions["arr_len"] = [this](std::vector<std::string>& args) {
        if (args.size() < 2) { std::cerr << "Error: arr_len requires arr_name and var." << std::endl; return; }
        std::string arr_name = trim(args[0]);
        std::string var_name = strip_sigil(trim(args[1]));
        declare_variable(var_name, std::to_string(arrays.count(arr_name) ? arrays[arr_name].size() : 0));
    };

    // arr_set[arr_name, index, value]
    functions["arr_set"] = [this](std::vector<std::string>& args) {
        if (args.size() < 3) { std::cerr << "Error: arr_set requires arr_name, index, value." << std::endl; return; }
        std::string arr_name = trim(args[0]);
        int idx = std::stoi(evaluate_expression(trim(args[1])));
        std::string val = evaluate_expression(trim(args[2]));
        if (!arrays.count(arr_name) || idx < 0 || (size_t)idx >= arrays[arr_name].size()) {
            std::cerr << "Error: arr_set '" << arr_name << "' index " << idx << " out of bounds." << std::endl; return;
        }
        arrays[arr_name][idx] = val;
    };

    // arr_push[arr_name, value]
    functions["arr_push"] = [this](std::vector<std::string>& args) {
        if (args.size() < 2) { std::cerr << "Error: arr_push requires arr_name and value." << std::endl; return; }
        std::string arr_name = trim(args[0]);
        arrays[arr_name].push_back(evaluate_expression(trim(args[1])));
    };
}
