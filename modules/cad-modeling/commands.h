#pragma once
#include "document.h"
namespace cad {
inline void fillet(Document& doc, std::size_t edge, double radius) {
  if(!doc.shape()) throw std::invalid_argument("empty document");
  auto candidate=doc.shape()->clone();
  candidate->fillet(edge,radius);
  doc.commit(std::move(candidate));
}
}
