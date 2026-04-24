#pragma once
#include <string>
#include <unordered_map>
#include <functional>
#include <vector>
#include <SFML/Graphics.hpp>

class DolphinInterpreter {
public:
    DolphinInterpreter();
    ~DolphinInterpreter();
    void execute(const std::string& code);

private:
    std::unordered_map<std::string, std::string> variables;
    std::unordered_map<std::string, std::function<void(std::vector<std::string>&)>> functions;

    sf::RenderWindow* gameWindow = nullptr;
    std::vector<std::pair<std::string, sf::RectangleShape>> shape_list;
    std::unordered_map<std::string, size_t> shape_index;
    sf::Color clearColor{0, 0, 0};

    std::string evaluate_expression(const std::string& expr);
    std::string resolve_variable(const std::string& var_name);
    std::vector<std::string> resolve_variable_array(const std::vector<std::string>& var_names);
    void declare_variable(const std::string& name, const std::string& value);
    void run_function(const std::string& name, std::vector<std::string> args);
    std::string trim(const std::string& str);
    std::string read_block(std::istringstream& ss, const std::string& first_line);
    static sf::Keyboard::Key str_to_key(const std::string& name);
};
