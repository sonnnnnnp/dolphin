#include "DolphinInterpreter.h"
#include <iostream>
#include <memory>

static sf::Keyboard::Key str_to_key(const std::string& name) {
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
    functions["window"] = [this](std::vector<std::string>& args) {
        args = resolve_variable_array(args);
        if (args.size() < 2) { std::cerr << "Error: window requires width and height." << std::endl; return; }
        delete gameWindow;
        gameWindow = new sf::RenderWindow(
            sf::VideoMode(std::stoi(args[0]), std::stoi(args[1])),
            args.size() > 2 ? args[2] : "dolphin"
        );
        gameWindow->setFramerateLimit(60);
    };

    // rect_create[id, x, y, w, h, r, g, b]
    functions["rect_create"] = [this](std::vector<std::string>& args) {
        if (args.size() < 8) { std::cerr << "Error: rect_create requires id x y w h r g b." << std::endl; return; }
        std::string id = args[0];
        std::vector<std::string> n = resolve_variable_array(std::vector<std::string>(args.begin() + 1, args.end()));

        sf::RectangleShape rect(sf::Vector2f(std::stof(n[2]), std::stof(n[3])));
        rect.setPosition(std::stof(n[0]), std::stof(n[1]));
        rect.setFillColor(sf::Color(std::stoi(n[4]), std::stoi(n[5]), std::stoi(n[6])));

        if (shape_index.count(id)) {
            shape_list[shape_index[id]].second = rect;
        } else {
            shape_index[id] = shape_list.size();
            shape_list.push_back({id, rect});
        }
    };

    // rect_set[id, x, y]
    functions["rect_set"] = [this](std::vector<std::string>& args) {
        if (args.size() < 3) { std::cerr << "Error: rect_set requires id x y." << std::endl; return; }
        std::string id = args[0];
        std::vector<std::string> n = resolve_variable_array(std::vector<std::string>(args.begin() + 1, args.end()));
        if (!shape_index.count(id)) { std::cerr << "Error: rect '" << id << "' not found." << std::endl; return; }
        shape_list[shape_index[id]].second.setPosition(std::stof(n[0]), std::stof(n[1]));
    };

    // key_check[KeyName, @var]
    functions["key_check"] = [this](std::vector<std::string>& args) {
        if (args.size() < 2) { std::cerr << "Error: key_check requires key_name and @var." << std::endl; return; }
        std::string key_name = resolve_variable(trim(args[0]));
        std::string var_name = trim(args[1]).substr(1);
        declare_variable(var_name, sf::Keyboard::isKeyPressed(str_to_key(key_name)) ? "1" : "0");
    };

    // bg[r, g, b]
    functions["bg"] = [this](std::vector<std::string>& args) {
        args = resolve_variable_array(args);
        if (args.size() < 3) return;
        clearColor = sf::Color(std::stoi(args[0]), std::stoi(args[1]), std::stoi(args[2]));
    };

    // img_load[id, path]
    functions["img_load"] = [this](std::vector<std::string>& args) {
        if (args.size() < 2) { std::cerr << "Error: img_load requires id and path." << std::endl; return; }
        std::string id = trim(args[0]), path = trim(args[1]);
        auto entry = std::make_unique<SpriteEntry>();
        if (!entry->texture.loadFromFile(path)) {
            std::cerr << "Error: Failed to load image '" << path << "'." << std::endl; return;
        }
        entry->sprite.setTexture(entry->texture);
        sprite_map[id] = std::move(entry);
    };

    // img_draw[id, x, y]
    functions["img_draw"] = [this](std::vector<std::string>& args) {
        args = resolve_variable_array(args);
        if (args.size() < 3) { std::cerr << "Error: img_draw requires id, x, y." << std::endl; return; }
        std::string id = args[0];
        if (!sprite_map.count(id)) { std::cerr << "Error: Image '" << id << "' not loaded." << std::endl; return; }
        sprite_map[id]->sprite.setPosition(std::stof(args[1]), std::stof(args[2]));
        sprite_draw_queue.push_back(sprite_map[id]->sprite);
    };

    // img_flip[id, 1/0]
    functions["img_flip"] = [this](std::vector<std::string>& args) {
        args = resolve_variable_array(args);
        if (args.size() < 2) return;
        std::string id = args[0];
        if (!sprite_map.count(id)) return;
        auto& entry = *sprite_map[id];
        bool flip = args[1] == "1";
        float w = static_cast<float>(entry.texture.getSize().x);
        entry.sprite.setScale(flip ? -1.f : 1.f, 1.f);
        entry.sprite.setOrigin(flip ? w : 0.f, 0.f);
    };

    // camera_set[x, y] — ビューの左上座標を指定
    functions["camera_set"] = [this](std::vector<std::string>& args) {
        args = resolve_variable_array(args);
        if (args.size() < 2) return;
        cameraPos = {std::stof(args[0]), std::stof(args[1])};
    };

    // gameloop ( ... )
    keyword_handlers["gameloop"] = [this](const std::string& block) {
        if (!gameWindow) { std::cerr << "Error: call window[] before gameloop." << std::endl; return; }
        auto defaultView = gameWindow->getDefaultView();
        while (gameWindow->isOpen()) {
            sf::Event event;
            while (gameWindow->pollEvent(event))
                if (event.type == sf::Event::Closed) gameWindow->close();
            if (!gameWindow->isOpen()) break;

            sprite_draw_queue.clear();
            execute(block);

            sf::View view(sf::FloatRect(
                cameraPos.x, cameraPos.y,
                defaultView.getSize().x, defaultView.getSize().y));
            gameWindow->setView(view);
            gameWindow->clear(clearColor);
            for (auto& [id, shape] : shape_list)
                gameWindow->draw(shape);
            for (auto& sp : sprite_draw_queue)
                gameWindow->draw(sp);
            gameWindow->display();
        }
        gameWindow->setView(defaultView);
    };
}
