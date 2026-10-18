#include "commander.h"
#include "endpoint.h"
#include <iostream>
#include <string>

int main() try {
    cad::Commander commander;
    cad::protocol::Endpoint endpoint(commander);
    std::string line;
    while (std::getline(std::cin, line)) {
        cad::Json response;
        try {
            const auto request=cad::Json::parse(line);
            const bool versioned=request.is_object() && (request.contains("protocol") || request.contains("method") || request.contains("request_id"));
            response=versioned?endpoint.execute(request):commander.execute(request);
        }
        catch (const cad::Json::parse_error& e) { response = cad::command_error("invalid_request", e.what()); }
        std::cout << response.dump() << std::endl;
        if (!std::cout) return 1;
    }
    return std::cin.bad() ? 1 : 0;
} catch (const std::exception& e) {
    std::cerr << "CAD worker startup/transport failure: " << e.what() << '\n';
    return 1;
}
