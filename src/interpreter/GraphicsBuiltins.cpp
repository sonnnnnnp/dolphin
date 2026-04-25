#include "DolphinInterpreter.h"
#include <iostream>

using namespace std;

static sf::Keyboard::Key str_to_key(const string& name) {
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

void DolphinInterpreter::register_graphics_builtins() {
    // window[width, height, title?]
    functions["window"] = [this](vector<string>& args) {
        args = resolve_variable_array(args);
        if (args.size() < 2) { cerr << "Error: window requires width and height." << endl; return; }
        delete gameWindow;
        gameWindow = new sf::RenderWindow(
            sf::VideoMode(stoi(args[0]), stoi(args[1])),
            args.size() > 2 ? args[2] : "dolphin"
        );
        gameWindow->setFramerateLimit(60);
    };

    // rect_create[id, x, y, w, h, r, g, b]
    functions["rect_create"] = [this](vector<string>& args) {
        if (args.size() < 8) { cerr << "Error: rect_create requires id x y w h r g b." << endl; return; }
        string id = args[0];
        vector<string> n = resolve_variable_array(vector<string>(args.begin() + 1, args.end()));

        sf::RectangleShape rect(sf::Vector2f(stof(n[2]), stof(n[3])));
        rect.setPosition(stof(n[0]), stof(n[1]));
        rect.setFillColor(sf::Color(stoi(n[4]), stoi(n[5]), stoi(n[6])));

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
        vector<string> n = resolve_variable_array(vector<string>(args.begin() + 1, args.end()));
        if (!shape_index.count(id)) { cerr << "Error: rect '" << id << "' not found." << endl; return; }
        shape_list[shape_index[id]].second.setPosition(stof(n[0]), stof(n[1]));
    };

    // key_check[KeyName, @var]
    functions["key_check"] = [this](vector<string>& args) {
        if (args.size() < 2) { cerr << "Error: key_check requires key_name and @var." << endl; return; }
        string key_name = resolve_variable(trim(args[0]));
        string var_name = trim(args[1]).substr(1);
        declare_variable(var_name, sf::Keyboard::isKeyPressed(str_to_key(key_name)) ? "1" : "0");
    };

    // bg[r, g, b]
    functions["bg"] = [this](vector<string>& args) {
        args = resolve_variable_array(args);
        if (args.size() < 3) return;
        clearColor = sf::Color(stoi(args[0]), stoi(args[1]), stoi(args[2]));
    };

    // gameloop ( ... ) — ウィンドウが閉じるまでブロックを毎フレーム実行
    keyword_handlers["gameloop"] = [this](const string& block) {
        if (!gameWindow) { cerr << "Error: call window[] before gameloop." << endl; return; }
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
    };
}
