#!/bin/sh
# Profile la page blog et génère profiling/<nom>.svg
#   ./profiling/profile.sh blog-v1                  (tokenizer seul)
#   ./profiling/profile.sh parse-v0 profile_parse   (tokenizer + DOM)
# Nécessite : cargo install inferno rustfilt
set -e
# On se place dans le dossier du crate (html/), quel que soit le dossier courant.
cd "$(dirname "$0")/.."
# Dans un workspace, les binaires compilés sont dans le target/ de la racine.
TARGET=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
NAME=${1:-blog}
EXAMPLE=${2:-profile_blog}
TMP=$(mktemp -d)
cargo build -q --profile profiling --example "$EXAMPLE"
"$TARGET"/profiling/examples/"$EXAMPLE" > "$TMP/speed.txt" &
PID=$!
sleep 0.3
sample $PID 3 1 -mayDie -file "$TMP/sample.txt" > /dev/null 2>&1
wait $PID
SPEED=$(sed -E 's/.*, //' "$TMP/speed.txt")
inferno-collapse-sample "$TMP/sample.txt" | rustfilt \
  | grep "$EXAMPLE::main" \
  | sed -E "s/^.*($EXAMPLE\`$EXAMPLE::main)/\1/" | sed -E 's/[a-z_]+`//g' \
  | sed -E 's/<html_parseur::tokenizer::Tokenizer as core::iter::traits::iterator::Iterator>::next/Tokenizer::next/g; s/<html_parseur::tokenizer::Tokenizer>::/Tokenizer::/g; s/<html_parseur::tree_builder::TreeBuilder>::/TreeBuilder::/g; s/<html_parseur::dom::Document>::/Document::/g; s/<html_parseur::atoms::Interner>::/Interner::/g' \
  > "$TMP/stacks.folded"
inferno-flamegraph --title "html-parseur · blog · $NAME ($SPEED)" \
  --subtitle "Largeur = part du temps CPU. Cliquer pour zoomer." \
  --colors rust --width 1400 --countname échantillons \
  < "$TMP/stacks.folded" > "profiling/$NAME.svg"
cp "$TMP/stacks.folded" "profiling/$NAME.folded"
echo "profiling/$NAME.svg ($SPEED)"
