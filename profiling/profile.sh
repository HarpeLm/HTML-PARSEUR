#!/bin/sh
# Profile la page blog et génère profiling/<nom>.svg
#   ./profiling/profile.sh blog-v1
# Nécessite : cargo install inferno rustfilt
set -e
NAME=${1:-blog}
TMP=$(mktemp -d)
cargo build -q --profile profiling --example profile_blog
./target/profiling/examples/profile_blog > "$TMP/speed.txt" &
PID=$!
sleep 0.3
sample $PID 3 1 -mayDie -file "$TMP/sample.txt" > /dev/null 2>&1
wait $PID
SPEED=$(sed -E 's/.*, //' "$TMP/speed.txt")
inferno-collapse-sample "$TMP/sample.txt" | rustfilt \
  | grep 'profile_blog::main' \
  | sed -E 's/^.*(profile_blog`profile_blog::main)/\1/; s/[a-z_]+`//g' \
  | sed -E 's/<html_tokenizer::tokenizer::Tokenizer as core::iter::traits::iterator::Iterator>::next/Tokenizer::next/g; s/<html_tokenizer::tokenizer::Tokenizer>::/Tokenizer::/g' \
  > "$TMP/stacks.folded"
inferno-flamegraph --title "html-tokenizer · blog · $NAME ($SPEED)" \
  --subtitle "Largeur = part du temps CPU. Cliquer pour zoomer." \
  --colors rust --width 1400 --countname échantillons \
  < "$TMP/stacks.folded" > "profiling/$NAME.svg"
cp "$TMP/stacks.folded" "profiling/$NAME.folded"
echo "profiling/$NAME.svg ($SPEED)"
