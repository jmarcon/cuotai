import re,sys,os
bad=0
for path in sys.argv[1:]:
    tid=os.path.basename(path)[:-4]
    css=re.sub(r"/\*.*?\*/","",open(path).read(),flags=re.S)
    problems=[]
    if "!important" in css: problems.append("usa !important")
    if re.search(r"url\(\s*['\"]?(https?:|//)",css): problems.append("URL externa")
    if "@import" in css or "@font-face" in css: problems.append("@import/@font-face")
    for sel in re.findall(r"([^{}]+)\{",css):
        for part in sel.split(","):
            p=part.strip()
            if p and not p.startswith(f':root[data-theme="{tid}"]'):
                problems.append(f"seletor fora de escopo: {p[:60]}")
    for prop in re.findall(r"(?<![-\w])(width|height|padding|margin|display|position|grid-template[-\w]*|font-size|flex[-\w]*)\s*:",css):
        # position/inset só são aceitos no pseudo-elemento de efeito
        if prop not in ("position",): problems.append(f"propriedade de layout: {prop}")
    print(("OK   " if not problems else "FALHA"),tid,("" if not problems else "; ".join(sorted(set(problems)))))
    bad+=bool(problems)
sys.exit(bad)
