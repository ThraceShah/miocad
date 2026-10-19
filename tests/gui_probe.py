"""Internal isolated X11 probe: drives the real native window using XTest."""
import ctypes as C
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import tempfile
ROOT=Path(__file__).resolve().parents[1]
BIN=Path(os.environ.get('CAD_BIN',ROOT/'target/debug')).resolve()
OUT=ROOT/'artifacts/p7-gui';OUT.mkdir(parents=True,exist_ok=True)
(OUT/'gui.log').write_text('')
runtime=Path(tempfile.mkdtemp(prefix='miocad-p5-window-'))
log=(OUT/'xvfb.log').open('w')
xvfb=subprocess.Popen(['/usr/bin/Xvfb','-displayfd','1','-screen','0','1280x820x24','-nolisten','tcp','+extension','GLX'],stdout=subprocess.PIPE,stderr=log,text=True)
display=':'+xvfb.stdout.readline().strip()
env=dict(os.environ,CAD_GUI_PROBE_FILE=str(OUT/'ui.json'),DISPLAY=display,LIBGL_ALWAYS_SOFTWARE='1',VK_ICD_FILENAMES='/usr/share/vulkan/icd.d/lvp_icd.json',XDG_RUNTIME_DIR=str(OUT/'xdg'))
Path(env['XDG_RUNTIME_DIR']).mkdir(mode=0o700,exist_ok=True)
xl=C.CDLL('libX11.so.6');xt=C.CDLL('libXtst.so.6')
xl.XOpenDisplay.argtypes=[C.c_char_p];xl.XOpenDisplay.restype=C.c_void_p
xl.XFlush.argtypes=[C.c_void_p];xl.XCloseDisplay.argtypes=[C.c_void_p]
xt.XTestFakeMotionEvent.argtypes=[C.c_void_p,C.c_int,C.c_int,C.c_int,C.c_ulong]
xt.XTestFakeButtonEvent.argtypes=[C.c_void_p,C.c_uint,C.c_int,C.c_ulong]
xt.XTestFakeKeyEvent.argtypes=[C.c_void_p,C.c_uint,C.c_int,C.c_ulong]
xl.XKeysymToKeycode.argtypes=[C.c_void_p,C.c_ulong];xl.XKeysymToKeycode.restype=C.c_ubyte
xd=xl.XOpenDisplay(display.encode())
if not xd:raise RuntimeError('X display failed')
gui=None
handles=[]
def start():
 global gui
 f=(OUT/'gui.log').open('a');handles.append(f)
 gui=subprocess.Popen([str(BIN/'cad-gui'),'--runtime-dir',str(runtime)],env=env,stdout=f,stderr=f)
 return gui.pid
def motion(x,y):xt.XTestFakeMotionEvent(xd,-1,x,y,0);xl.XFlush(xd)
def click(x,y):
 motion(x,y);time.sleep(.08);xt.XTestFakeButtonEvent(xd,1,1,0);xl.XFlush(xd);time.sleep(.04);xt.XTestFakeButtonEvent(xd,1,0,0);xl.XFlush(xd);time.sleep(.12)
def key(sym):
 code=xl.XKeysymToKeycode(xd,sym);xt.XTestFakeKeyEvent(xd,code,1,0);xt.XTestFakeKeyEvent(xd,code,0,0);xl.XFlush(xd)
def close_window():
 output=subprocess.check_output(['/usr/bin/xwininfo','-root','-tree'],env=env,text=True)
 window=next(int(line.strip().split()[0],16) for line in output.splitlines() if 'AI-CAD' in line)
 class Data(C.Union): _fields_=[('l',C.c_long*5),('b',C.c_char*20)]
 class Message(C.Structure): _fields_=[('type',C.c_int),('serial',C.c_ulong),('send_event',C.c_int),('display',C.c_void_p),('window',C.c_ulong),('message_type',C.c_ulong),('format',C.c_int),('data',Data)]
 class Event(C.Union): _fields_=[('client',Message),('pad',C.c_long*24)]
 xl.XInternAtom.argtypes=[C.c_void_p,C.c_char_p,C.c_int];xl.XInternAtom.restype=C.c_ulong
 xl.XSendEvent.argtypes=[C.c_void_p,C.c_ulong,C.c_int,C.c_long,C.POINTER(Event)]
 event=Event();event.client.type=33;event.client.display=xd;event.client.window=window;event.client.format=32;event.client.message_type=xl.XInternAtom(xd,b'WM_PROTOCOLS',0);event.client.data.l[0]=xl.XInternAtom(xd,b'WM_DELETE_WINDOW',0)
 xl.XSendEvent(xd,window,0,0,C.byref(event));xl.XFlush(xd);gui.wait(timeout=10)
def shot(name):
 motion(8,8);time.sleep(.12)
 path=OUT/f'{name}.png'
 subprocess.run(['/usr/bin/ffmpeg','-hide_banner','-loglevel','error','-f','x11grab','-video_size','1280x820','-draw_mouse','0','-i',display,'-frames:v','1','-y',str(path)],check=True)
 return str(path)
start();print(json.dumps({'display':display,'gui':gui.pid,'runtime':str(runtime)}),flush=True)
try:
 for line in sys.stdin:
  try:
   cmd=json.loads(line);op=cmd['op']
   if op=='layout':result=json.loads((OUT/'ui.json').read_text())
   elif op=='control':
    until=time.monotonic()+10
    while True:
     layout=json.loads((OUT/'ui.json').read_text());button=layout['controls'].get(cmd['name'])
     if button and (button['enabled'] or cmd.get('allow_disabled',False)):break
     if time.monotonic()>until:raise ValueError('Control unavailable: '+cmd['name'])
     time.sleep(.03)
    x,y,w,h=button['rect'];scale=layout.get('scale',1);click(round((x+w/2)*scale),round((y+h/2)*scale));result=button
   elif op=='click':click(cmd['x'],cmd['y']);result='clicked'
   elif op=='move':motion(cmd['x'],cmd['y']);result='moved'
   elif op=='drag':
    motion(*cmd['from']);xt.XTestFakeButtonEvent(xd,cmd.get('button',1),1,0);xl.XFlush(xd)
    for i in range(1,16):
     motion(round(cmd['from'][0]+(cmd['to'][0]-cmd['from'][0])*i/15),round(cmd['from'][1]+(cmd['to'][1]-cmd['from'][1])*i/15));time.sleep(.02)
    xt.XTestFakeButtonEvent(xd,cmd.get('button',1),0,0);xl.XFlush(xd);result='dragged'
   elif op=='shot':result=shot(cmd['name'])
   elif op=='key':key(cmd['sym']);result='keyed'
   elif op=='stop-gui':close_window();result='closed'
   elif op=='start-gui':result=start()
   elif op=='state':result={'alive':gui.poll() is None,'log':(OUT/'gui.log').read_text()[-4000:]}
   elif op=='quit':break
   else:raise ValueError(op)
   print(json.dumps({'ok':True,'result':result}),flush=True)
  except Exception as e:print(json.dumps({'ok':False,'error':str(e)}),flush=True)
finally:
 if gui and gui.poll() is None:gui.terminate();gui.wait(timeout=10)
 if (runtime/'endpoint.json').exists():subprocess.run([str(BIN/'cad-cli'),'--runtime-dir',str(runtime),'--command','{"op":"service.shutdown"}'],stdout=subprocess.DEVNULL)
 xl.XCloseDisplay(xd);xvfb.terminate();xvfb.wait(timeout=10)
