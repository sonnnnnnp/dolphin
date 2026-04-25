#include "DolphinInterpreter.h"
#include <iostream>

using namespace std;

void DolphinInterpreter::register_builtins() {
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

    // arr_len[arr_name, @var]
    functions["arr_len"] = [this](vector<string>& args) {
        if (args.size() < 2) { cerr << "Error: arr_len requires arr_name and @var." << endl; return; }
        string arr_name = trim(args[0]);
        string var_name = trim(args[1]).substr(1);
        declare_variable(var_name, to_string(arrays.count(arr_name) ? arrays[arr_name].size() : 0));
    };

    // arr_set[arr_name, index, value]
    functions["arr_set"] = [this](vector<string>& args) {
        if (args.size() < 3) { cerr << "Error: arr_set requires arr_name, index, value." << endl; return; }
        string arr_name = trim(args[0]);
        int idx = stoi(evaluate_expression(trim(args[1])));
        string val = evaluate_expression(trim(args[2]));
        if (!arrays.count(arr_name) || idx < 0 || (size_t)idx >= arrays[arr_name].size()) {
            cerr << "Error: arr_set '" << arr_name << "' index " << idx << " out of bounds." << endl; return;
        }
        arrays[arr_name][idx] = val;
    };

    // arr_push[arr_name, value]
    functions["arr_push"] = [this](vector<string>& args) {
        if (args.size() < 2) { cerr << "Error: arr_push requires arr_name and value." << endl; return; }
        string arr_name = trim(args[0]);
        arrays[arr_name].push_back(evaluate_expression(trim(args[1])));
    };
}
