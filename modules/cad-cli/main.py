#!/usr/bin/env python3
"""One worker per session. Send one JSON command per line, receive one result."""
from pathlib import Path
import argparse
import json
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'scripts'))
from client import Client
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('server',nargs='?',help='CAD worker executable (or CAD_SERVER)')
parser.add_argument('--kernel',choices=['smlib','occt'],help='Backend (or CAD_KERNEL; default smlib)')
args=parser.parse_args()
client=Client(args.server,args.kernel)
try:
    for line in sys.stdin:
        try: result=client.command(json.loads(line))
        except Exception as e: result={'ok':False,'error':{'code':'command_failed','message':str(e)}}
        print(json.dumps(result),flush=True)
finally: client.close()
