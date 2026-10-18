#!/usr/bin/env python3
"""One worker per session. Send one JSON command per line, receive one result."""
from pathlib import Path
import json
import sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from client import Client
client=Client(sys.argv[1] if len(sys.argv)>1 else None)
try:
    for line in sys.stdin:
        try: result=client.command(json.loads(line))
        except Exception as e: result={'ok':False,'error':{'code':'command_failed','message':str(e)}}
        print(json.dumps(result),flush=True)
finally: client.close()
