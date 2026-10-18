#pragma once
#include "protocol.h"
#include <memory>
namespace cad { class Commander; }
namespace cad::protocol {
// One serial worker/session, not the P3 shared service or event subscription host.
class Endpoint {
public:
    explicit Endpoint(Commander& commander);
    ~Endpoint();
    Json execute(const Json& request);
private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};
}
