#include "endpoint.h"
#include "commander.h"
#include <deque>
#include <map>
#include <random>
#include <sstream>
#include <utility>
namespace cad::protocol {
namespace {
std::string identifier() { std::random_device r;std::ostringstream s;s<<std::hex;for(int i=0;i<4;++i)s<<r()<<'-';return s.str(); }
constexpr std::size_t max_entries=64, max_bytes=16*1024*1024;
}
struct Endpoint::Impl {
    Commander& commander;
    const std::string worker=identifier(), session=identifier();
    std::map<std::string,std::uint64_t> high_water;
    struct Entry { std::string fingerprint; Json response; bool execution; std::size_t bytes=0; };
    using Key=std::pair<std::string,std::uint64_t>;
    std::map<Key,Entry> cache;
    std::deque<Key> order;
    std::size_t bytes=0;
    std::uint64_t event_id=0;
    explicit Impl(Commander& core):commander(core){}
    Json base(const Json& req) const {
        auto client=req.value("client_id",Json("unknown"));
        auto request=req.value("request_id",Json(0));
        try { validate(client,contract()["$defs"]["Identifier"]); } catch(const SchemaError&) {client="unknown";}
        try { validate(request,contract()["$defs"]["Counter"]); } catch(const SchemaError&) {request=0;}
        return {{"protocol",contract()["protocol_version"]},{"client_id",client},{"request_id",request},
            {"worker_id",worker},{"session_id",session},{"service_id",nullptr},{"events",Json::array()},{"replayed",false}};
    }
    Json failure(const Json& req,const std::string& kind,const std::string& message) const {
        auto response=base(req);response.update(command_error(kind,message));response["error"]["retryable"]=false;return response;
    }
    void prune() {
        while(cache.size()>max_entries || bytes>max_bytes) {
            const auto key=order.front();order.pop_front();
            auto it=cache.find(key); if(it!=cache.end()){bytes-=it->second.bytes;cache.erase(it);}
        }
    }
    Json run(const Json& req) {
        if(!req.is_object()) return failure(Json::object(),"invalid_request","request must be an object");
        if(req.value("protocol",Json())!=contract()["protocol_version"])
            return failure(req,"version_mismatch","supported protocol: "+contract()["protocol_version"].get<std::string>());
        try { validate(req,contract()["$defs"]["Request"]); }
        catch(const SchemaError& e){return failure(req,"invalid_request",e.what());}
        const auto client=req["client_id"].get<std::string>();
        const auto request_id=req["request_id"].get<std::uint64_t>();
        const auto method=req["method"].get<std::string>();
        if(method=="hello") {
            if(!high_water.count(client) && high_water.size()>=128) return failure(req,"client_limit","worker client limit reached");
            high_water.emplace(client,0);
            auto response=base(req);response["ok"]=true;
            response["result"]={{"capabilities",commander.execute({{"op","capabilities"}}).at("result")},
                {"deduplication",{{"scope","worker"},{"max_entries",max_entries},{"max_bytes",max_bytes},{"durable",false}}},
                {"mesh_transport","json.inline"},{"events","inline"},{"shared_service",false},{"last_request_id",high_water.at(client)}};
            return response;
        }
        if(req["worker_id"]!=worker) return failure(req,"wrong_instance","worker changed; do not replay requests against another instance");
        if(req["session_id"]!=session) return failure(req,"wrong_session","unknown worker session");
        auto water=high_water.find(client);
        if(water==high_water.end()) return failure(req,"client_not_initialized","hello required for this client");
        if(request_id==0) return failure(req,"invalid_request","request_id zero is reserved for hello");
        const Key key{client,request_id};
        const auto fingerprint=req.dump();
        if(auto found=cache.find(key);found!=cache.end()) {
            if(found->second.fingerprint!=fingerprint) return failure(req,"request_conflict","request_id reused with different content");
            auto response=found->second.response;response["replayed"]=true;return response;
        }
        if(request_id<=water->second) return failure(req,"outcome_unavailable","request was already accepted; cached result unavailable; it will not run again");
        // Reserve the identity before invoking any side effect. If reply assembly
        // fails after a commit, a retry must never execute the command a second time.
        auto unknown=failure(req,"outcome_unknown","execution result unavailable; inspect document state");
        cache.emplace(key,Entry{fingerprint,unknown,method=="execute",0});order.push_back(key);water->second=request_id;
        auto response=base(req);
        try {
            if(method=="request.status") {
                const auto target=req["target_request_id"].get<std::uint64_t>();
                auto found=cache.find({client,target});
                response["ok"]=true;
                if(target>=request_id) response=failure(req,"invalid_params","status target must precede this request");
                else if(found!=cache.end() && found->second.execution && !(found->second.response.contains("error") && found->second.response["error"]["kind"]=="outcome_unknown"))
                    response["result"]={{"status","completed"},{"response",found->second.response}};
                else response["result"]={{"status","outcome_unavailable"}};
            } else {
                auto result=commander.execute(req["command"]);result.erase("id");response.update(result);
                if(!response["ok"].get<bool>())response["error"]["retryable"]=false;
                else {
                    const auto operation=req["command"].value("op",std::string{});
                    std::string effect;
                    for(const auto& spec:contract()["commands"])if(spec["id"]==operation)effect=spec["effect"].get<std::string>();
                    if((effect=="document_write" || effect=="session_write") && req["command"].value("document",std::string{}).rfind("tx.",0)!=0) {
                        const auto& result=response["result"];
                        response["events"].push_back({{"kind",effect=="session_write"?"document.created":"document.changed"},
                            {"event_id",++event_id},{"worker_id",worker},{"session_id",session},
                            {"document",result["document"]},{"revision",result["revision"]},
                            {"client_id",client},{"request_id",request_id},{"command",operation}});
                    }
                }
            }
            validate(response,contract()["$defs"]["Response"]);
            auto& entry=cache.at(key);entry.response=response;entry.bytes=response.dump().size()+fingerprint.size();bytes+=entry.bytes;prune();
            return response;
        } catch(const std::exception&) { return unknown; }
    }
};
Endpoint::Endpoint(Commander& commander):impl_(std::make_unique<Impl>(commander)){}
Endpoint::~Endpoint()=default;
Json Endpoint::execute(const Json& request) {
    try{return impl_->run(request);}catch(const std::exception& e){return impl_->failure(request.is_object()?request:Json::object(),"invalid_request",e.what());}
}
}
