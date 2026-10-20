#include "commander.h"
#include <iostream>
#include <stdexcept>
using cad::Json;
struct Counts {int boxes=0,fillets=0;};
class CountShape final:public cad::Shape {
  std::unique_ptr<cad::Shape> body;std::shared_ptr<Counts> counts;
public:
  CountShape(std::unique_ptr<cad::Shape> s,std::shared_ptr<Counts> c):body(std::move(s)),counts(std::move(c)){}
  std::unique_ptr<cad::Shape> clone()const override{return std::make_unique<CountShape>(body->clone(),counts);}
  cad::State state()const override{return body->state();}
  std::vector<cad::Edge> edges()const override{return body->edges();}
  cad::Mesh mesh()const override{return body->mesh();}
  void fillet(std::size_t e,double r)override{++counts->fillets;body->fillet(e,r);}
  void save_native(const std::string& p)const override{body->save_native(p);}
};
class CountKernel final:public cad::IKernel {
  std::unique_ptr<cad::IKernel> kernel=cad::make_smlib_kernel();std::shared_ptr<Counts> counts;
public:
  explicit CountKernel(std::shared_ptr<Counts> c):counts(std::move(c)){}
  std::unique_ptr<cad::Shape> box(cad::Point size)override{++counts->boxes;return std::make_unique<CountShape>(kernel->box(size),counts);}
};
void require(bool ok,const std::string& text){if(!ok)throw std::runtime_error(text);}
int main()try{
  auto counts=std::make_shared<Counts>();cad::Commander c(std::make_unique<CountKernel>(counts));
  auto call=[&](Json request){auto r=c.execute(request);require(r.at("ok"),r.dump());return r.at("result");};
  std::string doc=call({{"op","document.create"}}).at("document");
  auto scene=[&]{return call({{"op","render.scene"},{"document",doc}});};
  auto write=[&](Json req){req["document"]=doc;req["revision"]=scene().at("revision");return call(req);};
  write({{"op","model.box"},{"size",{10,10,10}}});
  write({{"op","model.fillet"},{"edge",scene()["data"]["edges"][0]["reference"]},{"radius",1}});
  auto fs=scene()["data"]["features"];const auto before=counts->boxes;
  write({{"op","feature.edit"},{"feature",fs[1]["id"]},{"patch",{{"radius",2}}}});
  require(counts->boxes==before,"unaffected Box was rebuilt");require(counts->fillets==2,"changed Fillet did not recompute exactly once");
  write({{"op","feature.edit"},{"feature",fs[1]["id"]},{"patch",{{"name","Renamed"}}}});
  require(counts->boxes==before && counts->fillets==2,"rename recalculated geometry");
  for(const auto* op:{"document.undo","document.undo","document.redo","document.redo"})write({{"op",op}});
  require(counts->boxes==before && counts->fillets==2,"Undo/Redo replayed a kernel operation");
  write({{"op","feature.edit"},{"feature",fs[0]["id"]},{"patch",{{"size",{20,10,10}}}}});
  require(counts->boxes==before+1 && counts->fillets==3,"upstream edit did not rebuild affected dependency chain");
  std::cout<<"PASS immutable Undo/Redo, metadata edits reuse cache, downstream-only and upstream invalidation\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
