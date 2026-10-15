#!/usr/bin/env python3
"""Minimal MCP stdio bridge, protocol 2025-11-25; local single-session prototype."""
import argparse
import base64
import json
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'scripts'))
from client import Client
OPS=['capabilities','document.create','document.state','document.undo','document.redo','model.box','model.fillet','topology.edges','render.mesh','render.snapshot','render.pick','export.native','export.step','import.step']
TOOL={'name':'cad_command','description':'Execute a CAD command. Start with capabilities and document.create. Box needs size[3] in mm and revision. Fillet needs a current topology.edges/render.pick reference, radius, revision. Mutations require current revision. Check capabilities: STEP import/export requires the optional OCCT backend; import.step needs path and revision, export.step needs path (.step/.stp). Import supports one solid and replaces the document body. render.snapshot returns shaded and ID images; render.pick takes render_id,x,y. State persists only for this server session.',
      'inputSchema':{'type':'object','properties':{'command':{'type':'object','properties':{'op':{'type':'string','enum':OPS},'document':{'type':'string'},'revision':{'type':'integer','minimum':0},'size':{'type':'array','items':{'type':'number','exclusiveMinimum':0},'minItems':3,'maxItems':3},'edge':{'type':'string'},'radius':{'type':'number','exclusiveMinimum':0},'path':{'type':'string'},'directory':{'type':'string'},'render_id':{'type':'string'},'x':{'type':'integer','minimum':0},'y':{'type':'integer','minimum':0}},'required':['op'],'additionalProperties':False}},'required':['command'],'additionalProperties':False}}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server',help='CAD worker executable (or CAD_SERVER)')
    parser.add_argument('--kernel',choices=['smlib','occt'],help='Backend (or CAD_KERNEL; default smlib)')
    args=parser.parse_args()
    client=Client(args.server,args.kernel); initialized=False; ready=False
    try:
        for line in sys.stdin:
            request=None
            try:
                request=json.loads(line)
                if not isinstance(request,dict) or request.get('jsonrpc')!='2.0' or not isinstance(request.get('method'),str):
                    raise ValueError('invalid JSON-RPC request')
                method=request['method']
                if 'id' not in request:
                    if method=='notifications/initialized' and initialized: ready=True
                    continue
                response={'jsonrpc':'2.0','id':request['id']}
                if method=='initialize':
                    if initialized: raise ValueError('already initialized')
                    params=request.get('params',{})
                    if not isinstance(params,dict): raise ValueError('invalid initialize params')
                    result={'protocolVersion':'2025-11-25','capabilities':{'tools':{'listChanged':False}},'serverInfo':{'name':'ai-cad','version':'0.1.0'}}
                    initialized=True
                elif method=='ping': result={}
                elif not ready: raise ValueError('initialize first, then notifications/initialized')
                elif method=='tools/list': result={'tools':[TOOL]}
                elif method=='tools/call':
                    params=request.get('params',{})
                    if params.get('name')!='cad_command':
                        response['error']={'code':-32602,'message':'unknown tool'}
                    else:
                        try:
                            command=params['arguments']['command']
                            if not isinstance(command,dict) or command.get('op') not in OPS: raise ValueError('invalid CAD command')
                            output=client.command(command)
                        except Exception as e: output={'ok':False,'error':{'message':str(e)}}
                        content=[{'type':'text','text':json.dumps(output)}]
                        if output.get('ok') and command.get('op')=='render.snapshot':
                            data=Path(output['result']['images']['shaded']).read_bytes()
                            content.append({'type':'image','mimeType':'image/png','data':base64.b64encode(data).decode()})
                        result={'content':content,'isError':not output.get('ok',False)}
                else: response['error']={'code':-32601,'message':'method not found'}
                if 'error' not in response: response['result']=result
            except json.JSONDecodeError:
                response={'jsonrpc':'2.0','id':None,'error':{'code':-32700,'message':'parse error'}}
            except Exception as e:
                if isinstance(request,dict) and 'id' not in request: continue
                response={'jsonrpc':'2.0','id':request.get('id') if isinstance(request,dict) else None,'error':{'code':-32600,'message':str(e)}}
            print(json.dumps(response),flush=True)
    finally: client.close()
if __name__=='__main__': main()
