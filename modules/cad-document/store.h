#pragma once
#include "document.h"
#include <filesystem>
#include <fstream>
#include <map>
#include <random>
#include <sstream>
#ifdef __unix__
#include <fcntl.h>
#include <unistd.h>
#endif
namespace cad {
// Persist rendering/topology order too: native SMLib roundtrips may retessellate
// faces differently even when the exact B-Rep remains valid.
class StoredShape final : public Shape {
  std::unique_ptr<Shape> body;
  DocumentJson data;
  bool cached=true;
public:
  StoredShape(std::unique_ptr<Shape> shape,DocumentJson metadata):body(std::move(shape)),data(std::move(metadata)){}
  std::unique_ptr<Shape> clone() const override {auto out=std::make_unique<StoredShape>(body->clone(),data);out->cached=cached;return out;}
  State state()const override {if(!cached)return body->state();const auto& s=data.at("state");return {s.at("volume"),s.at("faces"),s.at("edges"),s.at("valid")};}
  std::vector<Edge> edges()const override {if(!cached)return body->edges();std::vector<Edge> es;for(const auto& e:data.at("edges"))es.push_back({e.at("points").get<std::vector<Point>>(),e.at("length")});return es;}
  Mesh mesh()const override {if(!cached)return body->mesh();Mesh m;m.points=data.at("points").get<std::vector<Point>>();for(const auto& t:data.at("triangles"))m.triangles.push_back({t.at("vertices").get<std::array<std::size_t,3>>(),t.at("face")});return m;}
  void fillet(std::size_t edge,double radius)override {
    if(!cached){body->fillet(edge,radius);return;}
    const auto expected=edges().at(edge);const auto actual=body->edges();std::vector<std::size_t> matches;
    auto close=[](const Point& a,const Point& b){for(int k=0;k<3;++k)if(std::abs(a[k]-b[k])>1e-7*(1+std::max(std::abs(a[k]),std::abs(b[k]))))return false;return true;};
    for(std::size_t i=0;i<actual.size();++i){const auto& e=actual[i];if(e.points.size()!=expected.points.size()||std::abs(e.length-expected.length)>1e-7*(1+expected.length))continue;
      bool forward=true,reverse=true;for(std::size_t j=0;j<e.points.size();++j){forward=forward&&close(e.points[j],expected.points[j]);reverse=reverse&&close(e.points[e.points.size()-1-j],expected.points[j]);}if(forward||reverse)matches.push_back(i);}
    if(matches.size()!=1)throw std::runtime_error("saved edge has no unique native correspondence");
    body->fillet(matches.front(),radius);cached=false;
  }
  void save_native(const std::string& path)const override{body->save_native(path);}
};
class DocumentStore {
  std::filesystem::path root;
  std::map<const Shape*,std::pair<std::weak_ptr<const Shape>,std::string>> written;
  static std::string unique(){std::random_device r;std::ostringstream s;s<<std::hex;for(int i=0;i<4;++i)s<<r()<<'-';return s.str();}
  static void sync(const std::filesystem::path& path){
#ifdef __unix__
    int fd=::open(path.c_str(),O_RDONLY);if(fd<0)throw std::runtime_error("cannot open persistence file");int status=::fsync(fd);::close(fd);if(status)throw std::runtime_error("cannot sync persistence file");
#else
    (void)path;
#endif
  }
  std::string geometry(const std::shared_ptr<const Shape>& shape){
    const auto it=written.find(shape.get());if(it!=written.end()&&!it->second.first.expired())return it->second.second;
    const auto id=unique()+".smb", temp=id+".tmp";
    try{shape->save_native((root/temp).string());sync(root/temp);std::filesystem::rename(root/temp,root/id);}catch(...){std::error_code e;std::filesystem::remove(root/temp,e);throw;}
    const auto state=shape->state();const auto mesh=shape->mesh();DocumentJson edges=DocumentJson::array(),triangles=DocumentJson::array();
    for(const auto& e:shape->edges())edges.push_back({{"points",e.points},{"length",e.length}});
    for(const auto& t:mesh.triangles)triangles.push_back({{"vertices",t.vertices},{"face",t.face}});
    DocumentJson metadata={{"state",{{"volume",state.volume},{"faces",state.faces},{"edges",state.edges},{"valid",state.valid}}},{"edges",edges},{"points",mesh.points},{"triangles",triangles}};
    {std::ofstream out(root/(id+".json"));out.exceptions(std::ios::badbit|std::ios::failbit);out<<metadata.dump();out.flush();}sync(root/(id+".json"));
    written[shape.get()]={shape,id};return id;
  }
public:
  explicit DocumentStore(std::filesystem::path directory):root(std::move(directory)){}
  bool enabled()const{return !root.empty();}
  bool save(const std::map<std::string,Document>& documents){
    if(!enabled())return true;
    std::filesystem::create_directories(root);
    DocumentJson docs=DocumentJson::object();
    for(const auto& [id,doc]:documents)docs[id]=doc.archive([&](const auto& shape){return geometry(shape);});
    const DocumentJson manifest={{"format",1},{"kernel","smlib"},{"kernel_version","usd-brep-c9c5979"},{"documents",docs}};
    const auto bytes=manifest.dump();if(bytes.size()>64*1024*1024)throw std::runtime_error("document manifest exceeds 64 MiB");
    const auto temp=root/("workspace-"+unique()+".tmp");
    try{{std::ofstream out(temp,std::ios::binary);out.exceptions(std::ios::badbit|std::ios::failbit);out<<bytes;out.flush();}sync(temp);
      // Persist new immutable shape directory entries before the manifest points at them.
      sync(root);std::filesystem::rename(temp,root/"workspace.json");
    }catch(...){std::error_code e;std::filesystem::remove(temp,e);throw;}
    // Rename is the publication point. Directory sync failure must never be reported
    // as a rejected transaction after the manifest already changed.
#ifdef __unix__
    int fd=::open(root.c_str(),O_RDONLY);if(fd<0)return false;const int status=::fsync(fd);::close(fd);if(status)return false;
#endif
    return true;
  }
  std::map<std::string,Document> load(IKernel& kernel){
    std::map<std::string,Document> docs;
    if(!enabled()||!std::filesystem::exists(root/"workspace.json"))return docs;
    if(std::filesystem::file_size(root/"workspace.json")>64*1024*1024)throw std::runtime_error("document manifest too large");
    std::ifstream in(root/"workspace.json");auto m=DocumentJson::parse(in);
    if(m.at("format")!=1||m.at("kernel")!="smlib"||m.at("kernel_version")!="usd-brep-c9c5979")throw std::runtime_error("incompatible document store; explicit migration required");
    std::map<std::string,std::shared_ptr<const Shape>> shapes;
    for(const auto& [id,a]:m.at("documents").items()){
      docs.emplace(id,Document::restore(a,[&](const std::string& name){
        if(name.empty()||name.size()>160||name.find_first_not_of("0123456789abcdef-.smb")!=std::string::npos||std::filesystem::path(name).filename()!=name||std::filesystem::path(name).extension()!=".smb")throw std::runtime_error("invalid geometry path");
        auto& shape=shapes[name];if(!shape){if(std::filesystem::file_size(root/name)>512*1024*1024 || std::filesystem::file_size(root/(name+".json"))>64*1024*1024)throw std::runtime_error("saved geometry exceeds limits");
          auto native=kernel.load_native((root/name).string());std::ifstream metadata(root/(name+".json"));auto data=DocumentJson::parse(metadata);
          const auto current=native->state();const double saved=data.at("state").at("volume");
          if(!current.valid||current.edges!=data.at("state").at("edges")||current.faces!=data.at("state").at("faces")||std::abs(current.volume-saved)>1e-7*(1+std::abs(saved)))throw std::runtime_error("saved geometry validation mismatch");
          shape=std::make_shared<StoredShape>(std::move(native),std::move(data));written[shape.get()]={shape,name};}return shape;
      }));
    }
    return docs;
  }
};
}
