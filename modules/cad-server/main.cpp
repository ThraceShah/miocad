#include "commands.h"
#include "reference.h"
#include "json.hpp"
#include <filesystem>
#include <iostream>
#include <map>
#include <random>
#include <sstream>
using json=nlohmann::json;
std::string new_id() { std::random_device r; std::ostringstream out; out<<std::hex; for(int i=0;i<4;++i) out<<r()<<'-'; return out.str(); }
json state(const cad::Document& d) {
  json result={{"revision",d.revision()},{"units","mm"},{"empty",!d.shape()}};
  if(d.shape()) { auto s=d.shape()->state(); result.update({{"volume",s.volume},{"faces",s.faces},{"edges",s.edges},{"valid",s.valid},{"bounds",{{"min",s.minimum},{"max",s.maximum}}}}); }
  return result;
}
int main(int argc, char** argv) try {
  std::string selected="smlib";
  if(argc==3 && std::string(argv[1])=="--kernel") selected=argv[2];
  else if(argc!=1) throw std::invalid_argument("usage: cad-server [--kernel smlib|occt]");
  std::unique_ptr<cad::IKernel> kernel;
  if(selected=="smlib") kernel=cad::make_smlib_kernel();
#ifdef CAD_WITH_OCCT
  else if(selected=="occt") kernel=cad::make_occt_kernel();
#endif
  else throw std::invalid_argument("kernel unavailable in this build: "+selected);
  std::map<std::string,cad::Document> docs; std::string line;
  while(std::getline(std::cin,line)) {
    json request, response;
    try {
      request=json::parse(line); const auto op=request.at("op").get<std::string>();
      json result;
      if(op=="capabilities") result={{"kernel",kernel->name()},{"step_import",kernel->supports_step()},{"step_export",kernel->supports_step()},{"gpu_picking",false},{"persistent_naming",false},{"native_format",kernel->native_format()}};
      else if(op=="document.create") { auto id=new_id(); docs.emplace(id,cad::Document{}); result=state(docs.at(id)); result["document"]=id; }
      else {
        auto id=request.at("document").get<std::string>(); auto it=docs.find(id);
        if(it==docs.end()) throw std::invalid_argument("unknown document");
        auto& doc=it->second;
        if(op=="model.box" || op=="model.fillet" || op=="document.undo" || op=="document.redo" || op=="import.step") {
          const auto& rev=request.at("revision");
          if(!rev.is_number_unsigned() && !(rev.is_number_integer() && rev.get<long long>()>=0)) throw std::invalid_argument("revision must be nonnegative integer");
          doc.expect(rev.get<std::uint64_t>());
          if(op=="model.box") doc.commit(kernel->box(request.at("size").get<cad::Point>()));
          else if(op=="import.step") doc.commit(kernel->import_step(request.at("path").get<std::string>()));
          else if(op=="model.fillet") {
            if(!doc.shape()) throw std::invalid_argument("empty document");
            auto edge=cad::resolve_edge(request.at("edge").get<std::string>(),id,doc.revision(),doc.shape()->state().edges);
            cad::fillet(doc,edge,request.at("radius").get<double>());
          } else if(op=="document.undo") doc.undo(); else doc.redo();
          result=state(doc);
        } else if(op=="document.state") result=state(doc);
        else {
          if(!doc.shape()) throw std::invalid_argument("empty document");
          if(op=="topology.edges") {
            result=json::array(); auto edges=doc.shape()->edges();
            for(std::size_t i=0;i<edges.size();++i) result.push_back({{"reference",cad::token(id,doc.revision(),"edge",i)},{"points",edges[i].points},{"length",edges[i].length}});
          } else if(op=="render.mesh") {
            auto mesh=doc.shape()->mesh(); json triangles=json::array();
            for(const auto& t:mesh.triangles) triangles.push_back({{"vertices",t.vertices},{"face",cad::token(id,doc.revision(),"face",t.face)}});
            result={{"points",mesh.points},{"triangles",triangles}};
          } else if(op=="export.native" || op=="export.step") {
            auto path=request.at("path").get<std::string>();
            bool step=op=="export.step";
            if(step && !kernel->supports_step()) throw std::invalid_argument("unsupported capability: STEP export unavailable for selected kernel");
            const auto extension=std::filesystem::path(path).extension().string();
            if(step ? (extension!=".step" && extension!=".stp") : extension!="."+kernel->native_format())
              throw std::invalid_argument(step?"STEP export requires .step or .stp extension":"native export requires ."+kernel->native_format()+" extension");
            auto temp=path+".tmp-"+new_id();
            try {
              if(step) doc.shape()->save_step(temp); else doc.shape()->save_native(temp);
              std::filesystem::rename(temp,path);
            } catch(...) { std::error_code ignored; std::filesystem::remove(temp,ignored); throw; }
            result={{"path",path},{"format",step?"step":kernel->native_format()}};
          }
          else throw std::invalid_argument("unknown operation");
        }
        result=json{{"document",id},{"revision",doc.revision()},{"data",result}};
      }
      response={{"ok",true},{"result",result}};
    } catch(const std::exception& e) { response={{"ok",false},{"error",{{"code","command_failed"},{"message",e.what()}}}}; }
    if(request.is_object() && request.contains("id")) response["id"]=request["id"];
    std::cout<<response.dump()<<std::endl;
  }
}
catch(const std::exception& e) {
  std::cerr<<e.what()<<'\n'; return 2;
}
