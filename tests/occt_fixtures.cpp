// Generate analytic/periodic/NURBS and deliberately unsupported STEP fixtures.
// Uses OCCT directly, bypassing the CAD adapter and document pipeline.
#include <BRepBuilderAPI_MakeFace.hxx>
#include <BRepBuilderAPI_MakePolygon.hxx>
#include <BRepBuilderAPI_NurbsConvert.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRepPrimAPI_MakeCylinder.hxx>
#include <BRepPrimAPI_MakeSphere.hxx>
#include <BRepPrimAPI_MakeTorus.hxx>
#include <BRep_Builder.hxx>
#include <STEPControl_Writer.hxx>
#include <TopoDS_Compound.hxx>
#include <gp_Pln.hxx>
#include <filesystem>
#include <iostream>
#include <stdexcept>
int main(int argc,char** argv) try {
  if(argc!=2) throw std::invalid_argument("fixture output directory required");
  auto root=std::filesystem::path(argv[1]); std::filesystem::create_directories(root);
  auto write=[&](const char* name,const TopoDS_Shape& shape) {
    STEPControl_Writer writer;
    if(writer.Transfer(shape,STEPControl_AsIs)!=IFSelect_RetDone || writer.Write((root/name).c_str())!=IFSelect_RetDone)
      throw std::runtime_error("fixture STEP write failed");
  };
  auto cylinder=BRepPrimAPI_MakeCylinder(2,5).Shape();
  write("cylinder.step",cylinder);
  write("sphere.step",BRepPrimAPI_MakeSphere(2).Shape());
  write("torus.step",BRepPrimAPI_MakeTorus(5,1).Shape());
  write("nurbs.step",BRepBuilderAPI_NurbsConvert(cylinder,Standard_True).Shape());
  auto face=BRepBuilderAPI_MakeFace(gp_Pln(gp_Pnt(0,0,0),gp_Dir(0,0,1)),0,1,0,1).Shape();
  write("sheet.step",face);
  BRep_Builder builder; TopoDS_Compound multiple; builder.MakeCompound(multiple);
  builder.Add(multiple,BRepPrimAPI_MakeBox(2,3,4).Shape());
  builder.Add(multiple,BRepPrimAPI_MakeBox(gp_Pnt(10,0,0),2,3,4).Shape());
  write("multiple.step",multiple);
  TopoDS_Compound mixed; builder.MakeCompound(mixed);
  builder.Add(mixed,cylinder); builder.Add(mixed,face); write("mixed.step",mixed);
  return 0;
} catch(const std::exception& e) { std::cerr<<e.what()<<'\n'; return 1; }
