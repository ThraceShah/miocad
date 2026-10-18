#include "commander.h"
#include <iostream>
#include <string>

int main() try {
    cad::Commander commander;
    std::string line;
    while (std::getline(std::cin, line)) {
        cad::Json response;
        try { response = commander.execute(cad::Json::parse(line)); }
        catch (const cad::Json::parse_error& e) { response = cad::command_error("invalid_request", e.what()); }
        std::cout << response.dump() << std::endl;
        if (!std::cout) return 1;
    }
    return std::cin.bad() ? 1 : 0;
} catch (const std::exception& e) {
    std::cerr << "CAD worker startup/transport failure: " << e.what() << '\n';
    return 1;
}
