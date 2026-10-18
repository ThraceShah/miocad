#include "commander.h"
#include <cmath>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iostream>
#include <limits>
#include <set>
#include <stdexcept>
#include <vector>

namespace {
using cad::Json;
void require(bool condition, const std::string& message) {
    if (!condition) throw std::runtime_error(message);
}
Json call(cad::Commander& commander, Json request) {
    auto response = commander.execute(request);
    require(response.at("ok") == true, response.dump());
    return response.at("result");
}
void error(cad::Commander& commander, Json request, const std::string& kind) {
    auto response = commander.execute(request);
    require(response.at("ok") == false, "expected failure: " + response.dump());
    require(response.at("error").at("code") == "command_failed", "legacy error code changed");
    require(response.at("error").at("kind") == kind, response.dump());
    require(!response.at("error").at("message").get<std::string>().empty(), "missing diagnostic");
    if (request.is_object() && request.contains("id")) require(response.at("id") == request["id"], "error id lost");
}
std::string create(cad::Commander& commander) {
    return call(commander, {{"op", "document.create"}}).at("document");
}
Json state(cad::Commander& commander, const std::string& doc) {
    return call(commander, {{"op", "document.state"}, {"document", doc}});
}
void box(cad::Commander& commander, const std::string& doc, unsigned revision, cad::Point size = {10,10,10}) {
    call(commander, {{"op", "model.box"}, {"document", doc}, {"revision", revision}, {"size", size}});
}
std::string edge(cad::Commander& commander, const std::string& doc) {
    return call(commander, {{"op", "topology.edges"}, {"document", doc}}).at("data").at(0).at("reference");
}

void catalog_contract() {
    cad::Commander commander;
    const auto catalog = commander.commands();
    std::set<std::string> names;
    for (const auto& spec : catalog) {
        require(names.insert(spec.at("id")).second, "duplicate command id");
        require(spec.at("input_schema").at("type") == "object", "missing input schema");
        require(spec.at("output_schema").is_object() && !spec.at("output_schema").empty(), "missing output schema");
        require(spec.at("preconditions").is_array(), "missing preconditions");
        require(spec.at("effect").is_string(), "missing effect");
    }
    const std::set<std::string> expected = {"capabilities", "commands.list", "document.create", "document.state",
        "document.undo", "document.redo", "model.box", "model.fillet", "topology.edges", "render.mesh", "export.native"};
    require(names == expected, "catalog does not describe all core commands");
    require(call(commander, {{"op", "commands.list"}}) == catalog, "discovery differs from native catalog");
}

void real_kernel_workflow() {
    cad::Commander commander;
    const auto doc = create(commander), other = create(commander);
    box(commander, doc, 0);
    auto before = state(commander, doc);
    require(before["revision"] == 1 && before["data"]["volume"] == 1000, "box state changed");
    const auto selected = edge(commander, doc);
    const auto response = commander.execute({{"id", 42}, {"op", "model.fillet"}, {"document", doc}, {"revision", 1}, {"edge", selected}, {"radius", 1}});
    require(response["ok"] == true && response["id"] == 42, response.dump());
    const double expected = 1000 - 10 * (1 - std::acos(-1.0) / 4);
    require(std::abs(response["result"]["data"]["volume"].get<double>() - expected) < 1e-4, "fillet analytic volume");
    require(response["result"]["data"]["valid"] == true, "invalid fillet");
    auto mesh = call(commander, {{"op", "render.mesh"}, {"document", doc}})["data"];
    require(mesh["triangles"].size() > 12 && mesh["points"].size() > 8, "empty mesh");
    const auto after = state(commander, doc);
    require(after["revision"] == 2, "query changed revision");
    call(commander, {{"op", "document.undo"}, {"document", doc}, {"revision", 2}});
    require(state(commander, doc)["data"]["volume"] == 1000, "undo did not restore box");
    error(commander, {{"op", "model.fillet"}, {"document", doc}, {"revision", 3}, {"edge", selected}, {"radius", 1}}, "invalid_reference");
    call(commander, {{"op", "document.redo"}, {"document", doc}, {"revision", 3}});
    require(state(commander, doc)["data"]["volume"] == after["data"]["volume"], "redo failed");
    require(state(commander, other)["data"]["empty"] == true, "documents leaked");
    cad::Commander isolated;
    error(isolated, {{"op", "document.state"}, {"document", doc}}, "document_not_found");
}

void validation_and_error_contract() {
    cad::Commander commander;
    error(commander, Json::array(), "invalid_request");
    error(commander, {{"op", 12}}, "invalid_request");
    error(commander, {{"op", "unknown"}, {"id", "request-a"}}, "unknown_command");
    const auto doc = create(commander);
    error(commander, {{"op", "topology.edges"}, {"document", doc}}, "empty_document");
    error(commander, {{"op", "document.undo"}, {"document", doc}, {"revision", 0}}, "history_unavailable");
    box(commander, doc, 0);
    const auto before = state(commander, doc);
    for (Json revision : std::vector<Json>{-1, 1.5, true, "1"})
        error(commander, {{"op", "model.box"}, {"document", doc}, {"revision", revision}, {"size", {2,3,4}}}, "invalid_params");
    error(commander, {{"op", "model.box"}, {"document", doc}, {"revision", 0}, {"size", {2,3,4}}}, "revision_conflict");
    error(commander, {{"op", "model.box"}, {"document", doc}, {"revision", 1}, {"size", {2,3}}}, "invalid_params");
    error(commander, {{"op", "model.box"}, {"document", doc}, {"revision", 1}, {"size", {2,3,4,5}}}, "invalid_params");
    error(commander, {{"op", "model.box"}, {"document", doc}, {"revision", 1}, {"size", {0,3,4}}}, "invalid_params");
    for (double radius : {-1.0, std::numeric_limits<double>::infinity(), std::numeric_limits<double>::quiet_NaN()})
        error(commander, {{"op", "model.fillet"}, {"document", doc}, {"revision", 1}, {"edge", edge(commander, doc)}, {"radius", radius}}, "invalid_params");
    require(state(commander, doc) == before, "invalid request changed document");
}

struct Faults {
    enum Mode { None, KernelFailure, InvalidCandidate, ResponseFailure } mode = None;
    int state_calls = 0;
};
class FaultShape final : public cad::Shape {
    std::shared_ptr<Faults> faults_;
    double volume_;
    bool valid_ = true;
public:
    FaultShape(std::shared_ptr<Faults> faults, double volume) : faults_(std::move(faults)), volume_(volume) {}
    std::unique_ptr<cad::Shape> clone() const override { return std::make_unique<FaultShape>(*this); }
    cad::State state() const override {
        if (faults_->mode == Faults::ResponseFailure && ++faults_->state_calls == 2)
            throw std::runtime_error("injected query failure after candidate validation");
        return {volume_, 1, 1, valid_};
    }
    std::vector<cad::Edge> edges() const override { return {{{{0,0,0},{1,0,0}}, 1}}; }
    void fillet(std::size_t, double) override {
        volume_ -= 1;
        if (faults_->mode == Faults::KernelFailure) throw std::runtime_error("injected mutating kernel failure");
        if (faults_->mode == Faults::InvalidCandidate) valid_ = false;
    }
    cad::Mesh mesh() const override { return {}; }
    void save_native(const std::string& path) const override {
        std::ofstream(path) << "partial export";
        throw std::runtime_error("injected export failure");
    }
};
class FaultKernel final : public cad::IKernel {
    std::shared_ptr<Faults> faults_;
public:
    explicit FaultKernel(std::shared_ptr<Faults> faults) : faults_(std::move(faults)) {}
    std::unique_ptr<cad::Shape> box(cad::Point size) override {
        return std::make_unique<FaultShape>(faults_, size[0]*size[1]*size[2]);
    }
};
void rollback_contract(Faults::Mode mode, const std::string& kind) {
    auto faults = std::make_shared<Faults>();
    cad::Commander commander(std::make_unique<FaultKernel>(faults));
    const auto doc = create(commander);
    box(commander, doc, 0);
    box(commander, doc, 1, {2,2,2});
    call(commander, {{"op", "document.undo"}, {"document", doc}, {"revision", 2}});
    const auto before = state(commander, doc);
    const auto selected = edge(commander, doc);
    faults->mode = mode;
    if (mode == Faults::ResponseFailure)
        error(commander, {{"op", "model.box"}, {"document", doc}, {"revision", 3}, {"size", {3,3,3}}}, kind);
    else
        error(commander, {{"op", "model.fillet"}, {"document", doc}, {"revision", 3}, {"edge", selected}, {"radius", 1}}, kind);
    faults->mode = Faults::None;
    require(state(commander, doc) == before, "failed command changed body/revision/history");
    call(commander, {{"op", "document.redo"}, {"document", doc}, {"revision", 3}});
    require(state(commander, doc)["data"]["volume"] == 8, "failed command destroyed redo branch");
}
void history_response_rollback() {
    auto faults = std::make_shared<Faults>();
    cad::Commander commander(std::make_unique<FaultKernel>(faults));
    const auto doc = create(commander);
    box(commander, doc, 0);
    box(commander, doc, 1, {2,2,2});
    for (const auto& operation : {"document.undo", "document.redo"}) {
        const auto before = state(commander, doc);
        faults->state_calls = 1;
        faults->mode = Faults::ResponseFailure;
        error(commander, {{"op", operation}, {"document", doc}, {"revision", before["revision"]}}, "kernel_error");
        faults->mode = Faults::None;
        require(state(commander, doc) == before, "failed history response changed document");
        call(commander, {{"op", operation}, {"document", doc}, {"revision", before["revision"]}});
    }
    require(state(commander, doc)["data"]["volume"] == 8, "history was not recoverable");
}
void export_contract() {
    cad::Commander commander;
    const auto doc = create(commander);
    box(commander, doc, 0);
    const auto before = state(commander, doc);
    auto root = std::filesystem::temp_directory_path() / ("miocad-commander-" + doc);
    std::filesystem::create_directory(root);
    struct Cleanup { std::filesystem::path path; ~Cleanup() { std::error_code ignored; std::filesystem::remove_all(path, ignored); } } cleanup{root};
    const auto path = (root / "body.smb").string();
    call(commander, {{"op", "export.native"}, {"document", doc}, {"path", path}});
    require(std::filesystem::file_size(path) > 100, "native file missing");
    auto read = [](const std::string& filename) {
        std::ifstream input(filename, std::ios::binary);
        return std::string(std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>());
    };
    const auto original = read(path);
    error(commander, {{"op", "export.native"}, {"document", doc}, {"path", path + std::string(1,'\0') + ".smb"}}, "invalid_params");
    error(commander, {{"op", "export.native"}, {"document", doc}, {"path", (root/"missing"/"body.smb").string()}}, "io_error");
    require(state(commander, doc) == before, "export changed document");
    auto faults = std::make_shared<Faults>();
    cad::Commander failing(std::make_unique<FaultKernel>(faults));
    const auto failing_doc = create(failing);
    box(failing, failing_doc, 0);
    error(failing, {{"op", "export.native"}, {"document", failing_doc}, {"path", path}}, "io_error");
    require(read(path) == original, "failed export replaced existing file");
    for (const auto& entry : std::filesystem::directory_iterator(root))
        require(entry.path().extension() == ".smb", "temporary file leaked");
}
}
int main() {
    const std::vector<std::pair<std::string,std::function<void()>>> tests = {
        {"catalog", catalog_contract}, {"real kernel workflow", real_kernel_workflow},
        {"validation and errors", validation_and_error_contract},
        {"mutating kernel failure", []{ rollback_contract(Faults::KernelFailure,"kernel_error"); }},
        {"candidate validation failure", []{ rollback_contract(Faults::InvalidCandidate,"validation_failed"); }},
        {"response generation failure", []{ rollback_contract(Faults::ResponseFailure,"kernel_error"); }},
        {"history response rollback", history_response_rollback}, {"native export", export_contract}
    };
    int failures = 0;
    for (const auto& test : tests) try {
        test.second(); std::cout << "PASS " << test.first << '\n';
    } catch (const std::exception& e) {
        ++failures; std::cerr << "FAIL " << test.first << ": " << e.what() << '\n';
    }
    return failures ? 1 : 0;
}
