#include "protocol.h"
#include <fstream>
#include <iostream>
#include <limits>
#include <stdexcept>
int main(int argc,char** argv) try {
    if(argc!=3)throw std::runtime_error("contract and corpus paths required");
    using cad::protocol::Json;
    std::ifstream source(argv[1]),fixtures(argv[2]);
    Json contract,corpus;source>>contract;fixtures>>corpus;
    if(contract!=cad::protocol::contract())throw std::runtime_error("embedded contract is stale");
    for(const auto& item:corpus){
        auto which=item.at("schema").get<std::string>();Json schema;
        if(which.rfind("command:",0)==0){
            for(const auto& command:contract["commands"])if(command["id"]==which.substr(8))schema=command["input_schema"];
        }else schema=contract["$defs"].at(which);
        if(schema.is_null())throw std::runtime_error("unknown corpus schema");
        bool accepted=true;
        try{cad::protocol::validate(item["value"],schema);}catch(const cad::protocol::SchemaError&){accepted=false;}
        if(accepted!=item["valid"].get<bool>())throw std::runtime_error("case failed: "+item["name"].get<std::string>());
        if(Json::parse(item["value"].dump())!=item["value"])throw std::runtime_error("JSON roundtrip changed fixture");
    }
    try{cad::protocol::validate(Json(std::numeric_limits<double>::infinity()),{{"type","number"}});throw std::logic_error("Infinity accepted");}catch(const cad::protocol::SchemaError&){}
    std::cout<<corpus.size()<<" shared schema/serialization cases passed\n";
    return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
