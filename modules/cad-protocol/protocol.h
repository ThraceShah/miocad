#pragma once
#include "json.hpp"
#include <stdexcept>
#include <string>
namespace cad::protocol {
using Json = nlohmann::json;
class SchemaError : public std::runtime_error { public: using std::runtime_error::runtime_error; };
const Json& contract();
void validate(const Json& value, const Json& schema);
}
