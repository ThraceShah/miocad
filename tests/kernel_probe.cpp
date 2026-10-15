#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmApiBrep.h>
#include <SmApiQueries.h>
#include <SmBrep.h>
#include <SmPoly.h>
#include <cmath>
#include <iostream>
#include <memory>
#include <stdexcept>
using Body = std::unique_ptr<SmBrep>;
void require(bool value, const char* what) { if (!value) throw std::runtime_error(what); }
void check(SmApiStatus s) { if(s != SM_SUCCESS) throw std::runtime_error("kernel status " + std::to_string(s)); }
Body box(double x=0) { SmBrep* p=nullptr; auto s=SmApiCreateBox(SmVector3d(x,0,0),10,10,10,p); Body b(p); check(s); return b; }
double volume(SmBrep* b) { double v=0; check(SmApiBrepComputeVolume(b,1e-7,v)); return v; }
void valid(SmBrep* b) { SmBoolean m=FALSE; check(SmApiBrepIsManifoldSolid(b,m)); require(m && b->AssertValid(), "invalid BRep"); }
int main() try {
  SmApiCreateContext();
  auto a=box(); valid(a.get()); require(std::abs(volume(a.get())-1000)<1e-5,"box volume");
  SmTArray<SmEdge*> edges; check(SmApiGetEdges(a.get(),edges)); require(edges.GetSize()==12,"box edges");
  std::cout << "box: volume=1000 edges=12 valid=true\n";
  for (int operation=0; operation<3; ++operation) {
    auto lhs=box(), rhs=box(5); SmBrep* out=nullptr;
    auto status=operation==0 ? SmApiBooleanUnion(lhs.get(),rhs.get(),out) : operation==1 ? SmApiBooleanDifference(lhs.get(),rhs.get(),out) : SmApiBooleanIntersection(lhs.get(),rhs.get(),out);
    if(status==SM_SUCCESS) rhs.release(); // Successful boolean deletes operand B.
    check(status); require(out==lhs.get(),"boolean ownership"); valid(out);
    double expected=operation==0 ? 1500 : 500;
    require(std::abs(volume(out)-expected)<1e-4,"boolean volume");
    std::cout << "boolean " << operation << ": volume=" << volume(out) << " valid=true\n";
  }
  SmBrep* copy=nullptr; check(SmApiBrepCopy(a.get(),copy)); Body candidate(copy);
  check(SmApiGetEdges(copy,edges)); SmTArray<SmEdge*> selected; selected.Add(edges[0]);
  check(SmApiFilletEdges(copy,selected,1.0,1,1,1.0)); valid(copy);
  const double expected=1000-(1-std::acos(-1.0)/4)*10;
  require(std::abs(volume(copy)-expected)<1e-4,"fillet analytic volume");
  require(std::abs(volume(a.get())-1000)<1e-5,"source changed");
  std::cout << "fillet: volume=" << volume(copy) << " valid=true source_unchanged=true\n";
  SmPolyBrep* rawMesh=nullptr; check(SmApiTessellate(copy,rawMesh)); std::unique_ptr<SmPolyBrep> mesh(rawMesh);
  SmTArray<SmPoint3d> points; mesh->GetPolyPoints(points); require(points.GetSize()>8,"mesh empty");
  std::cout << "tessellation: vertices=" << points.GetSize() << "\nSTEP: unsupported by official SM_API; see docs/investigation.md\n";
  return 0;
} catch(const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
