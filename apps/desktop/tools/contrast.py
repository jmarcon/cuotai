#!/usr/bin/env python3
"""WCAG contrast checker for css/themes/<id>.css. Usage: python3 contrast.py [--strict] file.css
Reads the :root[data-theme="<id>"][data-mode="dark|light"] blocks and checks token pairs (#rrggbb).

Besides plain pairs, --strict measures composited backgrounds the UI really paints (PRD 0022 re-validation V-02/V-03):
`--fg` on each role's chip tint (`color-mix(in srgb, role 16%, base)`, DS-CMP-BADGE-2) over `--surface`, `--bg` and
`--surface-2`, and `--fg`/`--fg-muted` on the page background `--body-bg` (each #rrggbb stop of a gradient)."""
import re, sys

REQUIRED = ["--bg","--surface","--surface-2","--border","--fg","--fg-muted","--accent","--accent-fg",
            "--good","--warn","--bad","--info","--track"]
# (a, b, minimum, description)
PAIRS = [
    ("--fg","--surface",4.5,"main text on cards"),
    ("--fg","--bg",4.5,"main text on the background"),
    ("--fg","--surface-2",4.5,"main text on hover/groups"),
    ("--fg-muted","--surface",4.5,"secondary text on cards"),
    ("--fg-muted","--bg",4.0,"secondary text on the background"),
    ("--accent-fg","--accent",4.5,"text on the accent"),
    ("--accent","--surface",3.0,"accent on cards"),
    ("--good","--surface",3.0,"good state on cards"),
    ("--warn","--surface",3.0,"warning state on cards"),
    ("--bad","--surface",3.0,"bad state on cards"),
    ("--info","--surface",3.0,"informative on cards"),
    ("--border","--surface",1.15,"visible outline"),
    ("--track","--surface",1.10,"visible bar track"),
]
# --strict: design system DS-COL-5..7 (text ≥ 4.5 on all backgrounds, states as text ≥ 4.5,
# charts ≥ 3 also on --bg). Optional so old themes are not failed; new themes must pass.
STRICT = [
    ("--fg-muted","--surface-2",4.5,"DS-COL-5 secondary text on hover/groups"),
    ("--fg-muted","--bg",4.5,"DS-COL-5 secondary text on the background"),
] + [(k,bg,4.5,f"DS-COL-7 {k} as text on {bg}") for k in ("--good","--warn","--bad","--info") for bg in ("--surface","--bg")] + [
    ("--accent","--bg",3.0,"DS-COL-7 accent on the background"),
]
# DS-CMP-BADGE-2: a chip is `--fg` text on its role at this share over the surface it sits on.
TINT = 0.16
TINT_ROLES = ("--good","--warn","--bad","--info","--accent","--fg-muted")
TINT_BASES = ("--surface","--bg","--surface-2")
# Derived roles that fall back to a base token in css/themes.css when a theme leaves them out; they are resolved
# through that fallback so the pair is always checked (the danger button paints --bad-fg on --bad).
FALLBACK = {"--bad-fg": "--accent-fg", "--focus": "--accent", "--body-bg": "--bg"}
# Optional derived roles (DS-COL-6/7): checked when the theme defines them as #rrggbb or they resolve through FALLBACK.
OPTIONAL = [
    ("--bad-fg","--bad",4.5,"DS-COL-6 text on the bad state"),
    ("--focus","--surface",3.0,"focus ring on cards"),
    ("--focus","--bg",3.0,"focus ring on the background"),
    ("--chart-neutral","--surface",3.0,"neutral chart bars on cards"),
]
args=[a for a in sys.argv[1:] if not a.startswith("--")]
strict="--strict" in sys.argv[1:]
if strict: PAIRS=PAIRS+STRICT
HEX=re.compile(r"#[0-9a-fA-F]{6}")

def lum(h):
    r,g,b=[int(h[i:i+2],16)/255 for i in (1,3,5)]
    f=lambda c: c/12.92 if c<=0.03928 else ((c+0.055)/1.055)**2.4
    return 0.2126*f(r)+0.7152*f(g)+0.0722*f(b)
def ratio(a,b):
    la,lb=lum(a),lum(b)
    hi,lo=max(la,lb),min(la,lb)
    return (hi+0.05)/(lo+0.05)
def mix(a,b,p):
    """color-mix(in srgb, a p, b): channel-wise in gamma-encoded sRGB (the same as a at p alpha over b)."""
    ca=[int(a[i:i+2],16) for i in (1,3,5)]; cb=[int(b[i:i+2],16) for i in (1,3,5)]
    return "#"+"".join(f"{round(x*p+y*(1-p)):02x}" for x,y in zip(ca,cb))

css=open(args[0]).read()
css=re.sub(r"/\*.*?\*/","",css,flags=re.S)
blocks=re.findall(r':root\[data-theme="([a-z0-9-]+)"\]\[data-mode="(dark|light)"\]\s*\{([^}]*)\}',css)
if not blocks:
    print("ERROR: no :root[data-theme=…][data-mode=…] block found"); sys.exit(2)
bad=0
def check(label_a,label_b,va,vb,mn,desc):
    global bad
    r=ratio(va,vb)
    ok=r>=mn
    if not ok: bad+=1
    print(f"  {'ok  ' if ok else 'FAIL'} {label_a} × {label_b}: {r:.2f} (min {mn}) — {desc}")
for tid,mode,body in blocks:
    tok={k:v.strip() for k,v in re.findall(r"(-{0,2}[a-z][a-z0-9-]*)\s*:\s*([^;]+);",body)}
    for k,base in FALLBACK.items():
        if k not in tok and base in tok: tok[k]=tok[base]
    print(f"== {tid} / {mode}")
    for k in REQUIRED:
        v=tok.get(k,"")
        if not HEX.fullmatch(v):
            print(f"  MISSING/INVALID {k}: {v!r} (use #rrggbb)"); bad+=1
    cs=tok.get("color-scheme","")
    if cs!=mode: print(f"  color-scheme should be {mode!r}, is {cs!r}"); bad+=1
    for a,b,mn,desc in PAIRS+[p for p in OPTIONAL if p[0] in tok]:
        va,vb=tok.get(a,""),tok.get(b,"")
        if HEX.fullmatch(va) and HEX.fullmatch(vb): check(a,b,va,vb,mn,desc)
    if strict:
        fg=tok.get("--fg","")
        for role in TINT_ROLES:
            for base in TINT_BASES:
                vr,vb=tok.get(role,""),tok.get(base,"")
                if HEX.fullmatch(fg) and HEX.fullmatch(vr) and HEX.fullmatch(vb):
                    check("--fg",f"{role}@{int(TINT*100)}%/{base}",fg,mix(vr,vb,TINT),4.5,"DS-CMP-BADGE-2 chip text on its tint")
        stops=HEX.findall(tok.get("--body-bg",""))
        for text in ("--fg","--fg-muted"):
            vt=tok.get(text,"")
            if not HEX.fullmatch(vt): continue
            for i,stop in enumerate(stops):
                check(text,f"--body-bg[{i}]",vt,stop.lower(),4.5,"DS-COL-5 text on the page background")
    sem=[tok.get(k,"").lower() for k in ("--good","--warn","--bad")]
    if len(set(sem))<3: print("  FAIL good/warn/bad must be distinct"); bad+=1
print("RESULT:", "OK" if bad==0 else f"{bad} problem(s)")
sys.exit(1 if bad else 0)
