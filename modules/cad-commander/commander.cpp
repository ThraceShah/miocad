#include "commander.h"
#include "commands.h"
#include "reference.h"
#include <algorithm>
#include <cmath>
#include <filesystem>
#include <map>
#include <optional>
#include <random>
#include <sstream>
#include <stdexcept>
#include <utility>

namespace cad {
namespace {
class CommandError : public std::runtime_error {
public:
    std::string kind;
    CommandError(std::string category, const std::string& message)
        : std::runtime_error(message), kind(std::move(category)) {}
};

std::string new_id() {
    std::random_device random;
    std::ostringstream result;
    result << std::hex;
    for (int i = 0; i < 4; ++i) result << random() << '-';
    return result.str();
}

Json document_state(const Document& document) {
    Json result = {{"revision", document.revision()}, {"units", "mm"}, {"empty", !document.shape()}};
    if (document.shape()) {
        const auto state = document.shape()->state();
        result.update({{"volume", state.volume}, {"faces", state.faces}, {"edges", state.edges}, {"valid", state.valid}});
    }
    return result;
}

// Validation of the small schema subset used by this registry. This is not a
// general JSON Schema engine; the cross-language schema contract belongs to P2.
void validate(const Json& value, const Json& schema, const std::string& field) {
    const auto type = schema.value("type", std::string{});
    bool correct = true;
    if (type == "object") correct = value.is_object();
    else if (type == "array") correct = value.is_array();
    else if (type == "string") correct = value.is_string();
    else if (type == "integer") correct = value.is_number_integer();
    else if (type == "number") correct = value.is_number();
    else if (type == "boolean") correct = value.is_boolean();
    if (!correct) throw CommandError("invalid_params", field + " must be " + type);
    if (schema.contains("const") && value != schema["const"])
        throw CommandError("invalid_params", field + " has an unexpected value");
    if (value.is_number()) {
        if (!std::isfinite(value.get<double>())) throw CommandError("invalid_params", field + " must be finite");
        if (schema.contains("minimum") && value < schema["minimum"])
            throw CommandError("invalid_params", field + " is below its minimum");
        if (schema.contains("maximum") && value > schema["maximum"])
            throw CommandError("invalid_params", field + " exceeds its maximum");
        if (schema.contains("exclusiveMinimum") && value <= schema["exclusiveMinimum"])
            throw CommandError("invalid_params", field + " must be positive");
    }
    if (value.is_object()) {
        if (schema.contains("required")) for (const auto& name : schema["required"])
            if (!value.contains(name.get<std::string>()))
                throw CommandError("invalid_params", field + "." + name.get<std::string>() + " is required");
        if (schema.contains("properties")) for (auto it = schema["properties"].begin(); it != schema["properties"].end(); ++it)
            if (value.contains(it.key())) validate(value[it.key()], it.value(), field + "." + it.key());
    }
    if (value.is_array()) {
        if ((schema.contains("minItems") && value.size() < schema["minItems"].get<std::size_t>()) ||
            (schema.contains("maxItems") && value.size() > schema["maxItems"].get<std::size_t>()))
            throw CommandError("invalid_params", field + " has an invalid number of items");
        if (schema.contains("items")) for (std::size_t i = 0; i < value.size(); ++i)
            validate(value[i], schema["items"], field + "[" + std::to_string(i) + "]");
    }
}

Json input_schema(const std::string& id, Json properties = Json::object(), Json required = Json::array()) {
    properties["op"] = {{"type", "string"}, {"const", id}};
    properties["id"] = Json::object();
    required.push_back("op");
    return {{"type", "object"}, {"properties", std::move(properties)}, {"required", std::move(required)}, {"additionalProperties", true}};
}
Json state_schema() {
    return {{"type", "object"}, {"required", {"revision", "units", "empty"}},
        {"properties", {{"revision", {{"type", "integer"}, {"minimum", 0}}}, {"units", {{"const", "mm"}}},
        {"empty", {{"type", "boolean"}}}, {"volume", {{"type", "number"}}},
        {"faces", {{"type", "integer"}}}, {"edges", {{"type", "integer"}}}, {"valid", {{"type", "boolean"}}}}}};
}
Json document_output(Json data) {
    return {{"type", "object"}, {"required", {"document", "revision", "data"}},
        {"properties", {{"document", {{"type", "string"}}}, {"revision", {{"type", "integer"}, {"minimum", 0}}}, {"data", std::move(data)}}}};
}
}

Json command_error(const std::string& kind, const std::string& message) {
    return {{"ok", false}, {"error", {{"code", "command_failed"}, {"kind", kind}, {"message", message}}}};
}

struct Commander::Impl {
    struct Context {
        const Json& request;
        std::string document_id;
        Document* document = nullptr;
    };
    enum class Effect { Query, SessionWrite, DocumentWrite, ExternalWrite };
    enum class History { None, Undo, Redo };
    using Handler = Json (Impl::*)(Context&);
    struct Spec {
        std::string id;
        Effect effect;
        bool document;
        bool body;
        bool revision;
        History history;
        Json input;
        Json output;
        Handler handler;
    };
    std::unique_ptr<IKernel> kernel;
    std::map<std::string, Document> documents;

    explicit Impl(std::unique_ptr<IKernel> backend) : kernel(std::move(backend)) {
        if (!kernel) throw std::invalid_argument("Commander requires a kernel");
    }

    Json capabilities(Context&) {
        return {{"kernel", "smlib"}, {"gpu_picking", false}, {"persistent_naming", false}, {"native_format", "smb"}};
    }
    Json list(Context&) { return catalog(); }
    Json create(Context& context) {
        auto result = document_state(*context.document);
        result["document"] = context.document_id;
        return result;
    }
    Json state(Context& context) { return document_state(*context.document); }
    Json box(Context& context) {
        context.document->commit(kernel->box(context.request.at("size").get<Point>()));
        return document_state(*context.document);
    }
    Json fillet(Context& context) {
        std::size_t selected;
        const auto count = context.document->shape()->state().edges;
        try {
            selected = resolve_edge(context.request.at("edge").get<std::string>(), context.document_id, context.document->revision(), count);
        } catch (const std::invalid_argument& e) { throw CommandError("invalid_reference", e.what()); }
        cad::fillet(*context.document, selected, context.request.at("radius").get<double>());
        return document_state(*context.document);
    }
    Json undo(Context& context) { context.document->undo(); return document_state(*context.document); }
    Json redo(Context& context) { context.document->redo(); return document_state(*context.document); }
    Json edges(Context& context) {
        Json result = Json::array();
        const auto edges = context.document->shape()->edges();
        for (std::size_t i = 0; i < edges.size(); ++i)
            result.push_back({{"reference", token(context.document_id, context.document->revision(), "edge", i)}, {"points", edges[i].points}, {"length", edges[i].length}});
        return result;
    }
    Json mesh(Context& context) {
        const auto mesh = context.document->shape()->mesh();
        Json triangles = Json::array();
        for (const auto& triangle : mesh.triangles)
            triangles.push_back({{"vertices", triangle.vertices}, {"face", token(context.document_id, context.document->revision(), "face", triangle.face)}});
        return {{"points", mesh.points}, {"triangles", triangles}};
    }
    Json export_native(Context& context) {
        const auto path = context.request.at("path").get<std::string>();
        if (path.empty() || path.find('\0') != std::string::npos || std::filesystem::path(path).extension() != ".smb")
            throw CommandError("invalid_params", "native export requires a nonempty .smb path without NUL characters");
        const auto temporary = path + ".tmp-" + new_id();
        try {
            auto parent = std::filesystem::path(path).parent_path();
            if (!parent.empty() && !std::filesystem::is_directory(parent))
                throw CommandError("io_error", "export parent directory does not exist");
            context.document->shape()->save_native(temporary);
            std::filesystem::rename(temporary, path);
        } catch (const std::exception& e) {
            std::error_code ignored;
            std::filesystem::remove(temporary, ignored);
            throw CommandError("io_error", e.what());
        }
        return {{"path", path}, {"format", "smb"}};
    }

    static const std::vector<Spec>& specs() {
        static const auto entries = [] {
            const Json doc = {{"type", "string"}};
            const Json revision = {{"type", "integer"}, {"minimum", 0}};
            const Json doc_properties = {{"document", doc}};
            const Json edit_properties = {{"document", doc}, {"revision", revision}};
            auto box_properties = edit_properties;
            box_properties["size"] = {{"type", "array"}, {"minItems", 3}, {"maxItems", 3},
                {"items", {{"type", "number"}, {"exclusiveMinimum", 0}, {"maximum", 1e6}}}, {"description", "Dimensions in mm"}};
            auto fillet_properties = edit_properties;
            fillet_properties["edge"] = {{"type", "string"}, {"description", "Opaque current-revision edge reference"}};
            fillet_properties["radius"] = {{"type", "number"}, {"exclusiveMinimum", 0}, {"description", "Radius in mm"}};
            auto export_properties = doc_properties;
            export_properties["path"] = {{"type", "string"}, {"description", "Local native .smb output path; parent must exist; no NUL"}};
            auto created = state_schema();
            created["properties"]["document"] = doc;
            created["required"].push_back("document");
            const Json edge_output = {{"type", "array"}, {"items", {{"type", "object"}, {"required", {"reference", "points", "length"}}}}};
            const Json mesh_output = {{"type", "object"}, {"required", {"points", "triangles"}}};
            const Json file_output = {{"type", "object"}, {"required", {"path", "format"}}};
            return std::vector<Spec>{
                {"capabilities", Effect::Query, false, false, false, History::None, input_schema("capabilities"),
                    {{"type", "object"}, {"required", {"kernel", "gpu_picking", "persistent_naming", "native_format"}}}, &Impl::capabilities},
                {"commands.list", Effect::Query, false, false, false, History::None, input_schema("commands.list"), {{"type", "array"}}, &Impl::list},
                {"document.create", Effect::SessionWrite, false, false, false, History::None, input_schema("document.create"), created, &Impl::create},
                {"document.state", Effect::Query, true, false, false, History::None, input_schema("document.state", doc_properties, {"document"}), document_output(state_schema()), &Impl::state},
                {"model.box", Effect::DocumentWrite, true, false, true, History::None, input_schema("model.box", box_properties, {"document", "revision", "size"}), document_output(state_schema()), &Impl::box},
                {"model.fillet", Effect::DocumentWrite, true, true, true, History::None, input_schema("model.fillet", fillet_properties, {"document", "revision", "edge", "radius"}), document_output(state_schema()), &Impl::fillet},
                {"document.undo", Effect::DocumentWrite, true, false, true, History::Undo, input_schema("document.undo", edit_properties, {"document", "revision"}), document_output(state_schema()), &Impl::undo},
                {"document.redo", Effect::DocumentWrite, true, false, true, History::Redo, input_schema("document.redo", edit_properties, {"document", "revision"}), document_output(state_schema()), &Impl::redo},
                {"topology.edges", Effect::Query, true, true, false, History::None, input_schema("topology.edges", doc_properties, {"document"}), document_output(edge_output), &Impl::edges},
                {"render.mesh", Effect::Query, true, true, false, History::None, input_schema("render.mesh", doc_properties, {"document"}), document_output(mesh_output), &Impl::mesh},
                {"export.native", Effect::ExternalWrite, true, true, false, History::None, input_schema("export.native", export_properties, {"document", "path"}), document_output(file_output), &Impl::export_native}
            };
        }();
        return entries;
    }

    static Json catalog() {
        Json result = Json::array();
        for (const auto& spec : specs()) {
            Json preconditions = Json::array();
            if (spec.document) preconditions.push_back("document_exists");
            if (spec.body) preconditions.push_back("nonempty_body");
            if (spec.revision) preconditions.push_back("current_revision");
            if (spec.history == History::Undo) preconditions.push_back("undo_available");
            if (spec.history == History::Redo) preconditions.push_back("redo_available");
            const char* effect = spec.effect == Effect::Query ? "query" : spec.effect == Effect::SessionWrite ? "session_write" :
                spec.effect == Effect::DocumentWrite ? "document_write" : "external_write";
            result.push_back({{"id", spec.id}, {"effect", effect}, {"input_schema", spec.input}, {"output_schema", spec.output}, {"preconditions", preconditions}});
        }
        return result;
    }

    Json execute(const Json& request) {
        if (!request.is_object() || !request.contains("op") || !request["op"].is_string())
            throw CommandError("invalid_request", "request must be an object with a string op");
        const auto operation = request["op"].get<std::string>();
        const auto& entries = specs();
        const auto spec = std::find_if(entries.begin(), entries.end(), [&](const Spec& candidate) { return candidate.id == operation; });
        if (spec == entries.end()) throw CommandError("unknown_command", "unknown operation: " + operation);
        validate(request, spec->input, "request");
        Context context{request, {}, nullptr};
        Document* live = nullptr;
        if (spec->document) {
            context.document_id = request.at("document").get<std::string>();
            const auto it = documents.find(context.document_id);
            if (it == documents.end()) throw CommandError("document_not_found", "unknown document");
            live = &it->second;
            context.document = live;
            if (spec->revision && request.at("revision").get<std::uint64_t>() != live->revision())
                throw CommandError("revision_conflict", "stale revision");
            if (spec->body && !live->shape()) throw CommandError("empty_document", "empty document");
            if ((spec->history == History::Undo && !live->can_undo()) || (spec->history == History::Redo && !live->can_redo()))
                throw CommandError("history_unavailable", spec->history == History::Undo ? "nothing to undo" : "nothing to redo");
        }
        std::optional<Document> staged;
        if (spec->effect == Effect::DocumentWrite) {
            // Snapshots contain shared_ptr<const Shape>; copy history metadata,
            // then change only a deep-copied candidate through modeling commands.
            staged.emplace(*live);
            context.document = &*staged;
        } else if (spec->effect == Effect::SessionWrite) {
            do { context.document_id = new_id(); } while (documents.count(context.document_id));
            staged.emplace();
            context.document = &*staged;
        }
        auto result = (this->*(spec->handler))(context);
        if (spec->document)
            result = {{"document", context.document_id}, {"revision", context.document->revision()}, {"data", std::move(result)}};
        Json response = {{"ok", true}, {"result", std::move(result)}};
        if (request.contains("id")) response["id"] = request["id"];
        // Publish only after validation AND response construction have succeeded.
        // This also protects Undo/Redo and redo branches from query failures.
        if (spec->effect == Effect::DocumentWrite) live->swap(*staged);
        else if (spec->effect == Effect::SessionWrite) documents.emplace(context.document_id, std::move(*staged));
        return response;
    }
};

Commander::Commander() : Commander(make_smlib_kernel()) {}
Commander::Commander(std::unique_ptr<IKernel> kernel) : impl_(std::make_unique<Impl>(std::move(kernel))) {}
Commander::~Commander() = default;
Json Commander::commands() const { return Impl::catalog(); }
Json Commander::execute(const Json& request) {
    Json response;
    try { return impl_->execute(request); }
    catch (const CommandError& e) { response = command_error(e.kind, e.what()); }
    catch (const DocumentValidationError& e) { response = command_error("validation_failed", e.what()); }
    catch (const Json::exception& e) { response = command_error("invalid_params", e.what()); }
    catch (const std::invalid_argument& e) { response = command_error("invalid_params", e.what()); }
    catch (const std::runtime_error& e) { response = command_error("kernel_error", e.what()); }
    catch (const std::exception& e) { response = command_error("internal_error", e.what()); }
    if (request.is_object() && request.contains("id")) response["id"] = request["id"];
    return response;
}
}
