#!/usr/bin/env python3
"""Verificador de contraste (WCAG) para css/themes/<id>.css. Uso: python3 contrast.py [--strict] arquivo.css
Lê os blocos :root[data-theme="<id>"][data-mode="dark|light"] e confere pares de tokens (#rrggbb)."""
import re, sys

REQUIRED = ["--bg","--surface","--surface-2","--border","--fg","--fg-muted","--accent","--accent-fg",
            "--good","--warn","--bad","--info","--track"]
# (a, b, minimum, description)
PAIRS = [
    ("--fg","--surface",4.5,"texto principal sobre cards"),
    ("--fg","--bg",4.5,"texto principal sobre o fundo"),
    ("--fg","--surface-2",4.5,"texto principal em hover/grupos"),
    ("--fg-muted","--surface",4.5,"texto secundário sobre cards"),
    ("--fg-muted","--bg",4.0,"texto secundário sobre o fundo"),
    ("--accent-fg","--accent",4.5,"texto sobre o destaque"),
    ("--accent","--surface",3.0,"destaque sobre cards"),
    ("--good","--surface",3.0,"estado bom sobre cards"),
    ("--warn","--surface",3.0,"estado atenção sobre cards"),
    ("--bad","--surface",3.0,"estado ruim sobre cards"),
    ("--info","--surface",3.0,"informativo sobre cards"),
    ("--border","--surface",1.15,"contorno visível"),
    ("--track","--surface",1.10,"trilho da barra visível"),
]
# --strict: design system DS-COL-5..7 (text ≥ 4.5 on all backgrounds, states as text ≥ 4.5,
# charts ≥ 3 also on --bg). Optional so old themes are not failed; new themes must pass.
STRICT = [
    ("--fg-muted","--surface-2",4.5,"DS-COL-5 texto secundário em hover/grupos"),
    ("--fg-muted","--bg",4.5,"DS-COL-5 texto secundário sobre o fundo"),
] + [(k,bg,4.5,f"DS-COL-7 {k} como texto sobre {bg}") for k in ("--good","--warn","--bad","--info") for bg in ("--surface","--bg")] + [
    ("--accent","--bg",3.0,"DS-COL-7 destaque sobre o fundo"),
]
# Optional derived roles (DS-COL-6/7): checked only when the theme defines them as #rrggbb.
OPTIONAL = [
    ("--bad-fg","--bad",4.5,"DS-COL-6 texto sobre o estado ruim"),
    ("--focus","--surface",3.0,"anel de foco sobre cards"),
    ("--focus","--bg",3.0,"anel de foco sobre o fundo"),
    ("--chart-neutral","--surface",3.0,"barras neutras de gráfico sobre cards"),
]
args=[a for a in sys.argv[1:] if not a.startswith("--")]
if "--strict" in sys.argv[1:]: PAIRS=PAIRS+STRICT

def lum(h):
    r,g,b=[int(h[i:i+2],16)/255 for i in (1,3,5)]
    f=lambda c: c/12.92 if c<=0.03928 else ((c+0.055)/1.055)**2.4
    return 0.2126*f(r)+0.7152*f(g)+0.0722*f(b)
def ratio(a,b):
    la,lb=lum(a),lum(b)
    hi,lo=max(la,lb),min(la,lb)
    return (hi+0.05)/(lo+0.05)

css=open(args[0]).read()
css=re.sub(r"/\*.*?\*/","",css,flags=re.S)
blocks=re.findall(r':root\[data-theme="([a-z0-9-]+)"\]\[data-mode="(dark|light)"\]\s*\{([^}]*)\}',css)
if not blocks:
    print("ERRO: nenhum bloco :root[data-theme=…][data-mode=…] encontrado"); sys.exit(2)
bad=0
for tid,mode,body in blocks:
    tok=dict(re.findall(r"(-{0,2}[a-z][a-z0-9-]*)\s*:\s*([^;]+);",body))
    print(f"== {tid} / {mode}")
    for k in REQUIRED:
        v=tok.get(k,"").strip()
        if not re.fullmatch(r"#[0-9a-fA-F]{6}",v):
            print(f"  FALTA/INVÁLIDO {k}: {v!r} (use #rrggbb)"); bad+=1
    cs=tok.get("color-scheme","").strip()
    if cs!=mode: print(f"  color-scheme deveria ser {mode!r}, está {cs!r}"); bad+=1
    for a,b,mn,desc in PAIRS+[p for p in OPTIONAL if p[0] in tok]:
        va,vb=tok.get(a,"").strip(),tok.get(b,"").strip()
        if not (re.fullmatch(r"#[0-9a-fA-F]{6}",va) and re.fullmatch(r"#[0-9a-fA-F]{6}",vb)): continue
        r=ratio(va,vb)
        ok=r>=mn
        if not ok: bad+=1
        print(f"  {'ok  ' if ok else 'FALHA'} {a} × {b}: {r:.2f} (mín {mn}) — {desc}")
    sem=[tok.get(k,"").strip().lower() for k in ("--good","--warn","--bad")]
    if len(set(sem))<3: print("  FALHA good/warn/bad precisam ser distintos"); bad+=1
print("RESULTADO:", "OK" if bad==0 else f"{bad} problema(s)")
sys.exit(1 if bad else 0)
