#pragma once
#include <array>
#include <memory>
#include <string>
#include <stdexcept>
#include <vector>
namespace cad {
using Point = std::array<double,3>;
struct State { double volume; std::size_t faces, edges; bool valid; };
struct Edge { std::vector<Point> points; double length; };
struct Triangle { std::array<std::size_t,3> vertices; std::size_t face; };
struct Mesh { std::vector<Point> points; std::vector<Triangle> triangles; };
class Shape {
public:
  virtual ~Shape() = default;
  virtual std::unique_ptr<Shape> clone() const = 0;
  virtual State state() const = 0;
  virtual std::vector<Edge> edges() const = 0;
  virtual void fillet(std::size_t edge, double radius) = 0;
  virtual Mesh mesh() const = 0;
  virtual void save_native(const std::string& path) const = 0;
};
class IKernel {
public:
  virtual ~IKernel() = default;
  virtual std::unique_ptr<Shape> box(Point size) = 0;
  virtual std::unique_ptr<Shape> load_native(const std::string&) { throw std::runtime_error("native loading unsupported"); }
};
std::unique_ptr<IKernel> make_smlib_kernel();
}
