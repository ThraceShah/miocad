#include "kernel.h"
#include <stdexcept>
namespace cad {
void Shape::save_step(const std::string&) const {
  throw std::invalid_argument("unsupported capability: STEP export unavailable for selected kernel");
}
std::unique_ptr<Shape> IKernel::import_step(const std::string&) {
  throw std::invalid_argument("unsupported capability: STEP import unavailable for selected kernel");
}
}
