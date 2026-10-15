#include "kernel.h"
#include <BRepAdaptor_Curve.hxx>
#include <BRepBndLib.hxx>
#include <BRepBuilderAPI_Copy.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepFilletAPI_MakeFillet.hxx>
#include <BRepGProp.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepTools.hxx>
#include <BRep_Tool.hxx>
#include <Bnd_Box.hxx>
#include <DESTEP_Parameters.hxx>
#include <GCPnts_TangentialDeflection.hxx>
#include <GProp_GProps.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <Poly_Triangulation.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_Writer.hxx>
#include <Standard_Failure.hxx>
#include <Standard_Version.hxx>
#include <StepData_StepModel.hxx>
#include <TopExp.hxx>
#include <TopExp_Explorer.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Solid.hxx>
#include <cmath>
#include <stdexcept>
#include <utility>

#if OCC_VERSION_HEX < 0x070900
#error "AI-CAD OCCT adapter requires OCCT >= 7.9"
#endif
namespace cad {
namespace {
template<class F> auto checked(F&& call) {
  try { return call(); }
  catch(const Standard_Failure& e) {
    throw std::runtime_error(std::string("OCCT: ")+(e.GetMessageString()?e.GetMessageString():"operation failed"));
  }
}
void require(bool condition, const char* message) { if(!condition) throw std::runtime_error(message); }
Point point(const gp_Pnt& p) { return {p.X(),p.Y(),p.Z()}; }
TopTools_IndexedMapOfShape entities(const TopoDS_Shape& body, TopAbs_ShapeEnum type) {
  TopTools_IndexedMapOfShape result; TopExp::MapShapes(body,type,result); return result;
}
TopoDS_Shape copy(const TopoDS_Shape& body) {
  BRepBuilderAPI_Copy operation(body,Standard_True,Standard_False);
  require(operation.IsDone(),"OCCT deep copy failed"); return operation.Shape();
}
// Accept exactly one solid (possibly inside a compound) and no free components.
// Reject assemblies/mixed bodies rather than silently discarding geometry.
TopoDS_Shape single_solid(const TopoDS_Shape& shape) {
  require(!shape.IsNull(),"STEP contains no shape");
  auto solids=entities(shape,TopAbs_SOLID);
  require(solids.Extent()==1,"STEP import requires exactly one solid");
  const auto& solid=solids(1);
  for(auto type:{TopAbs_FACE,TopAbs_EDGE,TopAbs_VERTEX}) {
    const auto all=entities(shape,type), owned=entities(solid,type);
    for(int i=1;i<=all.Extent();++i) require(owned.Contains(all(i)),"STEP contains unsupported free geometry");
  }
  return solid;
}
class OcctShape final : public Shape {
  TopoDS_Shape body;
public:
  explicit OcctShape(TopoDS_Shape shape) : body(std::move(shape)) {}
  std::unique_ptr<Shape> clone() const override {
    return checked([&]() -> std::unique_ptr<Shape> { return std::make_unique<OcctShape>(copy(body)); });
  }
  State state() const override {
    return checked([&] {
      auto faces=entities(body,TopAbs_FACE), edges=entities(body,TopAbs_EDGE), solids=entities(body,TopAbs_SOLID);
      GProp_GProps props;
      // Integrate rational B-spline knot spans explicitly; the ordinary adaptive
      // Gauss path underestimates the rational cylinder fixture at this tolerance.
      const double integration_error=BRepGProp::VolumePropertiesGK(body,props,1e-9,Standard_True,Standard_True);
      require(std::isfinite(integration_error) && integration_error>=0,"OCCT volume integration failed");
      double volume=props.Mass();
      bool valid=solids.Extent()==1 && std::isfinite(volume) && volume>0 && BRepCheck_Analyzer(body,Standard_True,Standard_False,Standard_True).IsValid();
      Bnd_Box bounds; BRepBndLib::AddOptimal(body,bounds,Standard_False,Standard_False);
      double x,y,z,X,Y,Z; bounds.Get(x,y,z,X,Y,Z);
      return State{volume,static_cast<std::size_t>(faces.Extent()),static_cast<std::size_t>(edges.Extent()),valid,{x,y,z},{X,Y,Z}};
    });
  }
  std::vector<Edge> edges() const override {
    return checked([&] {
      auto map=entities(body,TopAbs_EDGE); std::vector<Edge> result;
      for(int i=1;i<=map.Extent();++i) {
        auto edge=TopoDS::Edge(map(i)); Edge e{};
        GProp_GProps props; BRepGProp::LinearProperties(edge,props); e.length=props.Mass();
        if(BRep_Tool::Degenerated(edge)) {
          auto vertices=entities(edge,TopAbs_VERTEX);
          if(vertices.Extent()) e.points.push_back(point(BRep_Tool::Pnt(TopoDS::Vertex(vertices(1)))));
        } else {
          BRepAdaptor_Curve curve(edge);
          GCPnts_TangentialDeflection sample(curve,0.0872664626,0.01,2);
          for(int j=1;j<=sample.NbPoints();++j) e.points.push_back(point(sample.Value(j)));
        }
        result.push_back(std::move(e));
      }
      return result;
    });
  }
  void fillet(std::size_t index,double radius) override {
    checked([&] {
      auto map=entities(body,TopAbs_EDGE);
      if(index>=static_cast<std::size_t>(map.Extent())) throw std::invalid_argument("edge does not exist");
      if(!std::isfinite(radius) || radius<=0) throw std::invalid_argument("radius must be positive and finite");
      BRepFilletAPI_MakeFillet operation(body);
      operation.Add(radius,TopoDS::Edge(map(static_cast<int>(index)+1))); operation.Build();
      require(operation.IsDone(),"OCCT fillet failed");
      body=operation.Shape();
    });
  }
  Mesh mesh() const override {
    return checked([&] {
      // Meshing attaches data to shared TShapes; never mesh an immutable history body.
      auto source=copy(body);
      BRepMesh_IncrementalMesh mesher(source,0.01,Standard_False,0.2617993878,Standard_False);
      require(mesher.IsDone(),"OCCT tessellation failed");
      auto faces=entities(source,TopAbs_FACE); Mesh result;
      for(int f=1;f<=faces.Extent();++f) {
        auto face=TopoDS::Face(faces(f)); TopLoc_Location location;
        auto triangles=BRep_Tool::Triangulation(face,location);
        require(!triangles.IsNull() && triangles->NbTriangles()>0,"OCCT face has no triangulation");
        auto offset=result.points.size();
        for(int i=1;i<=triangles->NbNodes();++i) result.points.push_back(point(triangles->Node(i).Transformed(location.Transformation())));
        for(int i=1;i<=triangles->NbTriangles();++i) {
          int a,b,c; triangles->Triangle(i).Get(a,b,c);
          if(face.Orientation()==TopAbs_REVERSED) std::swap(b,c);
          result.triangles.push_back({{offset+a-1,offset+b-1,offset+c-1},static_cast<std::size_t>(f-1)});
        }
      }
      return result;
    });
  }
  void save_native(const std::string& path) const override {
    checked([&] { require(BRepTools::Write(body,path.c_str()),"OCCT BRep write failed"); });
  }
  void save_step(const std::string& path) const override {
    checked([&] {
      STEPControl_Writer writer;
      DESTEP_Parameters params; params.WriteUnit=UnitsMethods_LengthUnit_Millimeter;
      writer.Model()->SetLocalLengthUnit(1.0); // input coordinates are mm
      require(writer.Transfer(copy(body),STEPControl_ManifoldSolidBrep,params)==IFSelect_RetDone,"STEP transfer failed");
      require(writer.Write(path.c_str())==IFSelect_RetDone,"STEP write failed");
    });
  }
};
class OcctKernel final : public IKernel {
public:
  OcctKernel() {
    checked([] {
      // Keep the JSONL transport stdout clean; OCCT transfer diagnostics go to stderr.
      auto messenger=Message::DefaultMessenger(); messenger->ChangePrinters().Clear();
      Handle(Message_PrinterOStream) printer=new Message_PrinterOStream("cerr",Standard_False);
      printer->SetToColorize(Standard_False); messenger->AddPrinter(printer);
    });
  }
  std::string name() const override { return "occt"; }
  std::string native_format() const override { return "brep"; }
  bool supports_step() const override { return true; }
  std::unique_ptr<Shape> box(Point size) override {
    return checked([&]() -> std::unique_ptr<Shape> {
      for(double d:size) if(!std::isfinite(d) || d<=0 || d>1e6) throw std::invalid_argument("box dimensions must be finite in (0,1000000] mm");
      return std::make_unique<OcctShape>(BRepPrimAPI_MakeBox(size[0],size[1],size[2]).Shape());
    });
  }
  std::unique_ptr<Shape> import_step(const std::string& path) override {
    return checked([&]() -> std::unique_ptr<Shape> {
      STEPControl_Reader reader;
      require(reader.ReadFile(path.c_str())==IFSelect_RetDone,"STEP read failed");
      reader.SetSystemLengthUnit(1.0); // target mm, independent of input file unit
      const int roots=reader.NbRootsForTransfer();
      require(roots>0,"STEP contains no transferable roots");
      require(reader.TransferRoots()==roots,"STEP transfer failed for one or more roots");
      return std::make_unique<OcctShape>(single_solid(reader.OneShape()));
    });
  }
};
}
std::unique_ptr<IKernel> make_occt_kernel() { return std::make_unique<OcctKernel>(); }
}
