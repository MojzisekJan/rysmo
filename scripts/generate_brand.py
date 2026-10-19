#!/usr/bin/env python3
"""Rebuild the approved Ember mark and all launcher assets. Requires Pillow."""
from pathlib import Path
from PIL import Image, ImageDraw
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "crates/rysmo_app/brand"
OUT.mkdir(parents=True, exist_ok=True)
# Approved rounded concept 01. Shared cubic geometry for SVG and PNG.
def path(start, segments): return (start, segments)
A=path((64,12),[
 ((80,9),(106,56),(113,80)),((121,103),(100,120),(78,113)),
 ((61,108),(34,100),(23,95)),((4,87),(7,66),(19,52)),
 ((29,39),(42,23),(49,18)),((54,14),(59,12),(64,12))])
AH=path((62,43),[
 ((70,39),(86,67),(84,75)),((82,84),(50,78),(43,71)),
 ((37,65),(54,47),(62,43))])
AS=path((113,80),[
 ((121,103),(100,120),(78,113)),((61,108),(34,100),(23,95)),
 ((9,90),(6,76),(13,62)),((17,82),(41,87),(66,91)),
 ((87,95),(105,94),(113,80))])

def coords(p):
 start,segs=p;points=[start];current=start
 for a,b,end in segs:
  for n in range(1,65):
   t=n/64;u=1-t
   points.append(tuple(u*u*u*current[k]+3*u*u*t*a[k]+3*u*t*t*b[k]+t*t*t*end[k] for k in (0,1)))
  current=end
 return points

def svgpath(p):
 start,segs=p
 return f'M {start[0]} {start[1]} '+' '.join('C '+' '.join(str(v) for q in (a,b,c) for v in q) for a,b,c in segs)+' Z'

LAYERS = [(A, AH, ['#ffce36','#ff5a20','#ef2383'], (0.2,0,0.8,1)),
          (AS, None, ['#ff327c','#8635ff'], (0,0.7,1,0.8))]

def svg(tile=False):
    defs=[]
    shapes=['<rect width="128" height="128" rx="26" fill="#20252c"/>'] if tile else []
    for i,(shape,hole,colors,axis) in enumerate(LAYERS):
        x0,y0,x1,y1=axis
        stops=''.join(f'<stop offset="{j/(len(colors)-1):.3f}" stop-color="{c}"/>' for j,c in enumerate(colors))
        defs.append(f'<linearGradient id="g{i}" gradientUnits="userSpaceOnUse" x1="{x0*128}" y1="{y0*128}" x2="{x1*128}" y2="{y1*128}">{stops}</linearGradient>')
        d=svgpath(shape)+(' '+svgpath(hole) if hole else '')
        shapes.append(f'<path d="{d}" fill="url(#g{i})" fill-rule="evenodd"/>')
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" role="img" aria-labelledby="title">'
            '<title id="title">Rysmo</title><defs>'+''.join(defs)+'</defs>'+''.join(shapes)+'</svg>\n')

def gradient(size,colors,axis):
    # A padded 1D colour ramp projected in C by Pillow, including clamped ends.
    colors=[tuple(bytes.fromhex(c[1:])) for c in colors]
    ramp=Image.new('RGBA',(5120,2))
    row=[]
    for x in range(5120):
        t=max(0,min(1,x/1024-2))*(len(colors)-1)
        i=min(int(t),len(colors)-2); f=t-i
        row.append(tuple(round(a+(b-a)*f) for a,b in zip(colors[i],colors[i+1]))+(255,))
    ramp.putdata(row+row)
    x0,y0,x1,y1=axis; dx=x1-x0; dy=y1-y0; den=dx*dx+dy*dy
    return ramp.transform((size,size),Image.Transform.AFFINE,
        (1024*dx/(size*den),1024*dy/(size*den),1024*(2-(x0*dx+y0*dy)/den),0,0,0.5),
        Image.Resampling.BILINEAR)

def raster(size,tile=False,square=False):
    S=size*4
    im=Image.new('RGBA',(S,S))
    if tile:
        ImageDraw.Draw(im).rounded_rectangle((0,0,S-1,S-1),radius=0 if square else S*26/128,fill='#20252c')
    for shape,hole,colors,axis in LAYERS:
        mask=Image.new('L',(S,S)); draw=ImageDraw.Draw(mask)
        draw.polygon([(x*S/128,y*S/128) for x,y in coords(shape)],fill=255)
        if hole: draw.polygon([(x*S/128,y*S/128) for x,y in coords(hole)],fill=0)
        layer=gradient(S,colors,axis)
        layer.putalpha(mask)
        im.alpha_composite(layer)
    return im.resize((size,size),Image.Resampling.LANCZOS)

(OUT/"logo.svg").write_text(svg(),encoding="utf-8")
(OUT/"favicon.svg").write_text(svg(True),encoding="utf-8")
raster(512).save(OUT/"logo.png")
for size in (32,64,192,512):
    raster(size,True).save(OUT/f"icon-{size}.png")
raster(180,True,True).save(OUT/"apple-touch-icon.png")
raster(256,True).save(OUT/"favicon.ico",sizes=[(16,16),(32,32),(48,48),(64,64),(256,256)])

# Native mobile launchers use the same geometry as the editor and web icons.
android_icon = ROOT / "packaging/android/res/mipmap-xxxhdpi/ic_launcher.png"
android_icon.parent.mkdir(parents=True, exist_ok=True)
raster(192, True).save(android_icon)
ios_icon = ROOT / "packaging/ios/Assets.xcassets/AppIcon.appiconset/AppIcon.png"
ios_icon.parent.mkdir(parents=True, exist_ok=True)
raster(1024, True, True).convert("RGB").save(ios_icon)
