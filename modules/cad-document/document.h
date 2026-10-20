#pragma once
#include "kernel.h"
#include "json.hpp"
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <functional>
#include <set>
#include <stdexcept>
#include <utility>
namespace cad {
using DocumentJson = nlohmann::json;
class DocumentValidationError : public std::runtime_error {
public:
  DocumentValidationError() : std::runtime_error("candidate validation failed") {}
};
class FeatureError : public std::runtime_error {
public:
  std::string kind, feature;
  FeatureError(std::string k, std::string f, const std::string& message)
    : std::runtime_error("feature " + f + ": " + message), kind(std::move(k)), feature(std::move(f)) {}
};
struct Feature {
  std::string id, name, kind, input;
  DocumentJson parameters, selector;
  bool suppressed = false;
  std::shared_ptr<const Shape> output;
};
struct DocumentSnapshot {
  std::string id, label;
  std::vector<Feature> features;
  std::shared_ptr<const Shape> shape;
};
// Feature definitions and undo entries are distinct. Cached geometry is immutable.
// Every mutator is invoked on Commander's staged Document; publish is a noexcept swap.
class Document {
  std::vector<std::shared_ptr<const DocumentSnapshot>> history{std::make_shared<DocumentSnapshot>(DocumentSnapshot{"s0", "Empty document", {}, nullptr})};
  std::size_t cursor=0;
  std::uint64_t revision_=0, serial=0, state_serial=0;
  static std::pair<Point,Point> bounds(const Shape& s) {
    Point lo{1e300,1e300,1e300}, hi{-1e300,-1e300,-1e300};
    for(const auto& e:s.edges()) for(const auto& p:e.points) for(int k=0;k<3;++k){lo[k]=std::min(lo[k],p[k]);hi[k]=std::max(hi[k],p[k]);}
    for(int k=0;k<3;++k) if(!(hi[k]>lo[k])) throw std::runtime_error("cannot establish reference coordinate frame");
    return {lo,hi};
  }
  static DocumentJson signature(const Edge& edge, const Shape& root) {
    const auto [lo,hi]=bounds(root);
    std::vector<Point> points;
    for(auto p:edge.points){for(int k=0;k<3;++k)p[k]=(p[k]-lo[k])/(hi[k]-lo[k]);points.push_back(p);}
    // A supporting line is stable when neighboring fillets shorten its endpoints.
    for(int axis=0;axis<3 && points.size()>1;++axis){
      bool straight=true;
      for(const auto& p:points) for(int k=0;k<3;++k)if(k!=axis && std::abs(p[k]-points.front()[k])>1e-8)straight=false;
      if(straight && std::abs(points.front()[axis]-points.back()[axis])>1e-8 && [&]{for(int k=0;k<3;++k)if(k!=axis && std::min(std::abs(points.front()[k]),std::abs(points.front()[k]-1.0))>1e-8)return false;return true;}()){auto fixed=points.front();fixed[axis]=0;return {{"mode","line"},{"axis",axis},{"fixed",fixed}};}
    }
    return {{"mode","exact_curve"},{"points",points}};
  }
  static bool matches(const DocumentJson& a,const DocumentJson& b) {
    if(a.at("mode")!=b.at("mode"))return false;
    auto close=[](Point x,Point y){for(int k=0;k<3;++k)if(std::abs(x[k]-y[k])>1e-8)return false;return true;};
    if(a.at("mode")=="line")return a.at("axis")==b.at("axis") && close(a.at("fixed").get<Point>(),b.at("fixed").get<Point>());
    const auto x=a.at("points").get<std::vector<Point>>(),y=b.at("points").get<std::vector<Point>>();
    if(x.size()!=y.size())return false;
    bool forward=true,reverse=true;
    for(std::size_t i=0;i<x.size();++i){forward=forward&&close(x[i],y[i]);reverse=reverse&&close(x[i],y[y.size()-1-i]);}
    return forward||reverse;
  }
  static const Shape& root_shape(const std::vector<Feature>& fs) {
    for(const auto& f:fs)if(f.kind=="box" && f.output)return *f.output;
    throw std::invalid_argument("no active Box reference frame");
  }
  void publish(DocumentSnapshot candidate, const std::string& label) {
    if(cursor>=9999)throw std::runtime_error("history limit reached (10000 states)");
    if(candidate.shape && !candidate.shape->state().valid)throw DocumentValidationError();
    candidate.id="s"+std::to_string(++state_serial);candidate.label=label;
    auto next=std::vector<std::shared_ptr<const DocumentSnapshot>>(history.begin(),history.begin()+cursor+1);
    next.push_back(std::make_shared<const DocumentSnapshot>(std::move(candidate)));
    history.swap(next);++cursor;++revision_;
  }
  static void recompute(DocumentSnapshot& state, IKernel& kernel, std::size_t from) {
    for(std::size_t i=from;i<state.features.size();++i){
      auto& f=state.features[i];std::shared_ptr<const Shape> input;
      if(!f.input.empty()){
        const auto it=std::find_if(state.features.begin(),state.features.begin()+i,[&](const Feature& p){return p.id==f.input;});
        if(it==state.features.begin()+i)throw FeatureError("invalid_dependency",f.id,"input must precede this feature");
        input=it->output;
      }
      try {
        if(f.suppressed){f.output=input;continue;}
        std::unique_ptr<Shape> result;
        if(f.kind=="box")result=kernel.box(f.parameters.at("size").get<Point>());
        else if(f.kind=="fillet"){
          if(!input)throw FeatureError("invalid_dependency",f.id,"input is empty or suppressed");
          const auto edges=input->edges();std::vector<std::size_t> found;
          for(std::size_t j=0;j<edges.size();++j)if(matches(f.selector,signature(edges[j],root_shape(state.features))))found.push_back(j);
          if(found.size()!=1)throw FeatureError(found.empty()?"reference_unresolved":"reference_ambiguous",f.id,"edge selector matched "+std::to_string(found.size())+" edges; explicit repair required");
          result=input->clone();result->fillet(found[0],f.parameters.at("radius").get<double>());
        }else throw FeatureError("invalid_dependency",f.id,"unknown feature kind");
        if(!result || !result->state().valid)throw DocumentValidationError();
        f.output=std::shared_ptr<const Shape>(std::move(result));
      }catch(const FeatureError&){throw;}catch(const std::exception& e){throw FeatureError("recompute_failed",f.id,e.what());}
    }
    state.shape=state.features.empty()?nullptr:state.features.back().output;
  }
public:
  std::uint64_t revision() const { return revision_; }
  const Shape* shape() const { return history[cursor]->shape.get(); }
  const DocumentSnapshot& snapshot() const {return *history[cursor];}
  bool can_undo() const { return cursor>0; }
  bool can_redo() const { return cursor+1<history.size(); }
  void swap(Document& other) noexcept {history.swap(other.history);std::swap(cursor,other.cursor);std::swap(revision_,other.revision_);std::swap(serial,other.serial);std::swap(state_serial,other.state_serial);}
  void expect(std::uint64_t r) const {if(r!=revision_)throw std::invalid_argument("stale revision");}
  // Retained for low-level callers. Application modeling uses typed feature methods.
  void commit(std::unique_ptr<Shape> candidate){if(!candidate||!candidate->state().valid)throw DocumentValidationError();DocumentSnapshot s;s.shape=std::shared_ptr<const Shape>(std::move(candidate));publish(std::move(s),"Geometry replacement");}
  void box(IKernel& kernel,Point size){
    const auto id="f"+std::to_string(++serial);
    DocumentSnapshot s;s.features.push_back({id,"Box "+std::to_string(serial),"box","",{{"size",size}},nullptr,false,nullptr});
    recompute(s,kernel,0);publish(std::move(s),"Create Box");
  }
  void fillet(IKernel&,std::size_t edge,double radius){
    if(!shape())throw std::invalid_argument("empty document");
    auto s=snapshot();const auto edges=shape()->edges();if(edge>=edges.size())throw std::invalid_argument("edge does not exist");
    // First application uses the explicitly picked index; later recomputations use the selector.
    auto result=shape()->clone();result->fillet(edge,radius);if(!result->state().valid)throw DocumentValidationError();
    const auto selector=signature(edges[edge],root_shape(s.features));const auto id="f"+std::to_string(++serial);
    const auto input=s.features.back().id;
    s.features.push_back({id,"Fillet "+std::to_string(serial),"fillet",input,{{"radius",radius}},selector,false,std::shared_ptr<const Shape>(std::move(result))});
    s.shape=s.features.back().output;publish(std::move(s),"Create Fillet");
  }
  void edit(IKernel& kernel,const std::string& id,const DocumentJson& patch){
    if(!patch.is_object()||patch.empty())throw std::invalid_argument("feature patch cannot be empty");
    auto s=snapshot();const auto it=std::find_if(s.features.begin(),s.features.end(),[&](const Feature& f){return f.id==id;});
    if(it==s.features.end())throw FeatureError("feature_not_found",id,"unknown feature");
    auto& f=*it;bool geometry=false;
    for(const auto& [key,value]:patch.items()){
      if(key=="name"){f.name=value.get<std::string>();if(f.name.empty()||f.name.size()>120)throw std::invalid_argument("feature name must contain 1..120 bytes");}
      else if(key=="suppressed"){geometry=geometry||(f.suppressed!=value.get<bool>());f.suppressed=value.get<bool>();}
      else if(key=="size" && f.kind=="box"){auto size=value.get<Point>();for(double v:size)if(!std::isfinite(v)||v<=0||v>1e6)throw std::invalid_argument("invalid Box dimensions");geometry=geometry||f.parameters.at("size")!=value;f.parameters["size"]=value;}
      else if(key=="radius" && f.kind=="fillet"){auto r=value.get<double>();if(!std::isfinite(r)||r<=0)throw std::invalid_argument("invalid Fillet radius");geometry=geometry||f.parameters.at("radius")!=value;f.parameters["radius"]=value;}
      else throw std::invalid_argument("parameter does not belong to this feature: "+key);
    }
    if(geometry)recompute(s,kernel,static_cast<std::size_t>(it-s.features.begin()));
    publish(std::move(s),"Edit "+id);
  }
  void recompute_all(IKernel& kernel){auto s=snapshot();recompute(s,kernel,0);publish(std::move(s),"Recompute features");}
  void undo(){if(!can_undo())throw std::invalid_argument("nothing to undo");--cursor;++revision_;}
  void redo(){if(!can_redo())throw std::invalid_argument("nothing to redo");++cursor;++revision_;}
  void accept(const Document& branch,const std::string& label){auto s=branch.snapshot();serial=std::max(serial,branch.serial);publish(std::move(s),label);}
  DocumentJson features() const {
    DocumentJson a=DocumentJson::array();
    for(const auto& f:snapshot().features)a.push_back({{"id",f.id},{"name",f.name},{"kind",f.kind},{"dependencies",f.input.empty()?std::vector<std::string>{}:std::vector<std::string>{f.input}},{"parameters",f.parameters},{"selector",f.selector},{"suppressed",f.suppressed},{"status",f.suppressed?"suppressed":"valid"}});
    return a;
  }
  DocumentJson timeline() const {
    DocumentJson entries=DocumentJson::array();for(const auto& s:history)entries.push_back({{"id",s->id},{"label",s->label}});
    return {{"cursor",cursor},{"entries",entries},{"can_undo",can_undo()},{"can_redo",can_redo()}};
  }
  // The writer stores immutable native geometry; the manifest restores results, never replaying commands.
  DocumentJson archive(const std::function<std::string(const std::shared_ptr<const Shape>&)>& write) const {
    DocumentJson states=DocumentJson::array();
    for(const auto& s:history){DocumentJson fs=DocumentJson::array();for(const auto& f:s->features)fs.push_back({{"id",f.id},{"name",f.name},{"kind",f.kind},{"input",f.input},{"parameters",f.parameters},{"selector",f.selector},{"suppressed",f.suppressed},{"output",f.output?write(f.output):""}});
      states.push_back({{"id",s->id},{"label",s->label},{"features",fs},{"shape",s->shape?write(s->shape):""}});}
    return {{"format",1},{"revision",revision_},{"serial",serial},{"state_serial",state_serial},{"cursor",cursor},{"states",states}};
  }
  static Document restore(const DocumentJson& a,const std::function<std::shared_ptr<const Shape>(const std::string&)>& read){
    if(a.at("format")!=1)throw std::invalid_argument("unsupported document format");
    Document d;d.history.clear();d.revision_=a.at("revision");d.serial=a.at("serial");d.state_serial=a.at("state_serial");d.cursor=a.at("cursor");
    std::set<std::string> state_ids;
    for(const auto& s:a.at("states")){DocumentSnapshot out{s.at("id"),s.at("label"),{},nullptr};
      for(const auto& f:s.at("features")){const std::string output=f.at("output");out.features.push_back({f.at("id"),f.at("name"),f.at("kind"),f.at("input"),f.at("parameters"),f.at("selector"),f.at("suppressed"),output.empty()?nullptr:read(output)});}
      const std::string shape=s.at("shape");out.shape=shape.empty()?nullptr:read(shape);
      if(!state_ids.insert(out.id).second||out.id.size()<2||out.id[0]!='s'||out.id.find_first_not_of("0123456789",1)!=std::string::npos||std::stoull(out.id.substr(1))>d.state_serial)throw std::invalid_argument("invalid history identity");
      std::set<std::string> ids;std::string previous;
      for(const auto& f:out.features){
        if(f.id.size()<2||f.id[0]!='f'||f.id.find_first_not_of("0123456789",1)!=std::string::npos||std::stoull(f.id.substr(1))>d.serial||!ids.insert(f.id).second||f.name.empty()||f.name.size()>120)throw std::invalid_argument("invalid feature identity");
        if(f.kind=="box"){
          if(!previous.empty()||!f.input.empty())throw std::invalid_argument("Box must be the root feature");
          const auto size=f.parameters.at("size").get<Point>();for(double v:size)if(!std::isfinite(v)||v<=0||v>1e6)throw std::invalid_argument("invalid saved Box");
        }else if(f.kind=="fillet"){
          if(previous.empty()||f.input!=previous)throw std::invalid_argument("invalid feature dependency");
          const double r=f.parameters.at("radius");if(!std::isfinite(r)||r<=0||r>10000)throw std::invalid_argument("invalid saved Fillet");
          if(f.selector.at("mode")=="line"){
            const int axis=f.selector.at("axis");if(axis<0||axis>2)throw std::invalid_argument("invalid edge axis");f.selector.at("fixed").get<Point>();
          }else if(f.selector.at("mode")=="exact_curve"){if(f.selector.at("points").size()<2)throw std::invalid_argument("invalid edge curve");f.selector.at("points").get<std::vector<Point>>();}
          else throw std::invalid_argument("invalid edge selector");
        }else throw std::invalid_argument("unknown saved feature kind");
        previous=f.id;
      }
      if((out.features.empty()&&out.shape)||(!out.features.empty()&&out.shape!=out.features.back().output))throw std::invalid_argument("feature result and document result disagree");
      d.history.push_back(std::make_shared<const DocumentSnapshot>(std::move(out)));}
    if(d.history.empty()||d.history.size()>10000||d.cursor>=d.history.size())throw std::invalid_argument("invalid history cursor");return d;
  }
};
}
