#pragma once
#include <string>
#include <stdexcept>
namespace cad {
inline std::string token(const std::string& doc, unsigned long long revision, const std::string& kind, std::size_t slot) {
  return doc+":"+std::to_string(revision)+":"+kind+":"+std::to_string(slot);
}
inline std::size_t resolve_edge(const std::string& value,const std::string& doc,unsigned long long revision,std::size_t count) {
  // Resolve by exact token comparison; never parse a foreign/stale numeric slot.
  for(std::size_t i=0;i<count;++i) if(value==token(doc,revision,"edge",i)) return i;
  throw std::invalid_argument("unknown or stale edge reference");
}
}
