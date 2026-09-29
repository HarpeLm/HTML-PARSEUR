#!/bin/sh
# Profile le calcul du style d'une page et génère profiling/<nom>.svg
#   ./profiling/profile.sh cascade-v0                     (Wikipedia EN)
#   ./profiling/profile.sh cascade-v0-doc rust-doc-vec
# Nécessite : cargo install inferno rustfilt
set -e
cd "$(dirname "$0")/.."
TARGET=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
NAME=${1:-cascade}
PAGE=${2:-wikipedia-en-html}
TMP=$(mktemp -d)
cargo build -q --profile profiling --example profile_style
"$TARGET"/profiling/examples/profile_style "$PAGE" 4 > "$TMP/speed.txt" &
PID=$!
sleep 0.5
sample $PID 3 1 -mayDie -file "$TMP/sample.txt" > /dev/null 2>&1
wait $PID
SPEED=$(sed -E 's/.*, //' "$TMP/speed.txt")
inferno-collapse-sample "$TMP/sample.txt" | rustfilt \
  | grep "profile_style::main" \
  | sed -E 's/^.*(profile_style`profile_style::main)/\1/' | sed -E 's/[a-z_]+`//g' \
  | sed -E 's/<lumen_style::cascade::StyleEngine>::/StyleEngine::/g; s/<lumen_css::selectors::([A-Za-z]+)>::/\1::/g; s/<lumen_style::cascade::DomElement as lumen_css::selectors::Element>::/DomElement::/g; s/<html_parseur::dom::Document>::/Document::/g' \
  > "$TMP/stacks.folded"
mkdir -p profiling
inferno-flamegraph --title "lumen-style · $NAME · $PAGE ($SPEED)" \
  --subtitle "Largeur = part du temps CPU. Cliquer pour zoomer." \
  --colors rust --width 1400 --countname échantillons \
  < "$TMP/stacks.folded" > "profiling/$NAME.svg"
cp "$TMP/stacks.folded" "profiling/$NAME.folded"
echo "profiling/$NAME.svg ($SPEED)"
