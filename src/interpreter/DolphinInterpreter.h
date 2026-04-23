#pragma once
#include <string>
#include <unordered_map>
#include <functional>
#include <vector>

class DolphinInterpreter {
public:
    DolphinInterpreter();
    void execute(const std::string& code);

private:
    std::unordered_map<std::string, std::string> variables;
    std::unordered_map<std::string, std::function<void(std::vector<std::string>&)>> functions;

    std::string evaluate_expression(const std::string& expr);
    std::string resolve_variable(const std::string& var_name);
    std::vector<std::string> resolve_variable_array(const std::vector<std::string>& var_names);
    void declare_variable(const std::string& name, const std::string& value);
    void run_function(const std::string& name, std::vector<std::string> args);
    std::string trim(const std::string& str);
};