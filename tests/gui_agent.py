"""Real native Agent panel/bridge acceptance with a deterministic process adapter."""
import json
import os
from pathlib import Path
import shutil
import sys
import time
from gui_smoke import Smoke, ROOT

OUT=ROOT/'artifacts/agent-chat'
OUT.mkdir(parents=True,exist_ok=True)
os.environ['CAD_AGENT_BACKEND']='json-stdio'
os.environ['CAD_AGENT_COMMAND_JSON']=json.dumps([shutil.which('node'),str(ROOT/'tests/fixtures/agent_adapter.cjs'),str(OUT/'native-adapter-input.json')])
smoke=Smoke()
try:
    end=time.monotonic()+15
    while not smoke.ui('layout').get('connected'):
        assert time.monotonic()<end
        time.sleep(.1)
    smoke.ui('focus')
    time.sleep(.3)
    smoke.ui('control',name='new-document')
    smoke.ui('control',name='preview')
    end=time.monotonic()+12
    while not smoke.ui('layout')['controls'].get('confirm',{}).get('enabled'):
        assert time.monotonic()<end
        time.sleep(.1)
    smoke.ui('control',name='confirm')
    assert smoke.wait_revision(1)['data']['valid']
    smoke.ui('control',name='chat')
    smoke.ui('control',name='tree-edges-header')
    smoke.ui('control',name='tree-edge-0')
    refs=smoke.ui('layout')['references']
    assert len(refs)==1 and refs[0]['kind']=='edge',refs
    smoke.ui('focus')
    smoke.ui('control',name='chat-input')
    for char in 'fillet':
        smoke.ui('key',sym=ord(char));time.sleep(.05)
    smoke.ui('control',name='chat-send')
    end=time.monotonic()+25
    while True:
        layout=smoke.ui('layout')
        assert not layout.get('agent_error'),layout
        if layout.get('agent_task',{}).get('done'):break
        assert time.monotonic()<end
        time.sleep(.1)
    assert layout['agent_task']['phase']=='completed',layout
    assert 'agent-preview' not in layout['controls'] and 'confirm' not in layout['controls']
    state=smoke.wait_revision(2)
    assert state['data']['valid'] and state['data']['volume']<1000
    capture=smoke.ui('shot',name='agent-native-result')
    shutil.copy2(capture,OUT/'native-result.png')
    smoke.ui('control',name='agent-undo')
    end=time.monotonic()+15
    while smoke.ui('layout').get('agent_thinking'):
        assert time.monotonic()<end
        time.sleep(.1)
    assert abs(smoke.state()['data']['volume']-1000)<1e-6
    result=dict(passed=True,native_bridge=True,reference=refs[0],automatic_commit=True,no_approval=True,task_undo=True)
    (OUT/'native-results.json').write_text(json.dumps(result,indent=2))
    print(json.dumps(result))
except Exception:
    smoke.ui('shot',name='agent-native-failure')
    raise
finally:
    smoke.close()
