#!/usr/bin/env python3
"""Internal verification: real Codex host MCP discovery, no inference/model API calls."""
import json,os,queue,subprocess,tempfile,threading,time,tomllib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];OUT=ROOT/'artifacts/p7';OUT.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='p7-codex-mcp-') as runtime:
    command=['codex','app-server','--listen','stdio://']
    # Do not change user configuration or start unrelated configured MCP servers.
    config=Path.home()/'.codex/config.toml'
    for name in tomllib.loads(config.read_text()).get('mcp_servers',{}) if config.exists() else []:
        command+=['-c',f'mcp_servers.{name}.enabled=false']
    command+=['-c',f'mcp_servers.cad_p7.command={json.dumps(str(ROOT/"target/debug/cad-mcp"))}', '-c',f'mcp_servers.cad_p7.args={json.dumps(["--runtime-dir",runtime,"--worker",str(ROOT/"build/cad-server")])}','-c','mcp_servers.cad_p7.enabled=true']
    log=(OUT/'codex-host.stderr').open('w');host=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,text=True)
    messages=queue.Queue()
    def read():
        for line in host.stdout:
            try:messages.put(json.loads(line))
            except ValueError:pass
    threading.Thread(target=read,daemon=True).start()
    next_id=0
    def rpc(method,params):
        global next_id
        next_id+=1;rid=next_id;host.stdin.write(json.dumps(dict(id=rid,method=method,params=params))+'\n');host.stdin.flush()
        until=time.monotonic()+45
        while time.monotonic()<until:
            msg=messages.get(timeout=max(.1,until-time.monotonic()))
            if msg.get('id')==rid:
                assert 'error' not in msg,msg
                return msg['result']
        raise TimeoutError(method)
    try:
        initialized=rpc('initialize',{'clientInfo':{'name':'miocad_p7_verification','title':'CAD MCP verification','version':'1'},'capabilities':{'experimentalApi':True}})
        host.stdin.write('{"method":"initialized","params":{}}\n');host.stdin.flush()
        # Starting a thread initializes MCP clients, without starting any model turn.
        thread=rpc('thread/start',{'cwd':runtime,'approvalPolicy':'never','sandbox':'read-only','ephemeral':True})
        status=rpc('mcpServerStatus/list',{})
        servers=status.get('data',[]);cad=next((s for s in servers if s.get('name')=='cad_p7'),None)
        assert cad and cad.get('tools'),status
        assert any('cad_command' in key for key in cad['tools']),cad
        receipt={'passed':True,'client':subprocess.check_output(['codex','--version'],text=True).strip(),'initialize':initialized,'cad':cad,'scope':'Actual Codex MCP handshake/tool discovery only; no inference turn, not a VLM evaluation.'}
        (OUT/'codex-host.json').write_text(json.dumps(receipt,indent=2));print(json.dumps({'passed':True,'client':receipt['client'],'tools':list(cad['tools'])}))
    finally:
        host.terminate()
        try:host.wait(timeout=5)
        except subprocess.TimeoutExpired:host.kill();host.wait()
        # Discovery is lazy; stop the private CAD service only if MCP launched it.
        if (Path(runtime)/'endpoint.json').exists():subprocess.run([str(ROOT/'target/debug/cad-cli'),'--runtime-dir',runtime,'--command','{"op":"service.shutdown"}'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        log.close()
