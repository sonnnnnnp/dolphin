#include <iostream>
#include <sstream>
#include <fstream>

#include "interpreter/DolphinInterpreter.h"

using namespace std;

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