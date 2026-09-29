#!/usr/bin/env python3
"""Génère src/entities.rs depuis la table officielle du WHATWG.

    python3 tools/gen_entities.py
"""
import json
import urllib.request

URL = "https://html.spec.whatwg.org/entities.json"


def rust_str(s: str) -> str:
    out = []
    for ch in s:
        if ch in '"\\':
            out.append("\\" + ch)
        elif 0x20 <= ord(ch) < 0x7F:
            out.append(ch)
        else:
            out.append("\\u{%X}" % ord(ch))
    return '"' + "".join(out) + '"'


with urllib.request.urlopen(URL) as resp:
    data = json.load(resp)

# Clés sans le '&' initial, triées pour permettre la recherche dichotomique.
entries = sorted((name[1:], value["characters"]) for name, value in data.items())

with open("src/entities.rs", "w") as f:
    f.write("// Fichier généré par tools/gen_entities.py. Ne pas modifier à la main.\n")
    f.write(f"// Source : {URL} ({len(entries)} entités)\n\n")
    f.write("/// (nom sans '&', caractères de remplacement), trié par nom.\n")
    f.write("pub static ENTITIES: &[(&str, &str)] = &[\n")
    for name, chars in entries:
        f.write(f"    ({rust_str(name)}, {rust_str(chars)}),\n")
    f.write("];\n")

print(f"src/entities.rs : {len(entries)} entités")
