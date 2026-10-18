#!/usr/bin/env python3
"""Internal one-command MCP probe; images go to artifacts, never stdout/base64 logs."""
import base64,json,os,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def call(runtime,command,output=None):
    product=Path(os.environ.get('CAD_BIN',ROOT/'target/debug'))/'cad-mcp'
    with subprocess.Popen([str(product),'--runtime-dir',str(runtime),'--worker',str(ROOT/'build/cad-server'),'--client-id','p7-visual-mcp'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True) as child:
        def rpc(method,params):
            child.stdin.write(json.dumps(dict(jsonrpc='2.0',id=1,method=method,params=params))+'\n');child.stdin.flush();reply=json.loads(child.stdout.readline());assert 'error' not in reply,reply;return reply['result']
        rpc('initialize',dict(protocolVersion='2025-11-25',capabilities={},clientInfo={'name':'p7-visual-agent-bridge','version':'1'}))
        child.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n');child.stdin.flush()
        result=rpc('tools/call',{'name':'cad_command','arguments':{'command':command}})['structuredContent']
        child.stdin.close();child.wait(timeout=10)
    if output and result.get('result',{}).get('images'):
        folder=Path(output);folder.mkdir(parents=True,exist_ok=True)
        for mode,data in result['result']['images'].items():(folder/f'{mode}.png').write_bytes(base64.b64decode(data))
        (folder/'metadata.json').write_text(json.dumps(result,indent=2))
    return result
if __name__=='__main__':
    result=call(sys.argv[1],json.loads(sys.argv[2]),sys.argv[3] if len(sys.argv)>3 else None)
    if isinstance(result.get('result'),dict):result['result'].pop('images',None)
    print(json.dumps(result))
