#pragma once
#include <string>
#include <sstream>
#include <unordered_map>
#include <functional>
#include <vector>
#include <memory>
#include <SFML/Graphics.hpp>
#include <SFML/Audio.hpp>

// ユーザー定義関数の return に使う例外
struct ReturnException {
    std::string value;
};

class DolphinInterpreter {
public:
    DolphinInterpreter();
    ~DolphinInterpreter();
    void execute(const std::string& code);

private:
    // --- 型エイリアス ---
    using BuiltinFn    = std::function<void(std::vector<std::string>&)>;
    using BlockHandler = std::function<void(const std::string&)>;

    // ユーザー定義関数
    struct UserFunction {
        std::vector<std::string> params; // $ なしの名前
        std::string              body;
    };

    // --- 言語状態 ---
    std::unordered_map<std::string, std::string>              variables;
    std::unordered_map<std::string, std::vector<std::string>> arrays;
    std::unordered_map<std::string, BuiltinFn>                functions;
    std::unordered_map<std::string, BlockHandler>             keyword_handlers;
    std::unordered_map<std::string, UserFunction>             user_functions;

    // ローカルスコープスタック ($var)
    std::vector<std::unordered_map<std::string, std::string>> local_stack;

    // --- グラフィクス状態 ---
    sf::RenderWindow* gameWindow = nullptr;
    std::vector<std::pair<std::string, sf::RectangleShape>> shape_list;
    std::unordered_map<std::string, size_t> shape_index;
    sf::Color clearColor{0, 0, 0};

    struct SpriteEntry {
        sf::Texture texture;
        sf::Sprite  sprite;
    };
    std::unordered_map<std::string, std::unique_ptr<SpriteEntry>> sprite_map;
    std::vector<sf::Sprite> sprite_draw_queue;
    sf::Vector2f cameraPos{0.f, 0.f};

    std::unordered_map<std::string, std::unique_ptr<sf::Font>> font_map;
    std::vector<std::pair<std::string, sf::CircleShape>> circle_list;
    std::unordered_map<std::string, size_t> circle_index;

    struct TextEntry { sf::Text text; };
    std::vector<std::pair<std::string, TextEntry>> text_list;
    std::unordered_map<std::string, size_t> text_index;

    std::vector<sf::RectangleShape> rect_draw_queue;
    bool mouseClickedThisFrame = false;

    struct SoundEntry {
        sf::SoundBuffer buffer;
        sf::Sound       sound;
    };
    std::unordered_map<std::string, std::unique_ptr<SoundEntry>> sound_map;

    // --- パーサー / 評価 ---
    std::string evaluate_expression(const std::string& expr);
    std::string resolve_variable(const std::string& name);
    std::vector<std::string> resolve_variable_array(const std::vector<std::string>& args);
    void declare_variable(const std::string& name, const std::string& value);
    void run_function(const std::string& name, std::vector<std::string> args);
    std::string call_user_function(const std::string& name, const std::vector<std::string>& args);
    std::string read_block(std::istringstream& ss, const std::string& first_line);
    std::string trim(const std::string& str);
    std::string interpolate(const std::string& tmpl);
    bool is_function_def(const std::string& line);
    void parse_function_def(std::istringstream& ss, const std::string& line);

    // --- 組み込み登録（各 *Builtins.cpp で実装） ---
    void register_builtins();
    void register_graphics_builtins();
};
