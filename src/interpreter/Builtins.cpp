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
}
