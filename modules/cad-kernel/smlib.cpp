#include "kernel.h"
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmApiQueries.h>
#include <SmApiBrep.h>
#include <SmApiImportExport.h>
#include <SmBrep.h>
#include <SmPoly.h>
#include <cmath>
#include <optional>
#include <stdexcept>
#include <unordered_map>
namespace cad {
namespace {
void check(SmApiStatus s) { if(s != SM_SUCCESS) throw std::runtime_error("SMLib status " + std::to_string(s)); }
Point point(const SmPoint3d& p) { return {p.x,p.y,p.z}; }
class SmlibShape final : public Shape {
  std::unique_ptr<SmBrep> body;
  mutable std::optional<State> cached_state;
public:
  explicit SmlibShape(SmBrep* p) : body(p) {}
  std::unique_ptr<Shape> clone() const override {
    SmBrep* raw=nullptr; auto s=SmApiBrepCopy(body.get(),raw);
    auto result=std::make_unique<SmlibShape>(raw); check(s); return result;
  }
  State state() const override {
    if(cached_state)return *cached_state;
    SmBoolean manifold=FALSE; double volume=0;
    check(SmApiBrepIsManifoldSolid(body.get(),manifold));
    check(SmApiBrepComputeVolume(body.get(),1e-7,volume));
    SmTArray<SmFace*> faces; SmTArray<SmEdge*> edges;
    check(SmApiGetFaces(body.get(),faces)); check(SmApiGetEdges(body.get(),edges));
    cached_state=State{volume,faces.GetSize(),edges.GetSize(),bool(manifold && std::isfinite(volume) && volume>0 && body->AssertValid())};
    return *cached_state;
  }
  std::vector<Edge> edges() const override {
    SmTArray<SmPoint3d> points; SmTArray<ULONG> counts; SmTArray<SmEdge*> edges;
    check(SmApiTessellateBoundaries(body.get(),5,points,counts,edges));
    std::vector<Edge> result; std::size_t offset=0;
    for(ULONG i=0;i<edges.GetSize();++i) {
      Edge e; check(SmApiEdgeComputeLength(edges[i],1e-7,e.length));
      for(ULONG j=0;j<counts[i];++j) e.points.push_back(point(points[offset++]));
      result.push_back(std::move(e));
    }
    return result;
  }
  void fillet(std::size_t index, double radius) override {
    cached_state.reset();
    SmTArray<SmEdge*> edges; check(SmApiGetEdges(body.get(),edges));
    if(index>=edges.GetSize()) throw std::invalid_argument("edge does not exist");
    if(!std::isfinite(radius) || radius<=0) throw std::invalid_argument("radius must be positive and finite");
    SmTArray<SmEdge*> selected; selected.Add(edges[index]);
    check(SmApiFilletEdges(body.get(),selected,radius,1,1,1.0));
  }
  Mesh mesh() const override {
    SmPolyBrep* raw=nullptr; SmTessellationParams params; params.dSurfaceAngleTolDeg=15;
    auto status=SmApiTessellate(body.get(),raw,params);
    std::unique_ptr<SmPolyBrep> poly(raw); check(status);
    SmTArray<SmPolyVertex*> vertices; SmTArray<SmPolyFace*> polygons; SmTArray<SmFace*> faces;
    poly->GetPolyVertices(vertices); poly->GetPolyFaces(polygons); body->GetFaces(faces);
    Mesh result; std::unordered_map<SmPolyVertex*,std::size_t> ids;
    std::unordered_map<SmFace*,std::size_t> face_ids;
    for(ULONG i=0;i<faces.GetSize();++i) face_ids[faces[i]]=i;
    for(ULONG i=0;i<vertices.GetSize();++i) { ids[vertices[i]]=i; result.points.push_back(point(vertices[i]->GetPoint())); }
    for(auto* polygon:polygons) {
      auto* loop=polygon->GetOuterPolyLoop();
      if(!loop) throw std::runtime_error("mesh polygon has no loop");
      SmTArray<SmPolyEdge*> edges; loop->GetPolyEdges(edges);
      if(edges.GetSize()!=3) throw std::runtime_error("expected triangulated mesh");
      Triangle t{{ids.at(edges[0]->GetStartPolyVertex()),ids.at(edges[1]->GetStartPolyVertex()),ids.at(edges[2]->GetStartPolyVertex())},face_ids.at(polygon->GetOriginalFace())};
      result.triangles.push_back(t);
    }
    return result;
  }
  void save_native(const std::string& path) const override { check(SmApiWriteBrepToFile(body.get(),path.c_str(),TRUE)); }
};
class SmlibKernel final : public IKernel {
public:
  SmlibKernel() { SmApiCreateContext(); }
  std::unique_ptr<Shape> load_native(const std::string& path) override {
    SmBrep* raw=nullptr; auto status=SmApiReadBrepFromFile(path.c_str(),TRUE,FALSE,raw);
    auto result=std::make_unique<SmlibShape>(raw); check(status);
    if(!raw || !result->state().valid) throw std::runtime_error("invalid saved B-Rep");
    return result;
  }
  std::unique_ptr<Shape> box(Point size) override {
    for(double d:size) if(!std::isfinite(d) || d<=0 || d>1e6) throw std::invalid_argument("box dimensions must be finite in (0,1000000] mm");
    SmBrep* raw=nullptr; auto s=SmApiCreateBox(SmVector3d(0,0,0),size[0],size[1],size[2],raw);
    auto result=std::make_unique<SmlibShape>(raw); check(s); return result;
  }
};
}
std::unique_ptr<IKernel> make_smlib_kernel() { return std::make_unique<SmlibKernel>(); }
}
