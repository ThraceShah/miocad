"""Deterministic CPU preview. Geometry stays in the C++ B-Rep service."""
import json
import math
from pathlib import Path
import struct
import uuid
import zlib

def png(path, width, height, pixels):
    def chunk(kind, data):
        return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)
    rows=b''.join(b'\0'+bytes(pixels[y*width*3:(y+1)*width*3]) for y in range(height))
    Path(path).write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(rows))+chunk(b'IEND',b''))

def render(mesh, edges, document, revision, directory, size=384):
    root=Path(directory); root.mkdir(parents=True,exist_ok=True)
    render_id=uuid.uuid4().hex
    # Orthographic isometric camera looking from (+,+,+), common units with z-buffer.
    def camera(p):
        x,y,z=p
        return ((x-y)/math.sqrt(2),(2*z-x-y)/math.sqrt(6),(x+y+z)/math.sqrt(3))
    raw=[camera(p) for p in mesh['points']]
    lo=[min(p[a] for p in raw) for a in range(2)]; hi=[max(p[a] for p in raw) for a in range(2)]
    scale=(size-48)/max(hi[0]-lo[0],hi[1]-lo[1],1e-9)
    center=[(lo[a]+hi[a])/2 for a in range(2)]
    def project(p):
        x,y,z=camera(p)
        return ((x-center[0])*scale+size/2,size/2-(y-center[1])*scale,z)
    points=[project(p) for p in mesh['points']]
    depth=[-math.inf]*(size*size); faces=[None]*(size*size); picked=[None]*(size*size)
    colors=bytearray([242,245,248])*(size*size)
    for tri in mesh['triangles']:
        p,q,r=[points[i] for i in tri['vertices']]
        det=(q[1]-r[1])*(p[0]-r[0])+(r[0]-q[0])*(p[1]-r[1])
        if abs(det)<1e-10: continue
        world=[mesh['points'][i] for i in tri['vertices']]
        a=[world[1][i]-world[0][i] for i in range(3)]; b=[world[2][i]-world[0][i] for i in range(3)]
        n=[a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]
        length=math.sqrt(sum(v*v for v in n)) or 1
        light=0.35+0.65*abs(sum(n[i]*[0.35,0.25,0.9028][i] for i in range(3))/length)
        color=bytes(int(v*light) for v in [112,169,212])
        for y in range(max(0,int(min(p[1],q[1],r[1]))),min(size,math.ceil(max(p[1],q[1],r[1]))+1)):
            for x in range(max(0,int(min(p[0],q[0],r[0]))),min(size,math.ceil(max(p[0],q[0],r[0]))+1)):
                u=((q[1]-r[1])*(x+.5-r[0])+(r[0]-q[0])*(y+.5-r[1]))/det
                v=((r[1]-p[1])*(x+.5-r[0])+(p[0]-r[0])*(y+.5-r[1]))/det
                w=1-u-v
                if min(u,v,w)<-1e-7: continue
                z=u*p[2]+v*q[2]+w*r[2]; idx=y*size+x
                if z>depth[idx]: depth[idx]=z; faces[idx]=tri['face']; colors[3*idx:3*idx+3]=color
    for edge in edges:
        poly=[project(p) for p in edge['points']]
        for a,b in zip(poly,poly[1:]):
            steps=max(1,math.ceil(math.hypot(b[0]-a[0],b[1]-a[1])*2))
            for step in range(steps+1):
                t=step/steps; x,y,z=[a[i]*(1-t)+b[i]*t for i in range(3)]
                for dy in range(-2,3):
                    for dx in range(-2,3):
                        px,py=round(x)+dx,round(y)+dy
                        if 0<=px<size and 0<=py<size:
                            idx=py*size+px
                            if abs(z-depth[idx])*scale<4: picked[idx]=edge['reference']
    def id_pixels(values):
        ids={v:i+1 for i,v in enumerate(sorted(set(values)-{None}))}
        out=bytearray()
        for v in values:
            i=ids.get(v,0); out.extend((i&255,(i>>8)&255,(i>>16)&255))
        return out, {str(i):v for v,i in ids.items()}
    face_colors,face_map=id_pixels(faces); edge_colors,edge_map=id_pixels(picked)
    paths={mode:str((root/f'{render_id}-{mode}.png').resolve()) for mode in ['shaded','face-id','edge-id']}
    png(paths['shaded'],size,size,colors); png(paths['face-id'],size,size,face_colors); png(paths['edge-id'],size,size,edge_colors)
    metadata={'render_id':render_id,'document':document,'revision':revision,'width':size,'height':size,'camera':{'projection':'orthographic','direction':[1,1,1],'scale':scale,'center':center},'backend':'cpu','images':paths,'face_ids':face_map,'edge_ids':edge_map}
    (root/f'{render_id}.json').write_text(json.dumps(metadata,indent=2))
    return metadata,picked
