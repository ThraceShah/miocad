#include "protocol.h"
#include "cad_contract.h"
#include <cmath>
namespace cad::protocol {
const Json& contract() { static const Json data=Json::parse(contract_text); return data; }
namespace {
void finite(const Json& v, unsigned depth) {
    if(depth>64) throw SchemaError("JSON nesting exceeds 64 levels");
    if(v.is_number_float() && !std::isfinite(v.get<double>())) throw SchemaError("number must be finite");
    if(v.is_structured()) for(const auto& child:v) finite(child,depth+1);
}
void check(const Json& v,const Json& s,const std::string& path,unsigned depth) {
    auto fail=[&](const std::string& reason){ throw SchemaError(path+": "+reason); };
    if(depth>64) fail("schema nesting exceeds 64 levels");
    if(s.contains("$ref")) {
        const auto ref=s["$ref"].get<std::string>();
        if(ref.rfind("#/$defs/",0)!=0) fail("unsupported schema reference");
        check(v,contract().at("$defs").at(ref.substr(8)),path,depth+1); return;
    }
    for(const auto* key:{"anyOf","oneOf"}) if(s.contains(key)) {
        unsigned matched=0;
        for(const auto& variant:s[key]) try { check(v,variant,path,depth+1); ++matched; } catch(const SchemaError&) {}
        if((std::string(key)=="oneOf" && matched!=1) || !matched) fail(std::string(key)+" did not match");
    }
    const auto type=s.value("type",std::string{});
    if((type=="object" && !v.is_object()) || (type=="array" && !v.is_array()) ||
       (type=="string" && !v.is_string()) || (type=="integer" && !v.is_number_integer()) ||
       (type=="number" && !v.is_number()) || (type=="boolean" && !v.is_boolean()) ||
       (type=="null" && !v.is_null())) fail("expected "+type);
    if(s.contains("const") && v!=s["const"]) fail("unexpected constant");
    if(s.contains("enum")) { bool found=false; for(const auto& item:s["enum"]) if(item==v) found=true; if(!found) fail("not in enum"); }
    if(v.is_number()) {
        if(s.contains("minimum") && v<s["minimum"]) fail("below minimum");
        if(s.contains("maximum") && v>s["maximum"]) fail("above maximum");
        if(s.contains("exclusiveMinimum") && v<=s["exclusiveMinimum"]) fail("must be positive");
    }
    if(v.is_string()) {
        const auto& text=v.get_ref<const std::string&>();
        if(s.contains("minLength") && text.size()<s["minLength"].get<std::size_t>()) fail("string too short");
        if(s.contains("maxLength") && text.size()>s["maxLength"].get<std::size_t>()) fail("string too long");
        if(s.value("format",std::string{})=="identifier") for(unsigned char c:text)
            if(!((c>='a' && c<='z') || (c>='A' && c<='Z') || (c>='0' && c<='9') || c=='.' || c=='_' || c==':' || c=='-')) fail("invalid identifier");
    }
    if(v.is_object()) {
        if(s.contains("required")) for(const auto& name:s["required"]) if(!v.contains(name.get<std::string>())) fail("missing "+name.get<std::string>());
        const auto properties=s.value("properties",Json::object());
        for(auto it=v.begin();it!=v.end();++it) {
            if(properties.contains(it.key())) check(it.value(),properties[it.key()],path+"."+it.key(),depth+1);
            else if(s.contains("additionalProperties") && s["additionalProperties"]==false) fail("unknown field "+it.key());
        }
    }
    if(v.is_array()) {
        if(s.contains("minItems") && v.size()<s["minItems"].get<std::size_t>()) fail("array too short");
        if(s.contains("maxItems") && v.size()>s["maxItems"].get<std::size_t>()) fail("array too long");
        if(s.contains("items")) for(std::size_t i=0;i<v.size();++i) check(v[i],s["items"],path+"["+std::to_string(i)+"]",depth+1);
    }
}
}
void validate(const Json& value,const Json& schema) { finite(value,0); check(value,schema,"$",0); }
}
