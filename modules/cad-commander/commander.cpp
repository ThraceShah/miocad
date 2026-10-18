#include "commander.h"
#include "commands.h"
#include "reference.h"
#include "protocol.h"
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
    Json document_list(Context&) {
        Json result = Json::array();
        for (const auto& [id, document] : documents)
            result.push_back({{"document", id}, {"revision", document.revision()}, {"data", document_state(document)}});
        return result;
    }
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
    Json scene(Context& context) {
        return {{"state", document_state(*context.document)},
            {"mesh", context.document->shape() ? mesh(context) : Json(nullptr)},
            {"edges", context.document->shape() ? edges(context) : Json::array()},
            {"can_undo", context.document->can_undo()}, {"can_redo", context.document->can_redo()}};
    }
    Json preview(Context& context) {
        // Candidate owns only temporary history; mutations clone geometry as usual.
        // Use a distinct reference namespace so candidate edges can never target live geometry.
        Document candidate = *context.document;
        const auto& operation = context.request.at("operation");
        Context temporary{operation, context.document_id, &candidate};
        if (operation.at("kind") == "box") box(temporary);
        else {
            if (!candidate.shape()) throw CommandError("empty_document", "empty document");
            fillet(temporary);
        }
        temporary.document_id = "preview." + new_id();
        return scene(temporary);
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
        static const auto entries=[] {
            const std::map<std::string,Handler> handlers={
                {"capabilities",&Impl::capabilities},{"commands.list",&Impl::list},{"document.create",&Impl::create},
                {"document.list",&Impl::document_list},{"document.state",&Impl::state},{"model.box",&Impl::box},{"model.fillet",&Impl::fillet},
                {"document.undo",&Impl::undo},{"document.redo",&Impl::redo},{"topology.edges",&Impl::edges},
                {"render.scene",&Impl::scene},{"model.preview",&Impl::preview},{"render.mesh",&Impl::mesh},{"export.native",&Impl::export_native}};
            std::vector<Spec> result;
            for(const auto& item:protocol::contract().at("commands")) {
                const auto id=item.at("id").get<std::string>();
                const auto effect=item.at("effect").get<std::string>();
                const auto& pre=item.at("preconditions");
                auto has=[&](const char* name){return std::find(pre.begin(),pre.end(),name)!=pre.end();};
                result.push_back({id,effect=="query"?Effect::Query:effect=="session_write"?Effect::SessionWrite:
                    effect=="document_write"?Effect::DocumentWrite:Effect::ExternalWrite,
                    has("document_exists"),has("nonempty_body"),has("current_revision"),
                    has("undo_available")?History::Undo:has("redo_available")?History::Redo:History::None,
                    item.at("input_schema"),item.at("output_schema"),handlers.at(id)});
            }
            if(result.size()!=handlers.size()) throw std::logic_error("command schema/handler mismatch");
            return result;
        }();
        return entries;
    }
    static Json catalog() { return protocol::contract().at("commands"); }

    Json execute(const Json& request) {
        if (!request.is_object() || !request.contains("op") || !request["op"].is_string())
            throw CommandError("invalid_request", "request must be an object with a string op");
        const auto operation = request["op"].get<std::string>();
        const auto& entries = specs();
        const auto spec = std::find_if(entries.begin(), entries.end(), [&](const Spec& candidate) { return candidate.id == operation; });
        if (spec == entries.end()) throw CommandError("unknown_command", "unknown operation: " + operation);
        try { protocol::validate(request,spec->input); }
        catch(const protocol::SchemaError& e) { throw CommandError("invalid_params",e.what()); }
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
        try { protocol::validate(result,spec->output); }
        catch(const protocol::SchemaError& e) { throw CommandError("internal_error",std::string("invalid command result: ")+e.what()); }
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
