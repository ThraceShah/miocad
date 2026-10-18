#pragma once
#include "kernel.h"
#include "json.hpp"
#include <memory>
#include <string>

namespace cad {
using Json = nlohmann::json;

// Application boundary; no window, socket, stream or SMLib types are required.
// A Commander instance owns its documents and is called serially.
class Commander {
public:
    Commander();
    explicit Commander(std::unique_ptr<IKernel> kernel);
    ~Commander();
    Commander(const Commander&) = delete;
    Commander& operator=(const Commander&) = delete;

    Json execute(const Json& request);
    Json commands() const;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

// Shared legacy-compatible error envelope for application and transport errors.
Json command_error(const std::string& kind, const std::string& message);
}
