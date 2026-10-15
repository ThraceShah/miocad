#pragma once
#include "kernel.h"
#include <cstdint>
#include <stdexcept>
namespace cad {
class Document {
  std::vector<std::shared_ptr<const Shape>> history{nullptr};
  std::size_t cursor=0;
  std::uint64_t revision_=0;
public:
  std::uint64_t revision() const { return revision_; }
  const Shape* shape() const { return history[cursor].get(); }
  void expect(std::uint64_t r) const { if(r!=revision_) throw std::invalid_argument("stale revision"); }
  void commit(std::unique_ptr<Shape> candidate) {
    if(!candidate || !candidate->state().valid) throw std::runtime_error("candidate validation failed");
    // Build replacement history before swapping, including allocation failures.
    auto next=std::vector<std::shared_ptr<const Shape>>(history.begin(),history.begin()+cursor+1);
    next.push_back(std::shared_ptr<const Shape>(std::move(candidate)));
    history.swap(next); ++cursor; ++revision_;
  }
  void undo() { if(!cursor) throw std::invalid_argument("nothing to undo"); --cursor; ++revision_; }
  void redo() { if(cursor+1>=history.size()) throw std::invalid_argument("nothing to redo"); ++cursor; ++revision_; }
};
}
