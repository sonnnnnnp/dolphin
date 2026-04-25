#include "DolphinInterpreter.h"
#include <iostream>

void DolphinInterpreter::register_builtins() {
    // log[arg1, arg2, ...]
    functions["log"] = [this](std::vector<std::string>& args) {
        args = resolve_variable_array(args);
        if (args.empty()) return;
        std::cout << args[0];
        for (size_t i = 1; i < args.size(); ++i)
            std::cout << " " << args[i];
        std::cout << std::endl;
    };

    // input[@var]
    functions["input"] = [this](std::vector<std::string>& args) {
        if (args.empty()) { std::cerr << "Error: input requires 1 argument." << std::endl; return; }
        std::string input;
        std::getline(std::cin, input);
        declare_variable(args[0].substr(1), input);
    };

    // arr_len[arr_name, @var]
    functions["arr_len"] = [this](std::vector<std::string>& args) {
        if (args.size() < 2) { std::cerr << "Error: arr_len requires arr_name and @var." << std::endl; return; }
        std::string arr_name = trim(args[0]);
        std::string var_name = trim(args[1]).substr(1);
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
